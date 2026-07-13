use std::process::{ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::agent::{self, AgentEvent};
use crate::workspace::Worktree;

/// One agent backend behind the canonical `AgentEvent` model (ADR-004). The
/// claude adapter drives one-shot `claude -p` turns — the process exits after a
/// turn — while a persistent backend (codex) would keep a live process across
/// turns behind the same three methods.
#[allow(async_fn_in_trait)]
pub trait AgentAdapter {
    type Session;

    fn start_session(&self, worktree: Worktree) -> Self::Session;

    async fn run_turn(&self, session: &Self::Session, prompt: &str) -> TurnReport;

    async fn stop(&self, session: Self::Session);
}

/// The canonical events observed during a turn plus the adapter's authoritative
/// verdict on how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReport {
    pub outcome: TurnOutcome,
    pub events: Vec<AgentEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    Failed { reason: String },
    LaunchError { message: String },
}

/// A one-shot `claude -p` session: no long-lived process, just the worktree the
/// turn runs in. `run_turn` spawns a fresh child per invocation.
pub struct ClaudeSession {
    worktree: Worktree,
}

/// Launches the configured agent program with the worktree as cwd and the
/// rendered prompt on stdin, folding its stream-json stdout into `AgentEvent`s.
///
/// `program`/`base_args` are injectable so tests substitute a fake command; the
/// default targets `claude` in the non-interactive stream-json print mode.
pub struct ClaudeAdapter {
    program: String,
    base_args: Vec<String>,
}

impl ClaudeAdapter {
    pub fn new(auto_approve: bool) -> Self {
        let mut base_args = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ];
        if auto_approve {
            base_args.push("--dangerously-skip-permissions".to_string());
        }
        ClaudeAdapter {
            program: "claude".to_string(),
            base_args,
        }
    }

    #[cfg(test)]
    pub fn with_program(program: impl Into<String>, base_args: Vec<String>) -> Self {
        ClaudeAdapter {
            program: program.into(),
            base_args,
        }
    }
}

impl AgentAdapter for ClaudeAdapter {
    type Session = ClaudeSession;

    fn start_session(&self, worktree: Worktree) -> ClaudeSession {
        ClaudeSession { worktree }
    }

    async fn run_turn(&self, session: &ClaudeSession, prompt: &str) -> TurnReport {
        let mut child = match Command::new(&self.program)
            .args(&self.base_args)
            .current_dir(&session.worktree.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                return TurnReport {
                    outcome: TurnOutcome::LaunchError {
                        message: format!("cannot launch `{}`: {e}", self.program),
                    },
                    events: Vec::new(),
                };
            }
        };

        let pid = child.id().unwrap_or(0);

        if let Some(mut stdin) = child.stdin.take() {
            let prompt = prompt.to_owned();
            tokio::spawn(async move {
                let _ = stdin.write_all(prompt.as_bytes()).await;
            });
        }

        let mut events = Vec::new();
        let mut success_result = false;
        let mut result_error = None;
        let mut cancelled = false;

        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match agent::map_line(&line, pid, now_ms()) {
                    AgentEvent::TurnCompleted { .. } => success_result = true,
                    AgentEvent::TurnFailed { reason, .. } => result_error = Some(reason),
                    AgentEvent::TurnCancelled { .. } => cancelled = true,
                    other => events.push(other),
                }
            }
        }

        let status = child.wait().await;
        let at_ms = now_ms();

        let outcome = match status {
            Ok(status)
                if status.success() && success_result && result_error.is_none() && !cancelled =>
            {
                events.push(AgentEvent::TurnCompleted { pid, at_ms });
                TurnOutcome::Completed
            }
            Ok(status) => {
                let reason = failure_reason(Some(status), success_result, result_error, cancelled);
                events.push(AgentEvent::TurnFailed {
                    pid,
                    at_ms,
                    reason: reason.clone(),
                });
                TurnOutcome::Failed { reason }
            }
            Err(e) => {
                let reason = format!("cannot wait for agent: {e}");
                events.push(AgentEvent::TurnFailed {
                    pid,
                    at_ms,
                    reason: reason.clone(),
                });
                TurnOutcome::Failed { reason }
            }
        };

        TurnReport { outcome, events }
    }

    async fn stop(&self, _session: ClaudeSession) {}
}

fn failure_reason(
    status: Option<ExitStatus>,
    success_result: bool,
    result_error: Option<String>,
    cancelled: bool,
) -> String {
    if let Some(reason) = result_error {
        return reason;
    }
    if cancelled {
        return "turn cancelled".to_string();
    }
    match status {
        Some(status) if !status.success() => match status.code() {
            Some(code) => format!("agent exited with status {code}"),
            None => "agent terminated by signal".to_string(),
        },
        _ if !success_result => "turn ended without a success result".to_string(),
        _ => "turn failed".to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::TempDir;

    const SYSTEM_INIT: &str =
        r#"{"type":"system","subtype":"init","session_id":"sess-1","cwd":"/x","tools":[]}"#;
    const ASSISTANT: &str =
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"working"}]}}"#;
    const RESULT_SUCCESS: &str =
        r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#;

    fn write_fake(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake-agent");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path
    }

    fn worktree(dir: &Path) -> Worktree {
        Worktree {
            path: dir.to_path_buf(),
            branch: "agentd/test".to_string(),
        }
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap()
    }

    async fn run(adapter: &ClaudeAdapter, wt: Worktree, prompt: &str) -> TurnReport {
        let session = adapter.start_session(wt);
        let report =
            tokio::time::timeout(Duration::from_secs(10), adapter.run_turn(&session, prompt))
                .await
                .expect("run_turn hung");
        adapter.stop(session).await;
        report
    }

    #[tokio::test]
    async fn clean_success_runs_in_worktree_delivers_prompt_and_completes() {
        let scratch = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();
        let cwd_file = scratch.path().join("cwd.txt");
        let stdin_file = scratch.path().join("stdin.txt");
        let args_file = scratch.path().join("args.txt");
        let body = format!(
            "#!/bin/sh\npwd > '{cwd}'\nprintf '%s\\n' \"$@\" > '{args}'\ncat > '{stdin}'\n\
             printf '%s\\n' '{sys}'\nprintf '%s\\n' '{asst}'\nprintf '%s\\n' '{res}'\nexit 0\n",
            cwd = cwd_file.display(),
            args = args_file.display(),
            stdin = stdin_file.display(),
            sys = SYSTEM_INIT,
            asst = ASSISTANT,
            res = RESULT_SUCCESS,
        );
        let program = write_fake(scratch.path(), &body);
        let adapter = ClaudeAdapter::with_program(
            program.to_str().unwrap(),
            vec!["-p".to_string(), "--output-format".to_string()],
        );

        let report = run(&adapter, worktree(wt_dir.path()), "PROMPT-XYZ").await;

        assert_eq!(report.outcome, TurnOutcome::Completed);
        assert!(
            report
                .events
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnCompleted { .. })),
            "turn_completed must be emitted: {:?}",
            report.events
        );
        assert!(
            report
                .events
                .iter()
                .any(|e| matches!(e, AgentEvent::SessionStarted { .. })),
            "{:?}",
            report.events
        );
        assert!(
            report
                .events
                .iter()
                .any(|e| matches!(e, AgentEvent::Notification { .. })),
            "{:?}",
            report.events
        );

        assert_eq!(
            canonical(Path::new(
                std::fs::read_to_string(&cwd_file).unwrap().trim()
            )),
            canonical(wt_dir.path()),
            "child cwd must be the worktree",
        );
        assert_eq!(
            std::fs::read_to_string(&stdin_file).unwrap(),
            "PROMPT-XYZ",
            "the rendered prompt must reach the child on stdin",
        );
        let args = std::fs::read_to_string(&args_file).unwrap();
        assert!(
            args.contains("-p") && args.contains("--output-format"),
            "{args}"
        );
    }

    #[tokio::test]
    async fn non_zero_exit_reports_failure() {
        let scratch = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();
        let body = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{sys}'\nprintf '%s\\n' '{asst}'\nexit 3\n",
            sys = SYSTEM_INIT,
            asst = ASSISTANT,
        );
        let program = write_fake(scratch.path(), &body);
        let adapter = ClaudeAdapter::with_program(program.to_str().unwrap(), vec![]);

        let report = run(&adapter, worktree(wt_dir.path()), "p").await;

        match report.outcome {
            TurnOutcome::Failed { reason } => assert!(reason.contains("status 3"), "{reason}"),
            other => panic!("expected Failed, got {other:?}"),
        }
        assert!(
            !report
                .events
                .iter()
                .any(|e| matches!(e, AgentEvent::TurnCompleted { .. })),
        );
    }

    #[tokio::test]
    async fn clean_exit_without_success_result_reports_failure() {
        let scratch = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();
        let body = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{sys}'\nprintf '%s\\n' '{asst}'\nexit 0\n",
            sys = SYSTEM_INIT,
            asst = ASSISTANT,
        );
        let program = write_fake(scratch.path(), &body);
        let adapter = ClaudeAdapter::with_program(program.to_str().unwrap(), vec![]);

        let report = run(&adapter, worktree(wt_dir.path()), "p").await;

        match report.outcome {
            TurnOutcome::Failed { reason } => assert!(!reason.is_empty(), "{reason}"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn result_error_line_reports_failure_with_its_reason() {
        let scratch = TempDir::new().unwrap();
        let wt_dir = TempDir::new().unwrap();
        let error_line = r#"{"type":"result","subtype":"error_input_required","is_error":true}"#;
        let body = format!(
            "#!/bin/sh\ncat > /dev/null\nprintf '%s\\n' '{sys}'\nprintf '%s\\n' '{err}'\nexit 0\n",
            sys = SYSTEM_INIT,
            err = error_line,
        );
        let program = write_fake(scratch.path(), &body);
        let adapter = ClaudeAdapter::with_program(program.to_str().unwrap(), vec![]);

        let report = run(&adapter, worktree(wt_dir.path()), "p").await;

        match report.outcome {
            TurnOutcome::Failed { reason } => {
                assert!(reason.contains("input_required"), "{reason}")
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn missing_binary_surfaces_launch_error_without_hanging() {
        let wt_dir = TempDir::new().unwrap();
        let adapter =
            ClaudeAdapter::with_program("/nonexistent/definitely/not/here/claude-xyz", vec![]);

        let report = run(&adapter, worktree(wt_dir.path()), "p").await;

        assert!(
            matches!(report.outcome, TurnOutcome::LaunchError { .. }),
            "{:?}",
            report.outcome
        );
        assert!(report.events.is_empty());
    }
}
