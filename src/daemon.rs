use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::adapter::AgentAdapter;
use crate::config::{self, Config, ConfigError};
use crate::mapping::{DagSource, MappingError, RoleMapping};
use crate::projection::{EventKind, Projection, Snapshot};
use crate::prompt;
use crate::store::Store;
use crate::tick::{RunRecord, SkipClaim, TickReport, reconcile, run_tick};
use crate::tracker::{Candidate, Tracker};
use crate::workspace::DiskWorktrees;

const HOLDER: &str = "agentd";

type SharedState = Arc<Mutex<DaemonState>>;

#[derive(Default)]
struct DaemonState {
    running: Vec<RunningItem>,
    records: Vec<RunRecord>,
}

/// An item published as Running the moment it is claimed and activated, before
/// its turn finishes. It is dropped from `running` and replaced by a terminal
/// `RunRecord` once the tick resolves.
struct RunningItem {
    id: String,
    identifier: String,
    started_at_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Status,
    Log { iter_id: Option<String> },
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Status { items: Vec<ItemView> },
    Log { lines: Vec<String> },
    Ok,
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ItemView {
    pub id: String,
    pub identifier: String,
    pub state: String,
    pub transition: String,
    pub runtime_ms: u64,
    pub started_at_ms: u64,
}

#[derive(Debug)]
pub enum DaemonError {
    Config(ConfigError),
    Mapping(MappingError),
    AlreadyRunning(PathBuf),
    Bind { path: PathBuf, source: io::Error },
    Connect { path: PathBuf, source: io::Error },
    Io(io::Error),
    Protocol(String),
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DaemonError::Config(e) => write!(f, "{e}"),
            DaemonError::Mapping(e) => write!(f, "invalid dispatch mapping: {e}"),
            DaemonError::AlreadyRunning(path) => {
                write!(
                    f,
                    "agentd already running (socket {} is live)",
                    path.display()
                )
            }
            DaemonError::Bind { path, source } => {
                write!(f, "cannot bind control socket {}: {source}", path.display())
            }
            DaemonError::Connect { path, source } => {
                write!(f, "cannot reach agentd on {}: {source}", path.display())
            }
            DaemonError::Io(e) => write!(f, "{e}"),
            DaemonError::Protocol(msg) => write!(f, "control protocol error: {msg}"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DaemonError::Config(e) => Some(e),
            DaemonError::Mapping(e) => Some(e),
            DaemonError::Bind { source, .. } | DaemonError::Connect { source, .. } => Some(source),
            DaemonError::Io(e) => Some(e),
            DaemonError::AlreadyRunning(_) | DaemonError::Protocol(_) => None,
        }
    }
}

impl From<io::Error> for DaemonError {
    fn from(e: io::Error) -> Self {
        DaemonError::Io(e)
    }
}

impl From<serde_json::Error> for DaemonError {
    fn from(e: serde_json::Error) -> Self {
        DaemonError::Protocol(e.to_string())
    }
}

/// A running daemon: control socket bound, orchestrator ticked, workers spawned.
#[derive(Debug)]
pub struct Daemon {
    socket_path: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    workers_spawned: usize,
    accept_handle: JoinHandle<()>,
    worker_handles: Vec<JoinHandle<()>>,
}

impl Daemon {
    pub fn workers_spawned(&self) -> usize {
        self.workers_spawned
    }

    /// Await a shutdown request (control socket `shutdown` or Ctrl-C).
    pub async fn wait(&self) {
        let mut rx = self.shutdown_tx.subscribe();
        tokio::select! {
            _ = wait_true(&mut rx) => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }

    /// Signal shutdown, join tasks, and remove the socket file.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.accept_handle.await;
        for handle in self.worker_handles {
            let _ = handle.await;
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Validate config, bind the control socket, spawn the orchestrator worker, and
/// serve control requests. The tracker and adapter are injected (ADR-004): the
/// binary wires the live lazyspec tracker and claude adapter; tests substitute
/// fakes to exercise the whole composition without a live backend.
pub async fn start<T, A>(
    config_path: &Path,
    socket_path: &Path,
    dag: &dyn DagSource,
    tracker: T,
    adapter: A,
) -> Result<Daemon, DaemonError>
where
    T: Tracker + Send + Sync + 'static,
    A: AgentAdapter + Send + Sync + 'static,
    A::Session: Send,
{
    let config = config::load(config_path).map_err(DaemonError::Config)?;
    RoleMapping::from_config(&config)
        .validate(dag)
        .map_err(DaemonError::Mapping)?;

    let listener = bind_control_socket(socket_path).await?;

    let orchestrator = spawn_orchestrator(config_path, config, tracker, adapter);

    let accept_state = orchestrator.state.clone();
    let accept_tx = orchestrator.shutdown_tx.clone();
    let accept_handle = tokio::spawn(accept_loop(listener, accept_state, accept_tx));

    Ok(Daemon {
        socket_path: socket_path.to_path_buf(),
        shutdown_tx: orchestrator.shutdown_tx,
        workers_spawned: orchestrator.worker_handles.len(),
        accept_handle,
        worker_handles: orchestrator.worker_handles,
    })
}

/// The orchestrator half of the daemon: shared state, the shutdown channel, and
/// the worker task(s). Bound separately from the control socket so the dispatch
/// path is exercisable without binding — the socket bind is what a sandbox
/// blocks, not the orchestration.
struct Orchestrator {
    state: SharedState,
    shutdown_tx: watch::Sender<bool>,
    /// Seeds the worker's reloadable config source. The producer that pushes
    /// fresh config through this handle is STORY-050; until then it is dormant
    /// in production and only exercised by tests.
    #[allow(dead_code)]
    config_tx: watch::Sender<Config>,
    worker_handles: Vec<JoinHandle<()>>,
}

/// Reconcile the store once, then poll: run a tick, wait `poll_interval_ms`, and
/// tick again, until shutdown. Each tick publishes the claimed item as Running the
/// moment it is activated and replaces it with a terminal record when the turn
/// resolves. Ticks are sequential — the next wait starts only when the current
/// tick's post-commit seam completes, so no two ticks overlap. No socket is bound
/// here.
fn spawn_orchestrator<T, A>(
    config_path: &Path,
    config: Config,
    tracker: T,
    adapter: A,
) -> Orchestrator
where
    T: Tracker + Send + Sync + 'static,
    A: AgentAdapter + Send + Sync + 'static,
    A::Session: Send,
{
    let state: SharedState = Arc::new(Mutex::new(DaemonState::default()));
    let (shutdown_tx, _) = watch::channel(false);

    let store_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let repo = store_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| store_dir.clone());
    let store_path = store_dir.join("store.redb");
    let log_path = store_dir.join("log");
    let template = read_template(&repo, &config);
    let worker_state = state.clone();
    let (config_tx, config_rx) = watch::channel(config);
    let mut shutdown_rx = shutdown_tx.subscribe();

    let worker_handles = vec![tokio::spawn(async move {
        let store = match Store::open(&store_path) {
            Ok(store) => store,
            Err(_) => return,
        };
        let projection = Projection::new(log_path);
        let snapshot = Snapshot::new(store_dir);

        {
            let config = config_rx.borrow().clone();
            let now = now_ms();
            let mapping = RoleMapping::from_config(&config);
            let worktrees = DiskWorktrees::new(repo.join(&config.workspace.root));
            if let Ok(report) = reconcile(&store, &tracker, &worktrees, &mapping, now) {
                // A daemon restart, not a new commit: there is no live holder to
                // attribute the release to, so the line carries only the id. Expired
                // orphans, vanished-doc drops, completed finalizations, and claims
                // whose worktree vanished are all durable claim releases, so each is
                // projected the same way.
                for id in report
                    .released
                    .iter()
                    .chain(&report.dropped)
                    .chain(&report.finalized)
                    .chain(&report.abandoned)
                {
                    projection.record(now, id, EventKind::ReconcileRelease, &[]);
                    snapshot.remove_ref(id);
                }
                if let Ok(claims) = store.claims() {
                    snapshot.write_state(&claims);
                }
            }
        }

        loop {
            let now = now_ms();
            let config = config_rx.borrow().clone();

            let running_state = worker_state.clone();
            let on_activated = move |candidate: &Candidate| {
                running_state.lock().unwrap().running.push(RunningItem {
                    id: candidate.id.clone(),
                    identifier: candidate.identifier.clone(),
                    started_at_ms: now_ms(),
                });
            };

            let report = run_tick(
                &tracker,
                &store,
                &adapter,
                &config,
                &template,
                &repo,
                now,
                HOLDER,
                on_activated,
            )
            .await;

            match report {
                TickReport::Dispatched(record) => {
                    // Post-commit seam (ADR-002): the store transaction already
                    // committed inside `run_tick`/`claim_and_activate`; this only
                    // best-effort mirrors that outcome to the plain-text log.
                    projection.record(
                        record.claimed_at_ms,
                        &record.id,
                        EventKind::Claim,
                        &[("holder", &record.holder)],
                    );
                    if record.claim_released {
                        projection.record(
                            record.ended_at_ms,
                            &record.id,
                            EventKind::Release,
                            &[("holder", &record.holder)],
                        );
                        snapshot.remove_ref(&record.id);
                    } else if let Ok(Some(held)) = store.get(&record.id) {
                        snapshot.set_ref(&record.id, &held.holder, held.fence);
                    }
                    if let Ok(claims) = store.claims() {
                        snapshot.write_state(&claims);
                    }

                    let mut state = worker_state.lock().unwrap();
                    state.running.retain(|item| item.id != record.id);
                    state.records.push(*record);
                }
                // Same post-commit seam as above (STORY-019 AC1): a gated-out advance
                // still durably claims-then-releases before the tick reports Skipped,
                // so those commits must reach the log and snapshot too. A lost claim
                // never touched the store, so it projects nothing.
                TickReport::Skipped { id, claim, .. } => match claim {
                    SkipClaim::ClaimedThenReleased => {
                        projection.record(now, &id, EventKind::Claim, &[("holder", HOLDER)]);
                        projection.record(now, &id, EventKind::Release, &[("holder", HOLDER)]);
                        snapshot.remove_ref(&id);
                        if let Ok(claims) = store.claims() {
                            snapshot.write_state(&claims);
                        }
                    }
                    SkipClaim::ClaimedReleaseFailed => {
                        projection.record(now, &id, EventKind::Claim, &[("holder", HOLDER)]);
                        if let Ok(Some(held)) = store.get(&id) {
                            snapshot.set_ref(&id, &held.holder, held.fence);
                        }
                        if let Ok(claims) = store.claims() {
                            snapshot.write_state(&claims);
                        }
                    }
                    SkipClaim::NotClaimed => {}
                },
                TickReport::Idle | TickReport::Error(_) => {}
            }

            // Re-read the interval fresh so a reload governs the next wait only,
            // never an in-flight tick (ADR-006/ADR-007). The sleep races the
            // shutdown watch so the loop exits promptly instead of after a full wait.
            let interval = config_rx.borrow().poll_interval_ms;
            tokio::select! {
                _ = wait_true(&mut shutdown_rx) => break,
                _ = tokio::time::sleep(Duration::from_millis(interval)) => {}
            }
        }
    })];

    Orchestrator {
        state,
        shutdown_tx,
        config_tx,
        worker_handles,
    }
}

pub async fn query_status(socket_path: &Path) -> Result<Vec<ItemView>, DaemonError> {
    match request(socket_path, &Request::Status).await? {
        Response::Status { items } => Ok(items),
        Response::Error { message } => Err(DaemonError::Protocol(message)),
        _ => Err(DaemonError::Protocol("unexpected response".to_string())),
    }
}

pub async fn query_log(
    socket_path: &Path,
    iter_id: Option<String>,
) -> Result<Vec<String>, DaemonError> {
    match request(socket_path, &Request::Log { iter_id }).await? {
        Response::Log { lines } => Ok(lines),
        Response::Error { message } => Err(DaemonError::Protocol(message)),
        _ => Err(DaemonError::Protocol("unexpected response".to_string())),
    }
}

pub async fn send_shutdown(socket_path: &Path) -> Result<(), DaemonError> {
    match request(socket_path, &Request::Shutdown).await? {
        Response::Ok => Ok(()),
        Response::Error { message } => Err(DaemonError::Protocol(message)),
        _ => Err(DaemonError::Protocol("unexpected response".to_string())),
    }
}

fn read_template(repo: &Path, config: &Config) -> String {
    std::fs::read_to_string(repo.join(&config.prompt.template))
        .unwrap_or_else(|_| prompt::DEFAULT_TEMPLATE.to_string())
}

async fn bind_control_socket(path: &Path) -> Result<UnixListener, DaemonError> {
    if path.exists() {
        match UnixStream::connect(path).await {
            Ok(_) => return Err(DaemonError::AlreadyRunning(path.to_path_buf())),
            Err(_) => std::fs::remove_file(path).map_err(|source| DaemonError::Bind {
                path: path.to_path_buf(),
                source,
            })?,
        }
    }
    UnixListener::bind(path).map_err(|source| DaemonError::Bind {
        path: path.to_path_buf(),
        source,
    })
}

async fn accept_loop(listener: UnixListener, state: SharedState, shutdown_tx: watch::Sender<bool>) {
    let mut shutdown_rx = shutdown_tx.subscribe();
    loop {
        tokio::select! {
            _ = wait_true(&mut shutdown_rx) => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tokio::spawn(handle_conn(stream, state.clone(), shutdown_tx.clone()));
                }
                Err(_) => break,
            },
        }
    }
}

async fn handle_conn(stream: UnixStream, state: SharedState, shutdown_tx: watch::Sender<bool>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }

    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(Request::Status) => {
            let state = state.lock().unwrap();
            let items = state
                .running
                .iter()
                .map(running_view)
                .chain(state.records.iter().map(view))
                .collect();
            Response::Status { items }
        }
        Ok(Request::Log { iter_id }) => {
            let lines = state
                .lock()
                .unwrap()
                .records
                .iter()
                .filter(|r| iter_id.as_deref().is_none_or(|id| r.id == id))
                .flat_map(RunRecord::log_lines)
                .collect();
            Response::Log { lines }
        }
        Ok(Request::Shutdown) => {
            let _ = shutdown_tx.send(true);
            Response::Ok
        }
        Err(e) => Response::Error {
            message: format!("bad request: {e}"),
        },
    };

    let body = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
    let _ = write_half.write_all(body.as_bytes()).await;
    let _ = write_half.write_all(b"\n").await;
    let _ = write_half.flush().await;
}

async fn request(socket_path: &Path, req: &Request) -> Result<Response, DaemonError> {
    let stream = UnixStream::connect(socket_path)
        .await
        .map_err(|source| DaemonError::Connect {
            path: socket_path.to_path_buf(),
            source,
        })?;
    let (read_half, mut write_half) = stream.into_split();

    let body = serde_json::to_string(req)?;
    write_half.write_all(body.as_bytes()).await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await?;

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    Ok(serde_json::from_str(line.trim())?)
}

async fn wait_true(rx: &mut watch::Receiver<bool>) {
    if *rx.borrow() {
        return;
    }
    while rx.changed().await.is_ok() {
        if *rx.borrow() {
            return;
        }
    }
}

fn running_view(item: &RunningItem) -> ItemView {
    ItemView {
        id: item.id.clone(),
        identifier: item.identifier.clone(),
        state: "Running".to_string(),
        transition: "-".to_string(),
        runtime_ms: 0,
        started_at_ms: item.started_at_ms,
    }
}

fn view(record: &RunRecord) -> ItemView {
    ItemView {
        id: record.id.clone(),
        identifier: record.identifier.clone(),
        state: record.state_label(),
        transition: record.transition_target(),
        runtime_ms: record.runtime_ms(),
        started_at_ms: record.started_at_ms,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener as StdUnixListener;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::sync::Notify;

    use crate::adapter::{TurnOutcome, TurnReport};
    use crate::agent::AgentEvent;
    use crate::config::load_str;
    use crate::mapping::StubDag;
    use crate::tracker::{DocLookup, DocView, TrackerError};
    use crate::workspace::Worktree;

    /// A tracker fake offering one candidate, a canned parent for prompt
    /// assembly, and recording every advance.
    struct FakeTracker {
        candidate: Candidate,
        parent: DocView,
        fetches: Arc<AtomicUsize>,
    }

    impl Tracker for FakeTracker {
        fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError> {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            Ok(vec![self.candidate.clone()])
        }

        fn fetch_doc(&self, _id: &str) -> Result<DocView, TrackerError> {
            Ok(self.parent.clone())
        }

        fn lookup_doc(&self, _id: &str) -> Result<DocLookup, TrackerError> {
            Ok(DocLookup::Present(self.parent.clone()))
        }

        fn advance(&self, _id: &str, _target: &str) -> Result<(), TrackerError> {
            Ok(())
        }
    }

    /// An adapter fake whose turn blocks on a gate until the test releases it, so
    /// the item is observably Running while the turn is in flight.
    struct BlockingAdapter {
        gate: Arc<Notify>,
    }

    impl AgentAdapter for BlockingAdapter {
        type Session = Worktree;

        fn start_session(&self, worktree: Worktree) -> Worktree {
            worktree
        }

        async fn run_turn(&self, _session: &Worktree, _prompt: &str) -> TurnReport {
            self.gate.notified().await;
            TurnReport {
                outcome: TurnOutcome::Completed,
                events: vec![AgentEvent::TurnCompleted { pid: 1, at_ms: 1 }],
            }
        }

        async fn stop(&self, _session: Worktree) {}
    }

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    }

    /// A git repo with a `.agentd/` store dir holding the config, mirroring the
    /// real layout so `spawn_orchestrator`'s `repo = store_dir.parent()` lands on
    /// the git root that `prepare_worktree` needs.
    fn init_project(config_body: &str) -> (TempDir, PathBuf, PathBuf) {
        let repo = TempDir::new().unwrap();
        git_ok(repo.path(), &["init", "-q"]);
        git_ok(repo.path(), &["config", "user.email", "test@example.com"]);
        git_ok(repo.path(), &["config", "user.name", "agentd test"]);
        std::fs::write(repo.path().join("README.md"), "base").unwrap();
        git_ok(repo.path(), &["add", "."]);
        git_ok(repo.path(), &["commit", "-q", "-m", "base"]);

        let store_dir = repo.path().join(".agentd");
        std::fs::create_dir_all(&store_dir).unwrap();
        let config_path = store_dir.join("config.toml");
        std::fs::write(&config_path, config_body).unwrap();
        let socket_path = store_dir.join("agentd.sock");
        (repo, config_path, socket_path)
    }

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    /// A `FakeTracker` plus a handle to its fetch counter. `fetch_dispatchable`
    /// is called once by reconcile at startup and once per tick, so the count is
    /// a faithful per-tick signal (STORY-002 AC1/AC3).
    fn fake_tracker_counting() -> (FakeTracker, Arc<AtomicUsize>) {
        let tracker = fake_tracker();
        let fetches = tracker.fetches.clone();
        (tracker, fetches)
    }

    fn fake_tracker() -> FakeTracker {
        FakeTracker {
            fetches: Arc::new(AtomicUsize::new(0)),
            candidate: Candidate {
                id: "ITER-014".to_string(),
                identifier: "execute-one-iteration".to_string(),
                title: "Execute one iteration end-to-end".to_string(),
                body: "Objective: prove the full path.".to_string(),
                state: "accepted".to_string(),
                parent: Some("STORY-060".to_string()),
                dependencies: Vec::new(),
            },
            parent: DocView {
                id: "STORY-060".to_string(),
                doc_type: "story".to_string(),
                title: "Execute one iteration end-to-end".to_string(),
                body: "As an operator, I want one eligible iteration to flow.".to_string(),
                status: "in-progress".to_string(),
            },
        }
    }

    fn blocking_adapter() -> (BlockingAdapter, Arc<Notify>) {
        let gate = Arc::new(Notify::new());
        (BlockingAdapter { gate: gate.clone() }, gate)
    }

    /// The sandbox denies `AF_UNIX` bind (Operation not permitted). Socket
    /// round-trip tests probe for it and skip rather than fail; the socket-free
    /// orchestration test below still covers the dispatch path there.
    fn sandbox_blocks_bind(dir: &Path) -> bool {
        let probe = dir.join(".bind-probe.sock");
        match StdUnixListener::bind(&probe) {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                false
            }
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => true,
            Err(_) => false,
        }
    }

    async fn wait_for_status(socket_path: &Path) -> Vec<ItemView> {
        for _ in 0..200 {
            let items = query_status(socket_path).await.unwrap();
            if !items.is_empty() {
                return items;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no status item appeared");
    }

    async fn wait_for_running(state: &SharedState) {
        for _ in 0..400 {
            if !state.lock().unwrap().running.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no running item appeared");
    }

    async fn wait_for_records(state: &SharedState, n: usize) {
        for _ in 0..400 {
            if state.lock().unwrap().records.len() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("fewer than {n} records accumulated");
    }

    async fn wait_for_fetches(fetches: &Arc<AtomicUsize>, n: usize) {
        for _ in 0..400 {
            if fetches.load(Ordering::SeqCst) >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("fewer than {n} tick fetches");
    }

    async fn drain_and_shutdown(orch: Orchestrator, gate: &Arc<Notify>) {
        let _ = orch.shutdown_tx.send(true);
        gate.notify_one();
        for handle in orch.worker_handles {
            handle.await.unwrap();
        }
    }

    // STORY-002 AC1: once the first tick completes, the loop waits the interval
    // and ticks again. A completed run keeps its claim, so a re-offered candidate
    // is skipped rather than re-dispatched — the faithful "another tick ran"
    // signal is a further fetch_dispatchable call, not a second record.
    #[tokio::test]
    async fn a_second_tick_runs_after_the_first_completes() {
        let (_repo, config_path, _socket) = init_project("");
        let (adapter, gate) = blocking_adapter();
        let (tracker, fetches) = fake_tracker_counting();
        let mut config = load_str("").unwrap();
        config.poll_interval_ms = 5;
        let orch = spawn_orchestrator(&config_path, config, tracker, adapter);

        wait_for_running(&orch.state).await;
        let after_first_tick = fetches.load(Ordering::SeqCst);
        gate.notify_one();
        wait_for_records(&orch.state, 1).await;

        wait_for_fetches(&fetches, after_first_tick + 1).await;

        drain_and_shutdown(orch, &gate).await;
    }

    // STORY-002 AC2: ticks never overlap. The turn stays gated shut while the
    // short interval elapses many times over, yet exactly one turn is ever in
    // flight and nothing completes — a concurrent model would have started more.
    #[tokio::test]
    async fn ticks_do_not_overlap_while_one_is_in_flight() {
        let (_repo, config_path, _socket) = init_project("");
        let (adapter, gate) = blocking_adapter();
        let mut config = load_str("").unwrap();
        config.poll_interval_ms = 5;
        let orch = spawn_orchestrator(&config_path, config, fake_tracker(), adapter);

        wait_for_running(&orch.state).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        {
            let state = orch.state.lock().unwrap();
            assert_eq!(
                state.running.len(),
                1,
                "one turn in flight; the next tick must not start until it completes"
            );
            assert!(
                state.records.is_empty(),
                "no tick may complete while the first is still blocked"
            );
        }

        drain_and_shutdown(orch, &gate).await;
    }

    // STORY-002 AC3: the interval is re-read each cycle. Starting from a 10-minute
    // interval, updating the shared config while the first tick is in flight makes
    // the next wait 5ms, so the following tick fetches well inside the test budget
    // — impossible if the stale 10-minute value still governed the wait.
    #[tokio::test]
    async fn a_reloaded_interval_governs_the_next_wait() {
        let (_repo, config_path, _socket) = init_project("");
        let (adapter, gate) = blocking_adapter();
        let (tracker, fetches) = fake_tracker_counting();
        let mut slow = load_str("").unwrap();
        slow.poll_interval_ms = 600_000;
        let orch = spawn_orchestrator(&config_path, slow, tracker, adapter);

        wait_for_running(&orch.state).await;
        let after_first_tick = fetches.load(Ordering::SeqCst);

        let mut fast = load_str("").unwrap();
        fast.poll_interval_ms = 5;
        orch.config_tx.send(fast).unwrap();

        gate.notify_one();
        wait_for_records(&orch.state, 1).await;
        wait_for_fetches(&fetches, after_first_tick + 1).await;

        drain_and_shutdown(orch, &gate).await;
    }

    // The dispatch path — claim, publish Running, run the turn — without binding a
    // control socket, so it is exercised even where the sandbox blocks bind.
    #[tokio::test]
    async fn orchestrator_publishes_running_then_records_terminal() {
        let (_repo, config_path, _socket) = init_project("");
        let (adapter, gate) = blocking_adapter();
        let orch = spawn_orchestrator(&config_path, load_str("").unwrap(), fake_tracker(), adapter);

        let mut running = None;
        for _ in 0..200 {
            if let Some(item) = orch.state.lock().unwrap().running.first() {
                running = Some(running_view(item));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let running = running.expect("running item never appeared");
        assert_eq!(running.state, "Running");
        assert_eq!(running.identifier, "execute-one-iteration");
        assert!(running.started_at_ms > 0);

        // The poll loop never exits on its own; shut it down after this one tick.
        let _ = orch.shutdown_tx.send(true);
        gate.notify_one();
        for handle in orch.worker_handles {
            handle.await.unwrap();
        }

        let state = orch.state.lock().unwrap();
        assert!(state.running.is_empty(), "running item must be cleared");
        assert_eq!(state.records.len(), 1);
        assert_eq!(state.records[0].id, "ITER-014");
    }

    // STORY-016 AC1/AC3: startup reconcile releases every expired orphan and
    // projects one release line per id, so an operator sees each on restart.
    // Socket-free, so it runs where the sandbox blocks bind.
    #[tokio::test]
    async fn reconcile_projects_a_release_line_for_each_orphan() {
        let (_repo, config_path, _socket) = init_project("");
        let store_dir = config_path.parent().unwrap().to_path_buf();
        let log_path = store_dir.join("log");

        // Two claims left by a dead daemon, both long expired and neither offered
        // by the tracker, so reconcile must release both as orphans.
        {
            let store = Store::open(&store_dir.join("store.redb")).unwrap();
            store
                .claim("ITER-100", "dead", 0, Duration::from_millis(1))
                .unwrap();
            store
                .claim("ITER-101", "dead", 0, Duration::from_millis(1))
                .unwrap();
        }

        let (adapter, gate) = blocking_adapter();
        let orch =
            spawn_orchestrator(&config_path, load_str("").unwrap(), fake_tracker(), adapter);
        // Reconcile is the one-time prelude; end the loop after it and the first tick.
        let _ = orch.shutdown_tx.send(true);
        gate.notify_one();
        for handle in orch.worker_handles {
            handle.await.unwrap();
        }

        let contents = std::fs::read_to_string(&log_path).unwrap();
        let mut released: Vec<&str> = contents
            .lines()
            .filter(|l| l.contains("event=reconcile_release"))
            .map(|l| {
                l.split(' ')
                    .find_map(|kv| kv.strip_prefix("iter="))
                    .expect("a reconcile_release line carries an iter id")
            })
            .collect();
        released.sort();
        assert_eq!(
            released,
            vec!["ITER-100", "ITER-101"],
            "one release line per orphan: {contents}"
        );

        let store = Store::open(&store_dir.join("store.redb")).unwrap();
        assert_eq!(store.get("ITER-100").unwrap(), None);
        assert_eq!(store.get("ITER-101").unwrap(), None);
    }

    #[tokio::test]
    async fn start_binds_socket_spawns_one_worker_and_reports_running() {
        let (_repo, config_path, socket_path) = init_project("");
        if sandbox_blocks_bind(socket_path.parent().unwrap()) {
            eprintln!("skipping: sandbox blocks AF_UNIX bind");
            return;
        }
        let (adapter, gate) = blocking_adapter();

        let daemon = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap();
        assert!(socket_path.exists());
        assert_eq!(daemon.workers_spawned(), 1);

        let items = wait_for_status(&socket_path).await;
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.state, "Running");
        assert!(!item.identifier.is_empty());
        assert!(item.started_at_ms > 0);

        gate.notify_one();
        daemon.shutdown().await;
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn invalid_config_aborts_and_leaves_no_socket() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(dir.path(), "poll_interval_ms = 0");
        let socket_path = dir.path().join("agentd.sock");
        let (adapter, _gate) = blocking_adapter();

        let err = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DaemonError::Config(_)), "{err}");
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn missing_config_aborts_and_leaves_no_socket() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("absent.toml");
        let socket_path = dir.path().join("agentd.sock");
        let (adapter, _gate) = blocking_adapter();

        let err = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DaemonError::Config(_)), "{err}");
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn mapping_referencing_a_missing_dag_state_aborts_and_leaves_no_socket() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(
            dir.path(),
            "[dispatch]\ntypes = [\"iteration\"]\n[dispatch.states]\nshipped = \"terminal\"\n",
        );
        let socket_path = dir.path().join("agentd.sock");
        let (adapter, _gate) = blocking_adapter();

        let err = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, DaemonError::Mapping(_)), "{err}");
        assert!(err.to_string().contains("shipped"), "{err}");
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn shutdown_over_socket_stops_daemon_and_removes_socket() {
        let (_repo, config_path, socket_path) = init_project("");
        if sandbox_blocks_bind(socket_path.parent().unwrap()) {
            eprintln!("skipping: sandbox blocks AF_UNIX bind");
            return;
        }
        let (adapter, gate) = blocking_adapter();

        let daemon = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap();
        send_shutdown(&socket_path).await.unwrap();
        gate.notify_one();
        daemon.wait().await;
        daemon.shutdown().await;

        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn stale_socket_file_is_reclaimed() {
        let (_repo, config_path, socket_path) = init_project("");
        if sandbox_blocks_bind(socket_path.parent().unwrap()) {
            eprintln!("skipping: sandbox blocks AF_UNIX bind");
            return;
        }
        std::fs::write(&socket_path, b"stale").unwrap();
        let (adapter, gate) = blocking_adapter();

        let daemon = start(
            &config_path,
            &socket_path,
            &StubDag::iteration(),
            fake_tracker(),
            adapter,
        )
        .await
        .unwrap();
        assert_eq!(wait_for_status(&socket_path).await.len(), 1);
        gate.notify_one();
        daemon.shutdown().await;
    }
}
