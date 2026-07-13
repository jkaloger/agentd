use std::path::Path;
use std::time::Duration;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

const CLAIMS: TableDefinition<&str, &[u8]> = TableDefinition::new("claims");

/// A durable claim on an iteration: who holds it and when the lease expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub holder: String,
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

            if let Some(existing) = table.get(id).map_err(claim_storage)? {
                let existing: ClaimRecord =
                    serde_json::from_slice(existing.value()).map_err(ClaimError::Encode)?;
                if existing.due_at > now {
                    return Err(ClaimError::AlreadyClaimed(existing));
                }
            }

            let record = ClaimRecord {
                holder: holder.to_string(),
                due_at: now + lease_ttl.as_millis() as u64,
            };
            let bytes = serde_json::to_vec(&record).map_err(ClaimError::Encode)?;
            table.insert(id, bytes.as_slice()).map_err(claim_storage)?;
            record
        };
        txn.commit().map_err(claim_storage)?;
        Ok(record)
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
}

fn storage(e: impl Into<redb::Error>) -> StoreError {
    StoreError::Storage(e.into())
}

fn claim_storage(e: impl Into<redb::Error>) -> ClaimError {
    ClaimError::Storage(e.into())
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
}
