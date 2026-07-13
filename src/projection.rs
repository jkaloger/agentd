use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use crate::store::ClaimRecord;

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

/// The point-in-time projection of the store (ADR-002): `.agentd/state.json` — the
/// whole claim set, rewritten atomically so a `jq` reader never sees a torn file —
/// plus `refs/claims/<iter-id>` pointers, one per live claim, git-style. Like the
/// log, this mirrors an already-committed store; every write is best-effort and a
/// failure is warned rather than surfaced, so it can never fail the caller.
pub struct Snapshot {
    dir: PathBuf,
}

impl Snapshot {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Rewrite `state.json` to reflect `claims`. The bytes land in a sibling temp
    /// file that is then `rename`d over the target: the rename is atomic, so a
    /// concurrent reader observes either the old file or the whole new one, never
    /// a partial write.
    pub fn write_state(&self, claims: &[(String, ClaimRecord)]) {
        if let Err(e) = self.write_state_inner(claims) {
            eprintln!(
                "agentd: warning: could not write {}: {e}",
                self.state_path().display()
            );
        }
    }

    /// Point `refs/claims/<iter_id>` at the current holder and fence.
    pub fn set_ref(&self, iter_id: &str, holder: &str, fence: u64) {
        if let Err(e) = self.set_ref_inner(iter_id, holder, fence) {
            eprintln!(
                "agentd: warning: could not write claim ref {}: {e}",
                self.ref_path(iter_id).display()
            );
        }
    }

    /// Drop `refs/claims/<iter_id>`. A missing ref is not an error.
    pub fn remove_ref(&self, iter_id: &str) {
        let path = self.ref_path(iter_id);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => eprintln!(
                "agentd: warning: could not remove claim ref {}: {e}",
                path.display()
            ),
        }
    }

    fn write_state_inner(&self, claims: &[(String, ClaimRecord)]) -> io::Result<()> {
        let mut entries = serde_json::Map::new();
        for (id, record) in claims {
            entries.insert(id.clone(), serde_json::to_value(record).map_err(io::Error::other)?);
        }
        let mut root = serde_json::Map::new();
        root.insert("claims".to_string(), serde_json::Value::Object(entries));
        let mut bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(root))?;
        bytes.push(b'\n');

        let tmp = self.dir.join("state.json.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(&bytes)?;
            file.flush()?;
        }
        fs::rename(&tmp, self.state_path())
    }

    fn set_ref_inner(&self, iter_id: &str, holder: &str, fence: u64) -> io::Result<()> {
        fs::create_dir_all(self.refs_dir())?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.ref_path(iter_id))?;
        file.write_all(format!("holder={holder} fence={fence}\n").as_bytes())?;
        file.flush()
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn refs_dir(&self) -> PathBuf {
        self.dir.join("refs").join("claims")
    }

    fn ref_path(&self, iter_id: &str) -> PathBuf {
        self.refs_dir().join(iter_id)
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

    fn claim(holder: &str, due_at: u64, fence: u64) -> ClaimRecord {
        ClaimRecord {
            holder: holder.to_string(),
            due_at,
            fence,
        }
    }

    #[test]
    fn state_json_is_valid_json_reflecting_the_claims() {
        let dir = TempDir::new().unwrap();
        let snapshot = Snapshot::new(dir.path());

        snapshot.write_state(&[
            ("ITER-001".to_string(), claim("agent-a", 5000, 1)),
            ("ITER-002".to_string(), claim("agent-b", 9000, 3)),
        ]);

        let contents = fs::read_to_string(dir.path().join("state.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(value["claims"]["ITER-001"]["holder"], "agent-a");
        assert_eq!(value["claims"]["ITER-001"]["fence"], 1);
        assert_eq!(value["claims"]["ITER-002"]["holder"], "agent-b");
        assert_eq!(value["claims"]["ITER-002"]["due_at"], 9000);
    }

    // Verification / AC1: temp+rename means a reader never sees a torn file. The
    // rename consumes the temp path, so no `state.json.tmp` survives and the final
    // file always parses as complete JSON — even when it replaces an existing one.
    #[test]
    fn state_json_is_written_via_temp_then_rename_leaving_no_partial_file() {
        let dir = TempDir::new().unwrap();
        let snapshot = Snapshot::new(dir.path());
        fs::write(dir.path().join("state.json"), b"{ not valid json").unwrap();

        snapshot.write_state(&[("ITER-001".to_string(), claim("agent-a", 5000, 1))]);

        assert!(
            !dir.path().join("state.json.tmp").exists(),
            "the temp file must be renamed away, not left behind"
        );
        let contents = fs::read_to_string(dir.path().join("state.json")).unwrap();
        serde_json::from_str::<serde_json::Value>(&contents)
            .expect("state.json is always complete, valid JSON after a rename");
    }

    #[test]
    fn a_ref_is_created_on_claim_and_removed_on_release() {
        let dir = TempDir::new().unwrap();
        let snapshot = Snapshot::new(dir.path());
        let ref_path = dir.path().join("refs").join("claims").join("ITER-001");

        snapshot.set_ref("ITER-001", "agent-a", 7);
        let contents = fs::read_to_string(&ref_path).unwrap();
        assert!(contents.contains("agent-a"), "ref carries the holder");
        assert!(contents.contains("7"), "ref carries the fence");

        snapshot.remove_ref("ITER-001");
        assert!(!ref_path.exists(), "release removes the ref");
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_write_failure_does_not_panic_or_error_the_caller() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).unwrap();

        let snapshot = Snapshot::new(dir.path());
        // Each call returns nothing: a panic or surfaced error would fail the test.
        snapshot.write_state(&[("ITER-001".to_string(), claim("agent-a", 5000, 1))]);
        snapshot.set_ref("ITER-001", "agent-a", 1);

        let mut restore = fs::metadata(dir.path()).unwrap().permissions();
        restore.set_mode(0o755);
        fs::set_permissions(dir.path(), restore).unwrap();

        assert!(!dir.path().join("state.json").exists());
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
