pub mod git_cache;

use std::path::PathBuf;

pub use git_cache::{get_git_status, save_git_status};

pub fn db_path() -> PathBuf {
    PathBuf::from("/tmp/.tmux-companion.db")
}

pub fn init_schema(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS git_status (
            id         TEXT PRIMARY KEY,
            status     BLOB NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )?;
    Ok(())
}
