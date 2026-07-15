use std::fmt;

use crate::tracker::{Candidate, DocView, Tracker, TrackerError};

/// The default per-ticket template (ADR-006): thin, injecting the rendered
/// iteration plus its immediate parent context and saying "execute it".
pub const DEFAULT_TEMPLATE: &str = include_str!("templates/prompt.liquid");

/// Assemble the agent prompt for `iteration`: fetch its immediate parent
/// story/bug via the tracker, then STRICT-render `template_src` with the
/// iteration body, parent context, and `attempt`.
///
/// Any fault — a missing parent link, a parent fetch failure, a template parse
/// error, or an unknown variable/filter at render time — returns a typed error
/// and never a half-rendered prompt (STORY-028 AC4).
pub fn assemble_prompt<T: Tracker>(
    template_src: &str,
    iteration: &Candidate,
    attempt: u32,
    tracker: &T,
) -> Result<String, PromptError> {
    let parent_id = iteration
        .parent
        .as_deref()
        .ok_or_else(|| PromptError::MissingParent {
            iteration: iteration.id.clone(),
        })?;
    let parent = tracker
        .fetch_doc(parent_id)
        .map_err(PromptError::ParentFetch)?;
    render(template_src, iteration, &parent, attempt)
}

fn render(
    template_src: &str,
    iteration: &Candidate,
    parent: &DocView,
    attempt: u32,
) -> Result<String, PromptError> {
    let parser = liquid::ParserBuilder::with_stdlib()
        .build()
        .map_err(PromptError::Parse)?;
    let template = parser.parse(template_src).map_err(PromptError::Parse)?;

    let globals = liquid::object!({
        "iteration": {
            "id": iteration.id,
            "title": iteration.title,
            "body": iteration.body,
        },
        "parent": {
            "type": parent.doc_type,
            "id": parent.id,
            "title": parent.title,
            "body": parent.body,
        },
        "attempt": (attempt),
    });

    template.render(&globals).map_err(PromptError::Render)
}

#[derive(Debug)]
pub enum PromptError {
    MissingParent { iteration: String },
    ParentFetch(TrackerError),
    Parse(liquid::Error),
    Render(liquid::Error),
}

impl fmt::Display for PromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PromptError::MissingParent { iteration } => {
                write!(
                    f,
                    "iteration `{iteration}` has no parent work item to render"
                )
            }
            PromptError::ParentFetch(e) => write!(f, "cannot fetch parent context: {e}"),
            PromptError::Parse(e) => write!(f, "cannot parse prompt template: {e}"),
            PromptError::Render(e) => write!(f, "cannot render prompt template: {e}"),
        }
    }
}

impl std::error::Error for PromptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PromptError::ParentFetch(e) => Some(e),
            PromptError::Parse(e) | PromptError::Render(e) => Some(e),
            PromptError::MissingParent { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::{CliFailure, DocLookup};

    struct FakeTracker {
        parent: Result<DocView, ()>,
    }

    impl FakeTracker {
        fn with_parent(parent: DocView) -> Self {
            FakeTracker { parent: Ok(parent) }
        }

        fn failing() -> Self {
            FakeTracker { parent: Err(()) }
        }
    }

    impl Tracker for FakeTracker {
        fn fetch_dispatchable(&self) -> Result<Vec<Candidate>, TrackerError> {
            unimplemented!("not exercised by prompt tests")
        }

        fn fetch_doc(&self, _id: &str) -> Result<DocView, TrackerError> {
            self.parent.clone().map_err(|()| {
                TrackerError::from(CliFailure::Exit {
                    code: Some(1),
                    stderr: "no such document".to_string(),
                })
            })
        }

        fn lookup_doc(&self, _id: &str) -> Result<DocLookup, TrackerError> {
            unimplemented!("not exercised by prompt tests")
        }

        fn advance(&self, _id: &str, _target_state: &str) -> Result<(), TrackerError> {
            unimplemented!("not exercised by prompt tests")
        }
    }

    fn iteration() -> Candidate {
        Candidate {
            id: "ITERATION-010".to_string(),
            identifier: "assemble-prompt".to_string(),
            title: "Assemble prompt from iteration plus parent context".to_string(),
            body: "Objective: render the iteration body and parent context.".to_string(),
            state: "accepted".to_string(),
            parent: Some("STORY-028".to_string()),
            dependencies: Vec::new(),
            priority: None,
            created_at: "2026-07-13".to_string(),
        }
    }

    fn parent() -> DocView {
        DocView {
            id: "STORY-028".to_string(),
            doc_type: "story".to_string(),
            title: "Assemble prompt from iteration plus parent context".to_string(),
            body: "As an operator, I want the plan with its parent intent.".to_string(),
            status: "accepted".to_string(),
        }
    }

    #[test]
    fn default_template_includes_iteration_and_parent_context() {
        let tracker = FakeTracker::with_parent(parent());

        let prompt = assemble_prompt(DEFAULT_TEMPLATE, &iteration(), 1, &tracker).unwrap();

        assert!(prompt.contains("Assemble prompt from iteration plus parent context"));
        assert!(prompt.contains("(ITERATION-010)"));
        assert!(prompt.contains("Objective: render the iteration body and parent context."));
        assert!(prompt.contains("Parent story:"));
        assert!(prompt.contains("(STORY-028)"));
        assert!(prompt.contains("As an operator, I want the plan with its parent intent."));
    }

    #[test]
    fn default_template_surfaces_a_retry_note_from_the_attempt_value() {
        let tracker = FakeTracker::with_parent(parent());

        let first = assemble_prompt(DEFAULT_TEMPLATE, &iteration(), 1, &tracker).unwrap();
        assert!(!first.contains("Retry (attempt"), "{first}");

        let retry = assemble_prompt(DEFAULT_TEMPLATE, &iteration(), 2, &tracker).unwrap();
        assert!(retry.contains("Retry (attempt 2)"), "{retry}");
    }

    #[test]
    fn attempt_is_bound_as_an_integer_available_to_the_template() {
        let tracker = FakeTracker::with_parent(parent());

        let prompt =
            assemble_prompt("attempt is {{ attempt }}", &iteration(), 3, &tracker).unwrap();

        assert_eq!(prompt, "attempt is 3");
    }

    #[test]
    fn an_unknown_variable_fails_with_a_render_error_not_a_blank() {
        let tracker = FakeTracker::with_parent(parent());

        let err =
            assemble_prompt("before {{ nope }} after", &iteration(), 1, &tracker).unwrap_err();

        assert!(matches!(err, PromptError::Render(_)), "{err}");
    }

    #[test]
    fn an_unknown_filter_fails_with_a_typed_error() {
        let tracker = FakeTracker::with_parent(parent());

        let err = assemble_prompt("{{ iteration.title | bogus }}", &iteration(), 1, &tracker)
            .unwrap_err();

        assert!(
            matches!(err, PromptError::Parse(_) | PromptError::Render(_)),
            "{err}"
        );
    }

    #[test]
    fn a_parent_fetch_failure_yields_a_typed_error_and_no_prompt() {
        let tracker = FakeTracker::failing();

        let err = assemble_prompt(DEFAULT_TEMPLATE, &iteration(), 1, &tracker).unwrap_err();

        assert!(matches!(err, PromptError::ParentFetch(_)), "{err}");
    }

    #[test]
    fn a_missing_parent_link_yields_a_typed_error() {
        let tracker = FakeTracker::with_parent(parent());
        let mut orphan = iteration();
        orphan.parent = None;

        let err = assemble_prompt(DEFAULT_TEMPLATE, &orphan, 1, &tracker).unwrap_err();

        assert!(matches!(err, PromptError::MissingParent { .. }), "{err}");
    }
}
