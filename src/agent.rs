use serde_json::Value;

/// The canonical, protocol-agnostic event model (ADR-004). Every adapter maps
/// its native protocol onto this enum so the orchestrator never sees a
/// Claude-, codex-, or opencode-specific shape. The generic message variants
/// carry only the wall-clock time and the agent process pid the runtime
/// supplies; nothing on them is tied to a particular agent's wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    SessionStarted {
        session_id: String,
    },
    /// The agent talking to us (Claude `assistant` messages).
    Notification {
        pid: u32,
        at_ms: u64,
        text: String,
    },
    /// Any other message in the turn (tool results, `user` echoes).
    OtherMessage {
        pid: u32,
        at_ms: u64,
        text: String,
    },
    TurnCompleted {
        pid: u32,
        at_ms: u64,
    },
    TurnFailed {
        pid: u32,
        at_ms: u64,
        reason: String,
    },
    TurnCancelled {
        pid: u32,
        at_ms: u64,
    },
    Malformed {
        raw: String,
        reason: String,
    },
}

/// Map one line of Claude `--output-format stream-json` onto a canonical event.
///
/// Pure and total: `pid` and `at_ms` are injected by the runtime so the mapper
/// never touches the clock, and every input — including invalid JSON or an
/// unrecognised shape — yields an `AgentEvent` rather than panicking, so a
/// caller folding a whole stream never aborts on a bad line.
pub fn map_line(line: &str, pid: u32, at_ms: u64) -> AgentEvent {
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(e) => return malformed(line, format!("invalid json: {e}")),
    };

    match value.get("type").and_then(Value::as_str) {
        Some("system") => map_system(&value, line),
        Some("assistant") => AgentEvent::Notification {
            pid,
            at_ms,
            text: message_text(&value),
        },
        Some("user") => AgentEvent::OtherMessage {
            pid,
            at_ms,
            text: message_text(&value),
        },
        Some("result") => map_result(&value, pid, at_ms),
        Some(other) => malformed(line, format!("unknown event type {other:?}")),
        None => malformed(line, "missing \"type\" field".to_string()),
    }
}

fn map_system(value: &Value, line: &str) -> AgentEvent {
    match value.get("session_id").and_then(Value::as_str) {
        Some(session_id) => AgentEvent::SessionStarted {
            session_id: session_id.to_string(),
        },
        None => malformed(line, "system line without session_id".to_string()),
    }
}

fn map_result(value: &Value, pid: u32, at_ms: u64) -> AgentEvent {
    let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    if is_cancel(subtype) {
        return AgentEvent::TurnCancelled { pid, at_ms };
    }
    if is_error || (!subtype.is_empty() && subtype != "success") {
        return AgentEvent::TurnFailed {
            pid,
            at_ms,
            reason: result_reason(value, subtype),
        };
    }
    AgentEvent::TurnCompleted { pid, at_ms }
}

fn is_cancel(subtype: &str) -> bool {
    subtype.contains("cancel") || subtype.contains("interrupt") || subtype.contains("abort")
}

fn result_reason(value: &Value, subtype: &str) -> String {
    if !subtype.is_empty() {
        return subtype.to_string();
    }
    value
        .get("error")
        .or_else(|| value.get("result"))
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .to_string()
}

/// Join the textual blocks of a `message.content` payload. Content may be a
/// bare string or an array of typed blocks; anything without extractable text
/// collapses to empty, keeping the generic variants free of wire specifics.
fn message_text(value: &Value) -> String {
    let content = match value.get("message").and_then(|m| m.get("content")) {
        Some(content) => content,
        None => return String::new(),
    };
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn malformed(line: &str, reason: String) -> AgentEvent {
    AgentEvent::Malformed {
        raw: line.to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PID: u32 = 4242;
    const AT_MS: u64 = 1_700_000_000_000;

    #[test]
    fn assistant_message_maps_to_notification_with_pid_and_timestamp() {
        let line = r#"{"type":"assistant","session_id":"s1","message":{"role":"assistant","content":[{"type":"text","text":"working on it"}]}}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::Notification {
                pid: PID,
                at_ms: AT_MS,
                text: "working on it".to_string(),
            }
        );
    }

    #[test]
    fn user_message_maps_to_other_message_with_pid_and_timestamp() {
        let line = r#"{"type":"user","session_id":"s1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#;

        let event = map_line(line, PID, AT_MS);

        assert!(matches!(
            event,
            AgentEvent::OtherMessage {
                pid: PID,
                at_ms: AT_MS,
                ..
            }
        ));
    }

    #[test]
    fn assistant_message_with_string_content_carries_the_text() {
        let line = r#"{"type":"assistant","message":{"content":"plain string body"}}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::Notification {
                pid: PID,
                at_ms: AT_MS,
                text: "plain string body".to_string(),
            }
        );
    }

    #[test]
    fn system_init_line_maps_to_session_started_with_the_session_id() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/tmp","session_id":"sess-abc-123","tools":[]}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::SessionStarted {
                session_id: "sess-abc-123".to_string(),
            }
        );
    }

    #[test]
    fn result_success_line_maps_to_turn_completed() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"session_id":"s1","result":"done"}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::TurnCompleted {
                pid: PID,
                at_ms: AT_MS,
            }
        );
    }

    #[test]
    fn result_failure_line_maps_to_turn_failed_with_a_reason() {
        let line =
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s1"}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::TurnFailed {
                pid: PID,
                at_ms: AT_MS,
                reason: "error_max_turns".to_string(),
            }
        );
    }

    #[test]
    fn result_is_error_without_subtype_still_fails() {
        let line = r#"{"type":"result","is_error":true,"error":"boom"}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::TurnFailed {
                pid: PID,
                at_ms: AT_MS,
                reason: "boom".to_string(),
            }
        );
    }

    #[test]
    fn result_cancel_line_maps_to_turn_cancelled() {
        let line =
            r#"{"type":"result","subtype":"error_cancelled","is_error":true,"session_id":"s1"}"#;

        let event = map_line(line, PID, AT_MS);

        assert_eq!(
            event,
            AgentEvent::TurnCancelled {
                pid: PID,
                at_ms: AT_MS,
            }
        );
    }

    #[test]
    fn invalid_json_maps_to_malformed_without_panicking() {
        let event = map_line("{not json", PID, AT_MS);

        match event {
            AgentEvent::Malformed { raw, reason } => {
                assert_eq!(raw, "{not json");
                assert!(reason.contains("invalid json"), "{reason}");
            }
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn unknown_shape_maps_to_malformed() {
        let unknown_type = map_line(r#"{"type":"telemetry","x":1}"#, PID, AT_MS);
        assert!(matches!(unknown_type, AgentEvent::Malformed { .. }));

        let no_type = map_line(r#"{"session_id":"s1"}"#, PID, AT_MS);
        assert!(matches!(no_type, AgentEvent::Malformed { .. }));

        let system_no_session = map_line(r#"{"type":"system","subtype":"init"}"#, PID, AT_MS);
        assert!(matches!(system_no_session, AgentEvent::Malformed { .. }));
    }

    #[test]
    fn a_malformed_line_does_not_stop_the_rest_of_the_stream() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#,
            "{ garbage not json",
            r#"{"type":"user","message":{"content":"tool output"}}"#,
            r#"{"type":"result","subtype":"success","is_error":false}"#,
        ];

        let events: Vec<AgentEvent> = lines.iter().map(|l| map_line(l, PID, AT_MS)).collect();

        assert_eq!(events.len(), lines.len());
        assert!(matches!(events[0], AgentEvent::SessionStarted { .. }));
        assert!(matches!(events[1], AgentEvent::Notification { .. }));
        assert!(matches!(events[2], AgentEvent::Malformed { .. }));
        assert!(matches!(events[3], AgentEvent::OtherMessage { .. }));
        assert!(matches!(events[4], AgentEvent::TurnCompleted { .. }));
    }
}
