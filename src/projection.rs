use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;

/// The class of durable state change a line records (ADR-002 projection).
///
/// `Reclaim` and `Heartbeat` are not fired yet: no caller distinguishes a
/// fresh claim from a reclaim of an expired lease, and heartbeat is not wired
/// into the daemon tick loop as of this iteration. The variants exist so
/// those future call sites (STORY-014's heartbeat wiring) have a kind ready.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Claim,
    Reclaim,
    Release,
    Heartbeat,
    ReconcileRelease,
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventKind::Claim => "claim",
            EventKind::Reclaim => "reclaim",
            EventKind::Release => "release",
            EventKind::Heartbeat => "heartbeat",
            EventKind::ReconcileRelease => "reconcile_release",
        };
        write!(f, "{s}")
    }
}

/// Append-only sink for `.agentd/log` (ADR-002): every durable state change gets
/// one `ts=<ms> iter=<id> event=<kind> [k=v...]` line. The store commits first;
/// this is the best-effort, offline-inspectable projection of that commit, not
/// the authority — so `record` cannot fail the caller (see below).
pub struct Projection {
    path: PathBuf,
}

impl Projection {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Append one line for a committed state change.
    ///
    /// Returns nothing: a write failure (missing/unwritable directory, full
    /// disk, ...) is warned to stderr and swallowed rather than surfaced,
    /// because by the time this is called the store has already committed —
    /// the state change happened regardless of whether it got projected.
    pub fn record(&self, now_ms: u64, iter_id: &str, kind: EventKind, fields: &[(&str, &str)]) {
        let mut line = format!("ts={now_ms} iter={iter_id} event={kind}");
        for (k, v) in fields {
            line.push(' ');
            line.push_str(k);
            line.push('=');
            line.push_str(v);
        }
        line.push('\n');

        if let Err(e) = self.append(&line) {
            eprintln!(
                "agentd: warning: could not append event log line to {}: {e}",
                self.path.display()
            );
        }
    }

    fn append(&self, line: &str) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn parse_line(line: &str) -> HashMap<&str, &str> {
        line.split(' ')
            .map(|kv| kv.split_once('=').expect("every field is key=value"))
            .collect()
    }

    #[test]
    fn a_state_change_appends_one_parseable_line_with_the_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("log");
        let projection = Projection::new(&path);

        projection.record(1000, "ITER-016", EventKind::Claim, &[("holder", "agent-a")]);

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let fields = parse_line(lines[0]);
        assert_eq!(fields["ts"], "1000");
        assert_eq!(fields["iter"], "ITER-016");
        assert_eq!(fields["event"], "claim");
        assert_eq!(fields["holder"], "agent-a");
        assert!(contents.ends_with('\n'), "line must be newline-terminated");
    }

    #[cfg(unix)]
    #[test]
    fn write_failure_does_not_panic_and_does_not_error_the_caller() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).unwrap();

        let path = dir.path().join("log");
        let projection = Projection::new(&path);
        // `record` returns nothing: this call is the assertion. A panic or an
        // unhandled Result would fail the test on its own.
        projection.record(1000, "ITER-016", EventKind::Release, &[]);

        let mut restore = fs::metadata(dir.path()).unwrap().permissions();
        restore.set_mode(0o755);
        fs::set_permissions(dir.path(), restore).unwrap();

        assert!(!path.exists(), "a denied write must not create the file");
    }

    #[test]
    fn appends_across_reopen_retaining_earlier_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("log");

        {
            let projection = Projection::new(&path);
            projection.record(1000, "ITER-001", EventKind::Claim, &[("holder", "agent-a")]);
        }
        {
            let projection = Projection::new(&path);
            projection.record(
                2000,
                "ITER-002",
                EventKind::Release,
                &[("holder", "agent-a")],
            );
        }

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2, "earlier line must survive the reopen");
        assert_eq!(parse_line(lines[0])["iter"], "ITER-001");
        assert_eq!(parse_line(lines[1])["iter"], "ITER-002");
    }

    // Task 4 / verification: a crash mid-write leaves an unterminated trailing
    // line. Since every complete line ends in '\n', a reader can tell complete
    // lines from a dangling partial one by that terminator alone — no rewrite
    // or truncation is ever needed to recover.
    #[test]
    fn a_partial_trailing_line_is_distinguishable_from_complete_lines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("log");
        let projection = Projection::new(&path);
        projection.record(1000, "ITER-001", EventKind::Claim, &[("holder", "agent-a")]);

        // Simulate a crash mid-append: bytes landed on disk with no trailing
        // newline yet.
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"ts=2000 iter=ITER-002 event=cla").unwrap();
        drop(file);

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            !contents.ends_with('\n'),
            "the simulated crash leaves a dangling partial line"
        );

        let mut complete_lines: Vec<&str> = contents.lines().collect();
        if !contents.ends_with('\n') {
            complete_lines.pop();
        }
        assert_eq!(
            complete_lines,
            vec!["ts=1000 iter=ITER-001 event=claim holder=agent-a"]
        );
    }
}
