use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::adapter::ClaudeAdapter;
use crate::config::{self, Config, ConfigError};
use crate::mapping::{DagSource, MappingError, RoleMapping};
use crate::prompt;
use crate::store::Store;
use crate::tick::{RunRecord, TickReport, reconcile, run_tick};
use crate::tracker::{LazyspecRunner, LazyspecTracker};

const HOLDER: &str = "agentd";

type SharedState = Arc<Mutex<DaemonState>>;

#[derive(Default)]
struct DaemonState {
    records: Vec<RunRecord>,
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

/// Validate config, bind the control socket, and run one immediate orchestrator tick.
pub async fn start(
    config_path: &Path,
    socket_path: &Path,
    dag: &dyn DagSource,
) -> Result<Daemon, DaemonError> {
    let config = config::load(config_path).map_err(DaemonError::Config)?;
    RoleMapping::from_config(&config)
        .validate(dag)
        .map_err(DaemonError::Mapping)?;

    let listener = bind_control_socket(socket_path).await?;

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
    let template = read_template(&repo, &config);
    let tracker = LazyspecTracker::new(
        LazyspecRunner::new(repo.clone()),
        RoleMapping::from_config(&config),
    );
    let adapter = ClaudeAdapter::new(config.agent.auto_approve);
    let worker_state = state.clone();

    let mut worker_handles = Vec::new();
    worker_handles.push(tokio::spawn(async move {
        let store = match Store::open(&store_path) {
            Ok(store) => store,
            Err(_) => return,
        };
        let now = now_ms();
        let _ = reconcile(&store, &tracker, now);
        let report = run_tick(
            &tracker, &store, &adapter, &config, &template, &repo, now, HOLDER,
        )
        .await;
        if let TickReport::Dispatched(record) = report {
            worker_state.lock().unwrap().records.push(*record);
        }
    }));
    let workers_spawned = 1;

    let accept_state = state.clone();
    let accept_tx = shutdown_tx.clone();
    let accept_handle = tokio::spawn(accept_loop(listener, accept_state, accept_tx));

    Ok(Daemon {
        socket_path: socket_path.to_path_buf(),
        shutdown_tx,
        workers_spawned,
        accept_handle,
        worker_handles,
    })
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
            let items = state.lock().unwrap().records.iter().map(view).collect();
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
    use crate::mapping::StubDag;
    use tempfile::TempDir;

    fn write_config(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[tokio::test]
    async fn start_binds_socket_spawns_one_worker_and_reports_running() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(dir.path(), "");
        let socket_path = dir.path().join("agentd.sock");

        let daemon = start(&config_path, &socket_path, &StubDag::iteration())
            .await
            .unwrap();
        assert!(socket_path.exists());
        assert_eq!(daemon.workers_spawned(), 1);

        let items = query_status(&socket_path).await.unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.state, "Running");
        assert!(!item.identifier.is_empty());
        assert!(item.started_at_ms > 0);

        daemon.shutdown().await;
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn invalid_config_aborts_and_leaves_no_socket() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(dir.path(), "poll_interval_ms = 0");
        let socket_path = dir.path().join("agentd.sock");

        let err = start(&config_path, &socket_path, &StubDag::iteration())
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

        let err = start(&config_path, &socket_path, &StubDag::iteration())
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

        let err = start(&config_path, &socket_path, &StubDag::iteration())
            .await
            .unwrap_err();
        assert!(matches!(err, DaemonError::Mapping(_)), "{err}");
        assert!(err.to_string().contains("shipped"), "{err}");
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn shutdown_over_socket_stops_daemon_and_removes_socket() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(dir.path(), "");
        let socket_path = dir.path().join("agentd.sock");

        let daemon = start(&config_path, &socket_path, &StubDag::iteration())
            .await
            .unwrap();
        send_shutdown(&socket_path).await.unwrap();
        daemon.wait().await;
        daemon.shutdown().await;

        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn stale_socket_file_is_reclaimed() {
        let dir = TempDir::new().unwrap();
        let config_path = write_config(dir.path(), "");
        let socket_path = dir.path().join("agentd.sock");
        std::fs::write(&socket_path, b"stale").unwrap();

        let daemon = start(&config_path, &socket_path, &StubDag::iteration())
            .await
            .unwrap();
        assert_eq!(query_status(&socket_path).await.unwrap().len(), 1);
        daemon.shutdown().await;
    }
}
