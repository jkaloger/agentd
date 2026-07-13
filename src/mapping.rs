use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::config::{self, Config, StateRole};

/// The resolved dispatch role mapping: which document types are dispatchable and
/// how each of their lifecycle states classifies into an orchestration role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMapping {
    types: BTreeSet<String>,
    states: BTreeMap<String, StateRole>,
}

impl RoleMapping {
    pub fn from_config(config: &Config) -> Self {
        Self::resolve(&config.dispatch.types, &config.dispatch.states)
    }

    /// The ADR-003 shipped default: type `iteration`; `accepted` dispatch,
    /// `in-progress` active, `complete`/`rejected`/`superseded` terminal.
    pub fn adr003_default() -> Self {
        RoleMapping {
            types: BTreeSet::from([config::DEFAULT_DISPATCH_TYPE.to_string()]),
            states: config::default_states(),
        }
    }

    fn resolve(types: &[String], states: &BTreeMap<String, StateRole>) -> Self {
        if types.is_empty() || states.is_empty() {
            return Self::adr003_default();
        }
        RoleMapping {
            types: types.iter().cloned().collect(),
            states: states.clone(),
        }
    }

    /// Classify a `(type, state)` into its orchestration role, or `None` when the
    /// type is not dispatchable or the state is unmapped. Consumed by candidate
    /// fetch (ITER-006).
    #[allow(dead_code)]
    pub fn classify(&self, doc_type: &str, state: &str) -> Option<StateRole> {
        if !self.types.contains(doc_type) {
            return None;
        }
        self.states.get(state).copied()
    }

    #[allow(dead_code)]
    pub fn dispatchable_types(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(String::as_str)
    }

    /// Reject any mapped state that a dispatchable type's lifecycle DAG lacks.
    pub fn validate(&self, dag: &dyn DagSource) -> Result<(), MappingError> {
        for doc_type in &self.types {
            let known: BTreeSet<String> = dag
                .states_for_type(doc_type)
                .map_err(MappingError::Dag)?
                .into_iter()
                .collect();
            for (state, role) in &self.states {
                if !known.contains(state) {
                    return Err(MappingError::UnknownState {
                        doc_type: doc_type.clone(),
                        state: state.clone(),
                        role: *role,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Seam over the lazyspec lifecycle DAG. ITER-006's `Tracker` will supply an
/// implementation; for now the only implementation shells out to the CLI.
pub trait DagSource {
    fn states_for_type(&self, doc_type: &str) -> Result<Vec<String>, DagError>;
}

/// Reads the lifecycle DAG by running `lazyspec config show --json`.
pub struct LazyspecCli {
    dir: PathBuf,
}

impl LazyspecCli {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        LazyspecCli { dir: dir.into() }
    }
}

impl DagSource for LazyspecCli {
    fn states_for_type(&self, doc_type: &str) -> Result<Vec<String>, DagError> {
        let output = Command::new("lazyspec")
            .args(["config", "show", "--json"])
            .current_dir(&self.dir)
            .output()
            .map_err(DagError::Spawn)?;
        if !output.status.success() {
            return Err(DagError::Command {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        states_from_config(&output.stdout, doc_type)
    }
}

/// Parse `lazyspec config show --json` stdout and return a document type's
/// lifecycle states. Shared by every seam that shells to the config command.
pub(crate) fn states_from_config(stdout: &[u8], doc_type: &str) -> Result<Vec<String>, DagError> {
    let parsed: CliConfig = serde_json::from_slice(stdout).map_err(DagError::Parse)?;
    parsed
        .types
        .into_iter()
        .find(|t| t.name == doc_type)
        .map(|t| t.lifecycle.states)
        .ok_or_else(|| DagError::UnknownType(doc_type.to_string()))
}

#[derive(Deserialize)]
struct CliConfig {
    types: Vec<CliType>,
}

#[derive(Deserialize)]
struct CliType {
    name: String,
    lifecycle: CliLifecycle,
}

#[derive(Deserialize)]
struct CliLifecycle {
    states: Vec<String>,
}

#[derive(Debug)]
pub enum DagError {
    Spawn(io::Error),
    Command { code: Option<i32>, stderr: String },
    Parse(serde_json::Error),
    UnknownType(String),
}

impl fmt::Display for DagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DagError::Spawn(e) => write!(f, "cannot run `lazyspec config show --json`: {e}"),
            DagError::Command { code, stderr } => match code {
                Some(c) => write!(f, "`lazyspec config show --json` exited {c}: {stderr}"),
                None => write!(f, "`lazyspec config show --json` terminated: {stderr}"),
            },
            DagError::Parse(e) => write!(f, "cannot parse lazyspec config JSON: {e}"),
            DagError::UnknownType(ty) => {
                write!(f, "lazyspec has no document type `{ty}`")
            }
        }
    }
}

impl std::error::Error for DagError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DagError::Spawn(e) => Some(e),
            DagError::Parse(e) => Some(e),
            DagError::Command { .. } | DagError::UnknownType(_) => None,
        }
    }
}

#[derive(Debug)]
pub enum MappingError {
    Dag(DagError),
    UnknownState {
        doc_type: String,
        state: String,
        role: StateRole,
    },
}

impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MappingError::Dag(e) => write!(f, "cannot read lifecycle DAG: {e}"),
            MappingError::UnknownState {
                doc_type,
                state,
                role,
            } => write!(
                f,
                "dispatch.states maps `{state}` -> {role:?} for type `{doc_type}`, \
                 but its lifecycle DAG has no `{state}` state"
            ),
        }
    }
}

impl std::error::Error for MappingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MappingError::Dag(e) => Some(e),
            MappingError::UnknownState { .. } => None,
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct StubDag {
    types: BTreeMap<String, Vec<String>>,
}

#[cfg(test)]
impl StubDag {
    pub fn with_type(mut self, doc_type: &str, states: &[&str]) -> Self {
        self.types.insert(
            doc_type.to_string(),
            states.iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn iteration() -> Self {
        StubDag::default().with_type(
            "iteration",
            &[
                "draft",
                "review",
                "accepted",
                "in-progress",
                "complete",
                "rejected",
                "superseded",
            ],
        )
    }
}

#[cfg(test)]
impl DagSource for StubDag {
    fn states_for_type(&self, doc_type: &str) -> Result<Vec<String>, DagError> {
        self.types
            .get(doc_type)
            .cloned()
            .ok_or_else(|| DagError::UnknownType(doc_type.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_str;

    #[test]
    fn explicit_mapping_classifies_states_to_configured_roles() {
        let config = load_str(
            r#"
[dispatch]
types = ["story"]

[dispatch.states]
accepted = "dispatch"
"in-progress" = "active"
complete = "terminal"
"#,
        )
        .unwrap();
        let mapping = RoleMapping::from_config(&config);

        assert_eq!(
            mapping.classify("story", "accepted"),
            Some(StateRole::Dispatch)
        );
        assert_eq!(
            mapping.classify("story", "in-progress"),
            Some(StateRole::Active)
        );
        assert_eq!(
            mapping.classify("story", "complete"),
            Some(StateRole::Terminal)
        );
        assert_eq!(mapping.classify("story", "draft"), None);
        assert_eq!(mapping.classify("iteration", "accepted"), None);
    }

    #[test]
    fn absent_mapping_resolves_to_the_adr003_default() {
        let mapping = RoleMapping::from_config(&load_str("").unwrap());

        assert_eq!(mapping, RoleMapping::adr003_default());
        assert_eq!(
            mapping.classify("iteration", "accepted"),
            Some(StateRole::Dispatch)
        );
        assert_eq!(
            mapping.classify("iteration", "in-progress"),
            Some(StateRole::Active)
        );
        assert_eq!(
            mapping.classify("iteration", "complete"),
            Some(StateRole::Terminal)
        );
        assert_eq!(
            mapping.classify("iteration", "rejected"),
            Some(StateRole::Terminal)
        );
        assert_eq!(
            mapping.classify("iteration", "superseded"),
            Some(StateRole::Terminal)
        );
        assert_eq!(
            mapping.dispatchable_types().collect::<Vec<_>>(),
            vec!["iteration"]
        );
    }

    #[test]
    fn default_mapping_validates_against_the_iteration_dag() {
        let mapping = RoleMapping::adr003_default();
        assert!(mapping.validate(&StubDag::iteration()).is_ok());
    }

    #[test]
    fn mapping_referencing_a_missing_state_names_the_offending_type_and_state() {
        let config = load_str(
            r#"
[dispatch]
types = ["iteration"]

[dispatch.states]
accepted = "dispatch"
shipped = "terminal"
"#,
        )
        .unwrap();
        let mapping = RoleMapping::from_config(&config);

        let err = mapping.validate(&StubDag::iteration()).unwrap_err();
        assert!(matches!(err, MappingError::UnknownState { .. }));
        let msg = err.to_string();
        assert!(msg.contains("iteration"), "{msg}");
        assert!(msg.contains("shipped"), "{msg}");
    }

    #[test]
    fn validation_surfaces_a_dag_lookup_failure() {
        let mapping = RoleMapping::adr003_default();
        let err = mapping
            .validate(&StubDag::default().with_type("bug", &["reported"]))
            .unwrap_err();
        assert!(
            matches!(err, MappingError::Dag(DagError::UnknownType(_))),
            "{err}"
        );
    }

    #[test]
    fn real_lazyspec_cli_dag_parses_when_available() {
        let dag = LazyspecCli::new(".");
        // Skip when lazyspec is unavailable in the environment.
        if let Ok(states) = dag.states_for_type("iteration") {
            assert!(states.iter().any(|s| s == "accepted"), "{states:?}");
            assert!(states.iter().any(|s| s == "in-progress"), "{states:?}");
        }
    }
}
