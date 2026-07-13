use std::path::Path;
use std::time::Duration;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

const CLAIMS: TableDefinition<&str, &[u8]> = TableDefinition::new("claims");
const RETRIES: TableDefinition<&str, &[u8]> = TableDefinition::new("retries");

/// A durable claim on an iteration: who holds it, when the lease expires, and
/// a fence that a heartbeat must present to prove it still owns that lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub holder: String,
    pub due_at: u64,
    pub fence: u64,
}

/// A durable retry schedule for a failed iteration: which attempt this is, the
/// error that triggered it, and when it next becomes eligible.
///
/// `due_at` is absolute wall-clock ms (Unix epoch), never a monotonic instant:
/// it must outlive the process, so after a restart eligibility is re-derived by
/// comparing the persisted `due_at` against the wall clock — no timer handle is
/// stored (ADR-002).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryRecord {
    pub attempt: u32,
    pub error: String,
    pub due_at: u64,
}

#[derive(Debug)]
pub enum ClaimError {
    AlreadyClaimed(ClaimRecord),
    Storage(redb::Error),
    Encode(serde_json::Error),
}

#[derive(Debug)]
pub enum StoreError {
    Open(redb::Error),
    Storage(redb::Error),
    Encode(serde_json::Error),
    Decode(serde_json::Error),
}

#[derive(Debug)]
pub enum HeartbeatError {
    /// No claim exists at all for the given id.
    NotFound,
    /// A claim exists but its holder or fence does not match the caller's.
    Mismatch(ClaimRecord),
    Storage(redb::Error),
    Encode(serde_json::Error),
    Decode(serde_json::Error),
}

/// The agentd durable store: a daemon-owned redb database.
pub struct Store {
    db: Database,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let db = Database::create(path).map_err(|e| StoreError::Open(e.into()))?;
        let txn = db.begin_write().map_err(storage)?;
        txn.open_table(CLAIMS).map_err(storage)?;
        txn.open_table(RETRIES).map_err(storage)?;
        txn.commit().map_err(storage)?;
        Ok(Self { db })
    }

    /// Claim `id` for `holder` iff it is unclaimed or its lease has expired.
    ///
    /// The read-check-write runs inside one redb write transaction, which redb
    /// serializes against all other writers — so racing callers cannot both
    /// observe the id as free.
    pub fn claim(
        &self,
        id: &str,
        holder: &str,
        now: u64,
        lease_ttl: Duration,
    ) -> Result<ClaimRecord, ClaimError> {
        let txn = self.db.begin_write().map_err(claim_storage)?;
        let record = {
            let mut table = txn.open_table(CLAIMS).map_err(claim_storage)?;

            let mut fence = 1;
            if let Some(existing) = table.get(id).map_err(claim_storage)? {
                let existing: ClaimRecord =
                    serde_json::from_slice(existing.value()).map_err(ClaimError::Encode)?;
                if existing.due_at > now {
                    return Err(ClaimError::AlreadyClaimed(existing));
                }
                fence = existing.fence + 1;
            }

            let record = ClaimRecord {
                holder: holder.to_string(),
                due_at: now + lease_ttl.as_millis() as u64,
                fence,
            };
            let bytes = serde_json::to_vec(&record).map_err(ClaimError::Encode)?;
            table.insert(id, bytes.as_slice()).map_err(claim_storage)?;
            record
        };
        txn.commit().map_err(claim_storage)?;
        Ok(record)
    }

    /// Extend a live claim's lease. Rejects (without writing) unless `holder`
    /// and `fence` both match the stored record — a stale or impersonating
    /// caller cannot renew a lease it does not hold.
    pub fn heartbeat(
        &self,
        id: &str,
        holder: &str,
        fence: u64,
        now: u64,
        lease_ttl: Duration,
    ) -> Result<ClaimRecord, HeartbeatError> {
        let txn = self.db.begin_write().map_err(heartbeat_storage)?;
        let record = {
            let mut table = txn.open_table(CLAIMS).map_err(heartbeat_storage)?;

            let existing: ClaimRecord = {
                let value = table
                    .get(id)
                    .map_err(heartbeat_storage)?
                    .ok_or(HeartbeatError::NotFound)?;
                serde_json::from_slice(value.value()).map_err(HeartbeatError::Decode)?
            };
            if existing.holder != holder || existing.fence != fence {
                return Err(HeartbeatError::Mismatch(existing));
            }

            let record = ClaimRecord {
                holder: existing.holder,
                due_at: now + lease_ttl.as_millis() as u64,
                fence: existing.fence,
            };
            let bytes = serde_json::to_vec(&record).map_err(HeartbeatError::Encode)?;
            table
                .insert(id, bytes.as_slice())
                .map_err(heartbeat_storage)?;
            record
        };
        txn.commit().map_err(heartbeat_storage)?;
        Ok(record)
    }

    /// Every persisted claim as `(id, record)`. Used by `reconcile` on restart to
    /// find claims orphaned by a dead daemon.
    pub fn claims(&self) -> Result<Vec<(String, ClaimRecord)>, StoreError> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(CLAIMS).map_err(storage)?;
        let mut claims = Vec::new();
        for entry in table.iter().map_err(storage)? {
            let (key, value) = entry.map_err(storage)?;
            let record = serde_json::from_slice(value.value()).map_err(StoreError::Decode)?;
            claims.push((key.value().to_string(), record));
        }
        Ok(claims)
    }

    pub fn get(&self, id: &str) -> Result<Option<ClaimRecord>, StoreError> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(CLAIMS).map_err(storage)?;
        match table.get(id).map_err(storage)? {
            Some(value) => {
                let record = serde_json::from_slice(value.value()).map_err(StoreError::Decode)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// Drop any claim on `id`, freeing it for a later attempt. Idempotent: a
    /// missing claim is not an error.
    pub fn release(&self, id: &str) -> Result<(), StoreError> {
        let txn = self.db.begin_write().map_err(storage)?;
        {
            let mut table = txn.open_table(CLAIMS).map_err(storage)?;
            table.remove(id).map_err(storage)?;
        }
        txn.commit().map_err(storage)?;
        Ok(())
    }

    /// Persist (or replace) the retry schedule for `id`. `due_at` is absolute
    /// wall-clock ms; the store just records what the caller computed. Upserts,
    /// so a later failure for the same id overwrites the prior schedule rather
    /// than accumulating stale entries.
    pub fn schedule_retry(
        &self,
        id: &str,
        attempt: u32,
        error: &str,
        due_at: u64,
    ) -> Result<RetryRecord, StoreError> {
        let record = RetryRecord {
            attempt,
            error: error.to_string(),
            due_at,
        };
        let bytes = serde_json::to_vec(&record).map_err(StoreError::Encode)?;
        let txn = self.db.begin_write().map_err(storage)?;
        {
            let mut table = txn.open_table(RETRIES).map_err(storage)?;
            table.insert(id, bytes.as_slice()).map_err(storage)?;
        }
        txn.commit().map_err(storage)?;
        Ok(record)
    }

    /// Every persisted retry schedule as `(id, record)`. Used on restart to
    /// re-derive pending retries from their stored `due_at`.
    pub fn retries(&self) -> Result<Vec<(String, RetryRecord)>, StoreError> {
        let txn = self.db.begin_read().map_err(storage)?;
        let table = txn.open_table(RETRIES).map_err(storage)?;
        let mut retries = Vec::new();
        for entry in table.iter().map_err(storage)? {
            let (key, value) = entry.map_err(storage)?;
            let record = serde_json::from_slice(value.value()).map_err(StoreError::Decode)?;
            retries.push((key.value().to_string(), record));
        }
        Ok(retries)
    }

    /// Retry schedules eligible by `now` (absolute wall-clock ms). Eligibility
    /// is derived purely from the stored `due_at`, so a schedule whose `due_at`
    /// is already past when the store is reopened is returned immediately —
    /// backoff survives a crash without any serialized timer.
    pub fn due_retries(&self, now: u64) -> Result<Vec<(String, RetryRecord)>, StoreError> {
        Ok(self
            .retries()?
            .into_iter()
            .filter(|(_, record)| record.due_at <= now)
            .collect())
    }

    /// Drop the retry schedule for `id` once it is re-dispatched or leaves
    /// candidacy. Idempotent: a missing entry is not an error.
    pub fn clear_retry(&self, id: &str) -> Result<(), StoreError> {
        let txn = self.db.begin_write().map_err(storage)?;
        {
            let mut table = txn.open_table(RETRIES).map_err(storage)?;
            table.remove(id).map_err(storage)?;
        }
        txn.commit().map_err(storage)?;
        Ok(())
    }
}

fn storage(e: impl Into<redb::Error>) -> StoreError {
    StoreError::Storage(e.into())
}

fn claim_storage(e: impl Into<redb::Error>) -> ClaimError {
    ClaimError::Storage(e.into())
}

fn heartbeat_storage(e: impl Into<redb::Error>) -> HeartbeatError {
    HeartbeatError::Storage(e.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    const TTL: Duration = Duration::from_secs(60);

    fn open(dir: &TempDir) -> Store {
        Store::open(&dir.path().join("claims.redb")).unwrap()
    }

    #[test]
    fn fresh_claim_writes_one_row_and_returns_it() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let record = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        assert_eq!(record.holder, "agent-a");
        assert_eq!(record.due_at, 1000 + 60_000);

        assert_eq!(store.get("ITER-007").unwrap(), Some(record));
    }

    #[test]
    fn concurrent_claims_for_same_id_yield_exactly_one_winner() {
        let dir = TempDir::new().unwrap();
        let store = Arc::new(open(&dir));
        let n = 16;

        let barrier = Arc::new(std::sync::Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|i| {
                let store = store.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.claim("ITER-007", &format!("agent-{i}"), 1000, TTL)
                })
            })
            .collect();

        let mut winners = 0;
        for handle in handles {
            match handle.join().unwrap() {
                Ok(_) => winners += 1,
                Err(ClaimError::AlreadyClaimed(_)) => {}
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
        assert_eq!(winners, 1);
    }

    #[test]
    fn live_lease_blocks_a_second_holder() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let first = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let err = store
            .claim("ITER-007", "agent-b", 2000, TTL)
            .expect_err("live lease must not be overwritten");
        match err {
            ClaimError::AlreadyClaimed(held) => assert_eq!(held, first),
            other => panic!("expected AlreadyClaimed, got {other:?}"),
        }
        assert_eq!(store.get("ITER-007").unwrap().unwrap().holder, "agent-a");
    }

    #[test]
    fn expired_lease_can_be_reclaimed_by_a_new_holder() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let first = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let now = first.due_at + 1;
        let second = store.claim("ITER-007", "agent-b", now, TTL).unwrap();

        assert_eq!(second.holder, "agent-b");
        assert_eq!(store.get("ITER-007").unwrap(), Some(second));
    }

    #[test]
    fn reclaim_of_an_expired_lease_bumps_the_fence() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let first = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let now = first.due_at + 1;
        let second = store.claim("ITER-007", "agent-b", now, TTL).unwrap();

        assert_eq!(second.holder, "agent-b");
        assert_eq!(second.fence, first.fence + 1);
        assert_eq!(second.due_at, now + 60_000);
        assert_eq!(store.get("ITER-007").unwrap(), Some(second));
    }

    #[test]
    fn a_heartbeat_from_the_prior_holders_fence_is_rejected_after_reclaim() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let first = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let now = first.due_at + 1;
        store.claim("ITER-007", "agent-b", now, TTL).unwrap();

        let err = store
            .heartbeat("ITER-007", "agent-a", first.fence, now + 1, TTL)
            .expect_err("a heartbeat carrying the prior holder's stale fence must be rejected");
        match err {
            HeartbeatError::Mismatch(_) => {}
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[test]
    fn fence_strictly_increases_across_successive_reclaims_of_the_same_id() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let mut now = 1000;
        let mut last = 0;
        for i in 0..5 {
            let record = store.claim("ITER-007", &format!("agent-{i}"), now, TTL).unwrap();
            assert!(record.fence > last, "fence must strictly increase");
            last = record.fence;
            now = record.due_at + 1;
        }
    }

    #[test]
    fn release_drops_the_claim_and_frees_it_for_a_live_reclaim() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        store.release("ITER-007").unwrap();
        assert_eq!(store.get("ITER-007").unwrap(), None);

        let reclaimed = store.claim("ITER-007", "agent-b", 2000, TTL).unwrap();
        assert_eq!(reclaimed.holder, "agent-b");
    }

    #[test]
    fn release_of_an_unclaimed_id_is_a_no_op() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store.release("ITER-007").unwrap();
        assert_eq!(store.get("ITER-007").unwrap(), None);
    }

    #[test]
    fn claims_lists_every_persisted_row() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store.claim("ITER-001", "agent-a", 1000, TTL).unwrap();
        store.claim("ITER-002", "agent-b", 1000, TTL).unwrap();

        let mut claims = store.claims().unwrap();
        claims.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].0, "ITER-001");
        assert_eq!(claims[0].1.holder, "agent-a");
        assert_eq!(claims[1].0, "ITER-002");
        assert_eq!(claims[1].1.holder, "agent-b");
    }

    #[test]
    fn fresh_claim_carries_fence_one() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let record = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        assert_eq!(record.fence, 1);
    }

    #[test]
    fn heartbeat_extends_the_lease_in_a_durable_transaction() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let claimed = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let renewed = store
            .heartbeat("ITER-007", "agent-a", claimed.fence, 5000, TTL)
            .unwrap();

        assert_eq!(renewed.due_at, 5000 + 60_000);
        assert_eq!(renewed.holder, "agent-a");
        assert_eq!(renewed.fence, claimed.fence);
        assert_eq!(store.get("ITER-007").unwrap(), Some(renewed));
    }

    #[test]
    fn heartbeat_rejects_a_holder_mismatch() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let claimed = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let err = store
            .heartbeat("ITER-007", "agent-b", claimed.fence, 5000, TTL)
            .expect_err("mismatched holder must be rejected");
        match err {
            HeartbeatError::Mismatch(held) => assert_eq!(held, claimed),
            other => panic!("expected Mismatch, got {other:?}"),
        }
        assert_eq!(store.get("ITER-007").unwrap(), Some(claimed));
    }

    #[test]
    fn heartbeat_rejects_a_fence_mismatch() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let claimed = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let err = store
            .heartbeat("ITER-007", "agent-a", claimed.fence + 1, 5000, TTL)
            .expect_err("mismatched fence must be rejected");
        match err {
            HeartbeatError::Mismatch(held) => assert_eq!(held, claimed),
            other => panic!("expected Mismatch, got {other:?}"),
        }
        assert_eq!(store.get("ITER-007").unwrap(), Some(claimed));
    }

    #[test]
    fn heartbeat_on_an_absent_claim_is_rejected() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let err = store
            .heartbeat("ITER-007", "agent-a", 1, 1000, TTL)
            .expect_err("heartbeat on an absent claim must be rejected");
        match err {
            HeartbeatError::NotFound => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn expired_lease_stays_present_and_observable_via_get() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        let claimed = store.claim("ITER-007", "agent-a", 1000, TTL).unwrap();
        let now = claimed.due_at + 1;

        let still_there = store.get("ITER-007").unwrap().unwrap();
        assert_eq!(still_there, claimed);
        assert!(still_there.due_at <= now);
    }

    #[test]
    fn committed_claim_survives_reopen_and_reads_identically() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("claims.redb");

        let written = {
            let store = Store::open(&path).unwrap();
            store.claim("ITER-007", "agent-a", 1000, TTL).unwrap()
        };

        let store = Store::open(&path).unwrap();
        assert_eq!(store.get("ITER-007").unwrap(), Some(written));
    }

    #[test]
    fn schedule_retry_persists_the_entry_and_replaces_a_prior_one_for_the_same_id() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store
            .schedule_retry("ITER-007", 1, "boom", 5_000)
            .unwrap();
        let replaced = store
            .schedule_retry("ITER-007", 2, "boom again", 9_000)
            .unwrap();

        let retries = store.retries().unwrap();
        assert_eq!(retries.len(), 1, "the prior entry must be replaced, not appended");
        assert_eq!(retries[0].0, "ITER-007");
        assert_eq!(retries[0].1, replaced);
        assert_eq!(retries[0].1.attempt, 2);
        assert_eq!(retries[0].1.error, "boom again");
        assert_eq!(retries[0].1.due_at, 9_000);
    }

    #[test]
    fn a_scheduled_retry_survives_reopen_and_is_re_derived_from_due_at() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("claims.redb");

        let scheduled = {
            let store = Store::open(&path).unwrap();
            store.schedule_retry("ITER-007", 3, "flaky", 42_000).unwrap()
        };

        let store = Store::open(&path).unwrap();
        let retries = store.retries().unwrap();
        assert_eq!(retries.len(), 1);
        assert_eq!(retries[0].0, "ITER-007");
        assert_eq!(retries[0].1, scheduled);
        assert_eq!(retries[0].1.due_at, 42_000, "due_at is re-derived unchanged");
    }

    #[test]
    fn a_retry_whose_due_at_is_already_past_at_load_is_eligible_now() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("claims.redb");

        {
            let store = Store::open(&path).unwrap();
            store.schedule_retry("ITER-past", 1, "boom", 1_000).unwrap();
            store.schedule_retry("ITER-future", 1, "boom", 100_000).unwrap();
        }

        let store = Store::open(&path).unwrap();
        let now = 50_000;
        let due = store.due_retries(now).unwrap();
        assert_eq!(due.len(), 1, "only the past-due entry is eligible");
        assert_eq!(due[0].0, "ITER-past");
        assert!(due[0].1.due_at <= now);
    }

    #[test]
    fn clear_retry_removes_the_entry() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store.schedule_retry("ITER-007", 1, "boom", 5_000).unwrap();
        store.clear_retry("ITER-007").unwrap();

        assert!(store.retries().unwrap().is_empty());
    }

    #[test]
    fn clear_retry_on_an_absent_entry_is_a_no_op() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);

        store.clear_retry("ITER-007").unwrap();
        assert!(store.retries().unwrap().is_empty());
    }
}
