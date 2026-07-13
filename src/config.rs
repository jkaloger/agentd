use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const DEFAULT_POLL_INTERVAL_MS: u64 = 5000;
const DEFAULT_MAX_CONCURRENT: u32 = 2;
const DEFAULT_MAX_TURNS: u32 = 1;
const DEFAULT_AUTO_APPROVE: bool = true;
const DEFAULT_WORKSPACE_ROOT: &str = ".agentd/workspaces";
const DEFAULT_PROMPT_TEMPLATE: &str = ".agentd/prompt.liquid";
pub(crate) const DEFAULT_DISPATCH_TYPE: &str = "iteration";
const DEFAULT_CLAIM: &str = "in-progress";
const DEFAULT_SUCCESS: &str = "complete";
const DEFAULT_FAILURE: &str = "rejected";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceMode {
    Worktree,
    PlainDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateRole {
    Dispatch,
    Active,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub poll_interval_ms: u64,
    pub max_concurrent: u32,
    pub agent: Agent,
    pub workspace: Workspace,
    pub dispatch: Dispatch,
    pub transitions: Transitions,
    pub per_status_caps: BTreeMap<String, u32>,
    pub prompt: Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub kind: AgentKind,
    pub max_turns: u32,
    pub auto_approve: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub mode: WorkspaceMode,
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub types: Vec<String>,
    pub states: BTreeMap<String, StateRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transitions {
    pub claim: String,
    pub success: String,
    pub failure: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub template: String,
}

#[derive(Debug)]
pub enum ConfigError {
    Read { path: PathBuf, source: io::Error },
    Parse(toml::de::Error),
    Invalid { key: String, reason: String },
}

impl ConfigError {
    fn invalid(key: &str, reason: &str) -> Self {
        ConfigError::Invalid {
            key: key.to_string(),
            reason: reason.to_string(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Read { path, source } => {
                write!(f, "cannot read config {}: {source}", path.display())
            }
            ConfigError::Parse(e) => write!(f, "invalid config syntax: {e}"),
            ConfigError::Invalid { key, reason } => {
                write!(f, "invalid config: `{key}` {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Read { source, .. } => Some(source),
            ConfigError::Parse(e) => Some(e),
            ConfigError::Invalid { .. } => None,
        }
    }
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    load_str(&contents)
}

pub fn load_str(contents: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(contents).map_err(ConfigError::Parse)?;
    resolve(raw)
}

fn resolve(raw: RawConfig) -> Result<Config, ConfigError> {
    let poll_interval_ms = raw.poll_interval_ms.unwrap_or(DEFAULT_POLL_INTERVAL_MS);
    require_positive("poll_interval_ms", poll_interval_ms)?;

    let max_concurrent = raw.max_concurrent.unwrap_or(DEFAULT_MAX_CONCURRENT);
    require_positive("max_concurrent", max_concurrent as u64)?;

    let max_turns = raw.agent.max_turns.unwrap_or(DEFAULT_MAX_TURNS);
    require_positive("agent.max_turns", max_turns as u64)?;
    let agent = Agent {
        kind: raw.agent.kind.unwrap_or(AgentKind::Claude),
        max_turns,
        auto_approve: raw.agent.auto_approve.unwrap_or(DEFAULT_AUTO_APPROVE),
    };

    let root = raw
        .workspace
        .root
        .unwrap_or_else(|| DEFAULT_WORKSPACE_ROOT.to_string());
    require_non_empty("workspace.root", &root)?;
    let workspace = Workspace {
        mode: raw.workspace.mode.unwrap_or(WorkspaceMode::Worktree),
        root,
    };

    let types = raw
        .dispatch
        .types
        .unwrap_or_else(|| vec![DEFAULT_DISPATCH_TYPE.to_string()]);
    if types.is_empty() {
        return Err(ConfigError::invalid(
            "dispatch.types",
            "must list at least one document type",
        ));
    }
    for entry in &types {
        require_non_empty("dispatch.types", entry)?;
    }
    let states = raw.dispatch.states.unwrap_or_else(default_states);
    let dispatch = Dispatch { types, states };

    let claim = raw
        .transitions
        .claim
        .unwrap_or_else(|| DEFAULT_CLAIM.to_string());
    require_non_empty("transitions.claim", &claim)?;
    let success = raw
        .transitions
        .success
        .unwrap_or_else(|| DEFAULT_SUCCESS.to_string());
    require_non_empty("transitions.success", &success)?;
    let failure = raw
        .transitions
        .failure
        .unwrap_or_else(|| DEFAULT_FAILURE.to_string());
    require_non_empty("transitions.failure", &failure)?;
    let transitions = Transitions {
        claim,
        success,
        failure,
    };

    let per_status_caps = resolve_caps(raw.concurrency.per_status);

    let template = raw
        .prompt
        .template
        .unwrap_or_else(|| DEFAULT_PROMPT_TEMPLATE.to_string());
    require_non_empty("prompt.template", &template)?;
    let prompt = Prompt { template };

    Ok(Config {
        poll_interval_ms,
        max_concurrent,
        agent,
        workspace,
        dispatch,
        transitions,
        per_status_caps,
        prompt,
    })
}

fn require_positive(key: &str, value: u64) -> Result<(), ConfigError> {
    if value == 0 {
        Err(ConfigError::invalid(key, "must be >= 1"))
    } else {
        Ok(())
    }
}

fn require_non_empty(key: &str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        Err(ConfigError::invalid(key, "must not be empty"))
    } else {
        Ok(())
    }
}

fn resolve_caps(raw: BTreeMap<String, toml::Value>) -> BTreeMap<String, u32> {
    raw.into_iter()
        .filter_map(|(key, value)| coerce_cap(&value).map(|cap| (key, cap)))
        .collect()
}

fn coerce_cap(value: &toml::Value) -> Option<u32> {
    let n = value.as_integer()?;
    if n < 1 {
        return None;
    }
    u32::try_from(n).ok()
}

pub(crate) fn default_states() -> BTreeMap<String, StateRole> {
    BTreeMap::from([
        ("accepted".to_string(), StateRole::Dispatch),
        ("in-progress".to_string(), StateRole::Active),
        ("complete".to_string(), StateRole::Terminal),
        ("rejected".to_string(), StateRole::Terminal),
        ("superseded".to_string(), StateRole::Terminal),
    ])
}

#[derive(Deserialize)]
struct RawConfig {
    poll_interval_ms: Option<u64>,
    max_concurrent: Option<u32>,
    #[serde(default)]
    agent: RawAgent,
    #[serde(default)]
    workspace: RawWorkspace,
    #[serde(default)]
    dispatch: RawDispatch,
    #[serde(default)]
    transitions: RawTransitions,
    #[serde(default)]
    concurrency: RawConcurrency,
    #[serde(default)]
    prompt: RawPrompt,
}

#[derive(Default, Deserialize)]
struct RawAgent {
    kind: Option<AgentKind>,
    max_turns: Option<u32>,
    auto_approve: Option<bool>,
}

#[derive(Default, Deserialize)]
struct RawWorkspace {
    mode: Option<WorkspaceMode>,
    root: Option<String>,
}

#[derive(Default, Deserialize)]
struct RawDispatch {
    types: Option<Vec<String>>,
    states: Option<BTreeMap<String, StateRole>>,
}

#[derive(Default, Deserialize)]
struct RawTransitions {
    claim: Option<String>,
    success: Option<String>,
    failure: Option<String>,
}

#[derive(Default, Deserialize)]
struct RawConcurrency {
    #[serde(default)]
    per_status: BTreeMap<String, toml::Value>,
}

#[derive(Default, Deserialize)]
struct RawPrompt {
    template: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMBEDDED_TEMPLATE: &str = include_str!("templates/config.toml");

    #[test]
    fn minimal_config_takes_documented_defaults() {
        let config = load_str("").unwrap();

        assert_eq!(config.poll_interval_ms, 5000);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.agent.kind, AgentKind::Claude);
        assert_eq!(config.agent.max_turns, 1);
        assert!(config.agent.auto_approve);
        assert_eq!(config.workspace.mode, WorkspaceMode::Worktree);
        assert_eq!(config.workspace.root, ".agentd/workspaces");
        assert_eq!(config.dispatch.types, vec!["iteration".to_string()]);
        assert_eq!(
            config.dispatch.states.get("in-progress"),
            Some(&StateRole::Active)
        );
        assert_eq!(config.transitions.claim, "in-progress");
        assert_eq!(config.transitions.success, "complete");
        assert_eq!(config.transitions.failure, "rejected");
        assert!(config.per_status_caps.is_empty());
        assert_eq!(config.prompt.template, ".agentd/prompt.liquid");
    }

    #[test]
    fn partial_override_keeps_defaults_for_the_rest() {
        let config = load_str(
            r#"
poll_interval_ms = 1000

[agent]
kind = "codex"
"#,
        )
        .unwrap();

        assert_eq!(config.poll_interval_ms, 1000);
        assert_eq!(config.agent.kind, AgentKind::Codex);
        assert_eq!(config.agent.max_turns, 1);
        assert_eq!(config.max_concurrent, 2);
    }

    #[test]
    fn out_of_range_poll_interval_names_the_key() {
        let err = load_str("poll_interval_ms = 0").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
        assert!(err.to_string().contains("poll_interval_ms"), "{err}");
    }

    #[test]
    fn out_of_range_max_turns_names_the_key() {
        let err = load_str("[agent]\nmax_turns = 0").unwrap_err();
        assert!(err.to_string().contains("agent.max_turns"), "{err}");
    }

    #[test]
    fn empty_required_prompt_template_names_the_key() {
        let err = load_str("[prompt]\ntemplate = \"\"").unwrap_err();
        assert!(err.to_string().contains("prompt.template"), "{err}");
    }

    #[test]
    fn empty_dispatch_types_names_the_key() {
        let err = load_str("[dispatch]\ntypes = []").unwrap_err();
        assert!(err.to_string().contains("dispatch.types"), "{err}");
    }

    #[test]
    fn unknown_top_level_keys_are_ignored() {
        let config = load_str(
            r#"
future_scalar = "ignored"

[future_table]
whatever = 1
"#,
        )
        .unwrap();

        assert_eq!(config.poll_interval_ms, 5000);
        assert_eq!(config.agent.max_turns, 1);
    }

    #[test]
    fn bad_per_status_cap_entry_is_dropped_and_the_rest_applied() {
        let config = load_str(
            r#"
[concurrency.per_status]
"in-progress" = 3
"review" = 0
"broken" = "not-a-number"
"#,
        )
        .unwrap();

        assert_eq!(config.per_status_caps.get("in-progress"), Some(&3));
        assert!(!config.per_status_caps.contains_key("review"));
        assert!(!config.per_status_caps.contains_key("broken"));
        assert_eq!(config.per_status_caps.len(), 1);
    }

    #[test]
    fn embedded_template_parses_and_validates_cleanly() {
        let config = load_str(EMBEDDED_TEMPLATE).unwrap();

        assert_eq!(config.poll_interval_ms, 5000);
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.agent.kind, AgentKind::Claude);
        assert_eq!(config.agent.max_turns, 1);
        assert_eq!(config.workspace.mode, WorkspaceMode::Worktree);
        assert_eq!(config.dispatch.types, vec!["iteration".to_string()]);
        assert_eq!(
            config.dispatch.states.get("superseded"),
            Some(&StateRole::Terminal)
        );
        assert!(config.per_status_caps.is_empty());
    }
}
