use log::{debug, info};
use rusqlite::{Connection, Result};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct Database {
    conn: Connection,
}
impl Database {
    pub(crate) fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS files (
                path        TEXT PRIMARY KEY,
                size        INTEGER NOT NULL,
                mtime       INTEGER NOT NULL,
                hash        BLOB,
                scanned_at  INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_files_size
                ON files(size);

            CREATE INDEX IF NOT EXISTS idx_files_hash
                ON files(hash);

            CREATE INDEX IF NOT EXISTS idx_files_size_hash
                ON files(size, hash);
        ",
        )?;

        Ok(Self { conn })
    }

    pub(crate) fn insert_file(&self, path: &Path, size: u64, mtime: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files (path, size, mtime)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(path) DO UPDATE SET
            size = excluded.size,
            mtime = excluded.mtime
        ",
            (path.to_string_lossy().as_ref(), size as i64, mtime),
        )?;

        debug!("Inserted file {:?} into database", path);
        Ok(())
    }

    pub(crate) fn update_hash(&self, path: &Path, hash: &[u8; 32]) -> Result<()> {
        let scanned_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is before Unix epoch")
            .as_secs() as i64;

        self.conn.execute(
            "
        UPDATE files
        SET hash = ?1,
            scanned_at = ?2
        WHERE path = ?3
        ",
            (hash.as_slice(), scanned_at, path.to_string_lossy().as_ref()),
        )?;

        debug!("Updated hash for file {:?} into database", path);
        Ok(())
    }

    pub(crate) fn has_valid_hash(&self, path: &Path, mtime: i64) -> Result<bool> {
        let valid: bool = self.conn.query_row(
            "
        SELECT EXISTS(
            SELECT 1
            FROM files
            WHERE path = ?1
              AND mtime >= ?2
              AND hash IS NOT NULL
        )
        ",
            (path.to_string_lossy().as_ref(), mtime),
            |row| row.get(0),
        )?;

        if valid {
            debug!("File {} has valid hash in database", path.display());
        } else {
            debug!("File {} has no valid hash in database", path.display());
        }
        Ok(valid)
    }
}
