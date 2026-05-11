//! SQLite-backed evidence store (`sqlite-evidence` feature).

use crate::evidence::{EvidenceRecord, EvidenceStore, EvidenceStoreError};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct SqliteEvidenceStore {
    conn: Mutex<Connection>,
}

impl SqliteEvidenceStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EvidenceStoreError> {
        let conn = Connection::open(path).map_err(|e| EvidenceStoreError::Sqlite(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS evidence_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );",
        )
        .map_err(|e| EvidenceStoreError::Sqlite(e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl EvidenceStore for SqliteEvidenceStore {
    fn write_record(&self, record: EvidenceRecord) -> Result<(), EvidenceStoreError> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let payload = serde_json::to_string(&record.payload_json)
            .map_err(|e| EvidenceStoreError::Serde(e.to_string()))?;
        conn.execute(
            "INSERT INTO evidence_records (kind, payload_json) VALUES (?1, ?2)",
            params![record.kind, payload],
        )
        .map_err(|e| EvidenceStoreError::Sqlite(e.to_string()))?;
        Ok(())
    }
}
