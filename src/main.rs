use clap::{Parser, Subcommand};

mod config;
mod daemon;
#[allow(dead_code)]
mod dispatch;
mod init;
mod mapping;
#[allow(dead_code)]
mod store;
#[allow(dead_code)]
mod tracker;
#[allow(dead_code)]
mod workspace;

/// agentd — a git-like daemon that orchestrates coding agents against a lazyspec backlog.
#[derive(Parser)]
#[command(name = "agentd", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create the .agentd/ store.
    Init,
    /// Start the daemon.
    Start,
    /// Stop the daemon.
    Stop,
    /// Show running sessions and the retry queue.
    Status,
    /// Print the append-only event stream, optionally for one iteration.
    Log { iter_id: Option<String> },
    /// Show per-ticket detail for one iteration.
    Show { iter_id: String },
    /// Print the effective config after reload.
    Config,
    /// Force a poll and reconcile tick.
    Refresh,
    /// Manually dispatch a ticket.
    Assign { iter_id: String },
    /// Stop a running agent.
    Cancel { iter_id: String },
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Init => "init",
            Command::Start => "start",
            Command::Stop => "stop",
            Command::Status => "status",
            Command::Log { .. } => "log",
            Command::Show { .. } => "show",
            Command::Config => "config",
            Command::Refresh => "refresh",
            Command::Assign { .. } => "assign",
            Command::Cancel { .. } => "cancel",
        }
    }
}

async fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Init => run_init(),
        Command::Config => run_config(),
        Command::Start => run_start().await,
        Command::Status => run_status().await,
        Command::Stop => run_stop().await,
        other => {
            println!("unimplemented: {}", other.name());
            Ok(())
        }
    }
}

fn store_dir() -> Result<std::path::PathBuf, String> {
    std::env::current_dir()
        .map(|cwd| cwd.join(init::STORE_DIR))
        .map_err(|e| format!("cannot determine current directory: {e}"))
}

async fn run_start() -> Result<(), String> {
    let store = store_dir()?;
    let config_path = store.join("config.toml");
    let socket_path = store.join("agentd.sock");

    let project_root = store.parent().unwrap_or(&store).to_path_buf();
    let dag = mapping::LazyspecCli::new(project_root);
    let daemon = daemon::start(&config_path, &socket_path, &dag)
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "agentd started (socket {}, {} worker(s))",
        socket_path.display(),
        daemon.workers_spawned()
    );

    daemon.wait().await;
    daemon.shutdown().await;
    println!("agentd stopped");
    Ok(())
}

async fn run_status() -> Result<(), String> {
    let socket_path = store_dir()?.join("agentd.sock");
    let items = daemon::query_status(&socket_path)
        .await
        .map_err(|e| e.to_string())?;
    if items.is_empty() {
        println!("no running items");
    }
    for item in items {
        println!(
            "{}\t{}\t{}\tstarted_at={}",
            item.id, item.state, item.identifier, item.started_at_ms
        );
    }
    Ok(())
}

async fn run_stop() -> Result<(), String> {
    let socket_path = store_dir()?.join("agentd.sock");
    daemon::send_shutdown(&socket_path)
        .await
        .map_err(|e| e.to_string())?;
    println!("stop signal sent");
    Ok(())
}

fn run_init() -> Result<(), String> {
    let root =
        std::env::current_dir().map_err(|e| format!("cannot determine current directory: {e}"))?;
    match init::init(&root) {
        Ok(report) if report.already_initialized() => {
            println!("agentd already initialized at {}", report.store.display());
            for path in &report.existing {
                println!("  exists: {}", path.display());
            }
            Ok(())
        }
        Ok(report) => {
            println!("Initialized agentd store at {}", report.store.display());
            for path in &report.created {
                println!("  created: {}", path.display());
            }
            for path in &report.existing {
                println!("  exists (kept): {}", path.display());
            }
            Ok(())
        }
        Err(e) => Err(format!(
            "failed to initialize agentd store in {}: {e}",
            root.display()
        )),
    }
}

fn run_config() -> Result<(), String> {
    let path = std::env::current_dir()
        .map_err(|e| format!("cannot determine current directory: {e}"))?
        .join(init::STORE_DIR)
        .join("config.toml");
    if !path.exists() {
        println!("agentd is not initialized (no {})", path.display());
        return Ok(());
    }
    match config::load(&path) {
        Ok(cfg) => {
            println!("{cfg:#?}");
            Ok(())
        }
        Err(e) => Err(format!("{e}")),
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    run(cli.command).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[tokio::test]
    async fn parses_every_porcelain_subcommand() {
        let cases: &[&[&str]] = &[
            &["agentd", "init"],
            &["agentd", "start"],
            &["agentd", "stop"],
            &["agentd", "status"],
            &["agentd", "log"],
            &["agentd", "log", "ITER-001"],
            &["agentd", "show", "ITER-001"],
            &["agentd", "config"],
            &["agentd", "refresh"],
            &["agentd", "assign", "ITER-001"],
            &["agentd", "cancel", "ITER-001"],
        ];
        for args in cases {
            let cli = Cli::try_parse_from(*args)
                .unwrap_or_else(|e| panic!("failed to parse {args:?}: {e}"));
            if !matches!(
                cli.command,
                Command::Init | Command::Start | Command::Stop | Command::Status
            ) {
                run(cli.command).await.unwrap();
            }
        }
    }
}
