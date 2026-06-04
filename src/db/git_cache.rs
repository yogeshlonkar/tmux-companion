use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;

use crate::segments::git::GitStatus;

use super::{db_path, init_schema};

pub fn get_git_status(id: &str, ttl_secs: i64) -> anyhow::Result<Option<GitStatus>> {
    let conn = rusqlite::Connection::open(db_path())?;
    get_with_conn(&conn, id, ttl_secs)
}

pub fn save_git_status(id: &str, status: &GitStatus) -> anyhow::Result<()> {
    let conn = rusqlite::Connection::open(db_path())?;
    save_with_conn(&conn, id, status, now_secs())
}

// ── internal helpers ─────────────────────────────────────────────────────────

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn get_with_conn(
    conn: &rusqlite::Connection,
    id: &str,
    ttl_secs: i64,
) -> anyhow::Result<Option<GitStatus>> {
    init_schema(conn)?;

    let result: rusqlite::Result<(Vec<u8>, i64)> = conn.query_row(
        "SELECT status, updated_at FROM git_status WHERE id = ?1",
        rusqlite::params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok((blob, updated_at)) => {
            if now_secs() - updated_at >= ttl_secs {
                return Ok(None);
            }
            let status: GitStatus =
                serde_json::from_slice(&blob).context("deserialize cached git status")?;
            Ok(Some(status))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn save_with_conn(
    conn: &rusqlite::Connection,
    id: &str,
    status: &GitStatus,
    updated_at: i64,
) -> anyhow::Result<()> {
    init_schema(conn)?;
    let blob = serde_json::to_vec(status)?;
    conn.execute(
        "INSERT OR REPLACE INTO git_status (id, status, updated_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, blob, updated_at],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segments::git::{Area, GitStatus};

    fn test_status(branch: &str) -> GitStatus {
        GitStatus { branch: branch.into(), remote_success: true, ..Default::default() }
    }

    fn open_in_memory() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn schema_init_idempotent() {
        let conn = open_in_memory();
        // Calling init_schema a second time must not error (IF NOT EXISTS).
        init_schema(&conn).unwrap();
    }

    #[test]
    fn save_and_get_fresh() {
        let conn = open_in_memory();
        let status = test_status("main");
        save_with_conn(&conn, "abc", &status, now_secs()).unwrap();
        let got = get_with_conn(&conn, "abc", 60).unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().branch, "main");
    }

    #[test]
    fn get_missing_returns_none() {
        let conn = open_in_memory();
        let got = get_with_conn(&conn, "missing", 60).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn save_and_get_expired_returns_none() {
        let conn = open_in_memory();
        let status = test_status("main");
        // Store with updated_at in the past (10 seconds ago)
        save_with_conn(&conn, "abc", &status, now_secs() - 10).unwrap();
        // TTL of 5 seconds — 10s ago is expired
        let got = get_with_conn(&conn, "abc", 5).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn save_within_ttl_returns_some() {
        let conn = open_in_memory();
        let status = test_status("feature");
        save_with_conn(&conn, "xyz", &status, now_secs() - 1).unwrap();
        // TTL of 5 seconds — 1s ago is still fresh
        let got = get_with_conn(&conn, "xyz", 5).unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().branch, "feature");
    }

    #[test]
    fn save_overwrites_existing() {
        let conn = open_in_memory();
        save_with_conn(&conn, "id1", &test_status("first"), now_secs()).unwrap();
        save_with_conn(&conn, "id1", &test_status("second"), now_secs()).unwrap();
        let got = get_with_conn(&conn, "id1", 60).unwrap().unwrap();
        assert_eq!(got.branch, "second");
    }

    #[test]
    fn round_trips_full_status() {
        let conn = open_in_memory();
        let status = GitStatus {
            branch: "feat/test".into(),
            ahead: 3,
            behind: 1,
            staged: Area { modified: 2, added: 1, ..Default::default() },
            unstaged: Area { deleted: 1, ..Default::default() },
            stashed: 2,
            remote_success: true,
            ..Default::default()
        };
        save_with_conn(&conn, "k", &status, now_secs()).unwrap();
        let got = get_with_conn(&conn, "k", 60).unwrap().unwrap();
        assert_eq!(got.branch, "feat/test");
        assert_eq!(got.ahead, 3);
        assert_eq!(got.staged.modified, 2);
        assert_eq!(got.stashed, 2);
    }
}
