use crate::utils::cdmkn_dir;
use eyre::Result;
use notify::event::Event;
use rusqlite::Connection;
use std::fs;
use std::path::Path;

pub fn read_pid_file() -> Result<Option<u32>> {
    let pid_path = cdmkn_dir().join("watcher.pid");

    if !pid_path.exists() {
        return Ok(None);
    }

    Ok(Some(fs::read_to_string(&pid_path)?.parse::<u32>()?))
}

pub fn write_pid_file(pid: u32) -> Result<()> {
    let pid_path = cdmkn_dir().join("watcher.pid");

    Ok(fs::write(pid_path, format!("{}", pid))?)
}

pub fn delete_pid_file() -> Result<()> {
    Ok(fs::remove_file(cdmkn_dir().join("watcher.pid"))?)
}

pub fn insert_file(conn: &Connection, repository_id: u32, file_path: &Path) -> Result<i64> {
    let content = fs::read_to_string(file_path)?;

    conn.execute(
        "INSERT OR IGNORE INTO documents (repository_id, relative_path, canonical_path, content) VALUES (?1, ?2, ?3, ?4)",
        &[
            &format!("{}", repository_id),
            file_path.to_str().unwrap(),
            file_path.canonicalize()?.to_str().unwrap(),
            &content
        ],
    )?;
    let id = conn.query_row(
        "SELECT id FROM documents WHERE canonical_path = ?1",
        &[file_path.canonicalize()?.to_str().unwrap()],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub fn on_update(res: notify::Result<Event>) {
    match res {
        Ok(event) => println!("event: {:?}", event),
        Err(e) => println!("watch error: {:?}", e),
    }
}
