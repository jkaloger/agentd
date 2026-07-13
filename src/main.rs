use clap::{Parser, Subcommand};

mod init;

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

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Init => run_init(),
        other => {
            println!("unimplemented: {}", other.name());
            Ok(())
        }
    }
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

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    run(cli.command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_every_porcelain_subcommand() {
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
            if !matches!(cli.command, Command::Init) {
                run(cli.command).unwrap();
            }
        }
    }
}
