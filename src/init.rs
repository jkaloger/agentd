use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const STORE_DIR: &str = ".agentd";

const CONFIG_TEMPLATE: &str = include_str!("templates/config.toml");
const PROMPT_TEMPLATE: &str = include_str!("templates/prompt.liquid");

pub struct InitReport {
    pub store: PathBuf,
    pub created: Vec<PathBuf>,
    pub existing: Vec<PathBuf>,
}

impl InitReport {
    pub fn already_initialized(&self) -> bool {
        self.created.is_empty()
    }
}

pub fn init(root: &Path) -> io::Result<InitReport> {
    let store = root.join(STORE_DIR);
    fs::create_dir_all(&store)?;

    let files = [
        (store.join("config.toml"), CONFIG_TEMPLATE),
        (store.join("prompt.liquid"), PROMPT_TEMPLATE),
    ];

    let mut created = Vec::new();
    let mut existing = Vec::new();
    for (path, contents) in files {
        if path.exists() {
            existing.push(path);
        } else {
            fs::write(&path, contents)?;
            created.push(path);
        }
    }

    Ok(InitReport {
        store,
        created,
        existing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fresh_init_creates_documented_files() {
        let root = TempDir::new().unwrap();
        let report = init(root.path()).unwrap();

        assert!(!report.already_initialized());
        assert_eq!(report.created.len(), 2);
        assert!(report.existing.is_empty());

        let config = root.path().join(STORE_DIR).join("config.toml");
        let prompt = root.path().join(STORE_DIR).join("prompt.liquid");
        assert!(config.is_file());
        assert!(prompt.is_file());

        let config_body = fs::read_to_string(&config).unwrap();
        assert!(config_body.contains("poll_interval_ms"));
        assert!(config_body.starts_with("# agentd configuration"));
    }

    #[test]
    fn rerun_does_not_clobber_existing_files() {
        let root = TempDir::new().unwrap();
        init(root.path()).unwrap();

        let config = root.path().join(STORE_DIR).join("config.toml");
        let prompt = root.path().join(STORE_DIR).join("prompt.liquid");
        let sentinel = "# edited by operator\n";
        fs::write(&config, sentinel).unwrap();
        let prompt_before = fs::read_to_string(&prompt).unwrap();

        let report = init(root.path()).unwrap();

        assert!(report.already_initialized());
        assert!(report.created.is_empty());
        assert_eq!(report.existing.len(), 2);
        assert_eq!(fs::read_to_string(&config).unwrap(), sentinel);
        assert_eq!(fs::read_to_string(&prompt).unwrap(), prompt_before);
    }

    #[cfg(unix)]
    #[test]
    fn non_writable_target_dir_errors() {
        use std::os::unix::fs::PermissionsExt;

        let root = TempDir::new().unwrap();
        let mut perms = fs::metadata(root.path()).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(root.path(), perms).unwrap();

        let result = init(root.path());

        let mut restore = fs::metadata(root.path()).unwrap().permissions();
        restore.set_mode(0o755);
        fs::set_permissions(root.path(), restore).unwrap();

        assert!(result.is_err());
    }
}
