use crate::utils::{cdmkn_dir, connect_to_db};
use difference::{Changeset, Difference};
use eyre::Result;
use notify::event::Event;
use notify::EventKind;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn insert_file(conn: &Connection, repository_id: u32, file_path: &Path) -> Result<()> {
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

    Ok(())
}

pub fn on_update(res: notify::Result<Event>) -> Result<()> {
    let event = res?;

    if let EventKind::Modify(_) = event.kind {
        for files in event.paths {
            get_file_diff(files)?;
        }
    }
    Ok(())
}

fn get_file_diff(path: PathBuf) -> Result<()> {
    let conn = connect_to_db()?;

    let (id, old_content): (i32, String) = conn.query_row(
        "SELECT id, content FROM documents WHERE canonical_path = ?1",
        &[path.canonicalize()?.to_str().unwrap()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let new_content = fs::read_to_string(path)?;
    let changes = Changeset::new(&old_content, &new_content, "\n");
    changes
        .diffs
        .iter()
        .map(|diff| match diff {
            Difference::Same(s) => {
                format!("{{ type: \"same\", content: \"{}\" }}", s)
            }
            Difference::Add(s) => {
                format!("{{ type: \"add\", content: \"{}\" }}", s)
            }
            Difference::Rem(s) => {
                format!("{{ type: \"remove\", content: \"{}\" }}", s)
            }
        })
        .collect::<Vec<String>>()
        .join(",");

    if changes.distance > 0 {
        conn.execute(
            "UPDATE documents SET content = ?1 WHERE id = ?2",
            &[new_content, format!("{}", id)],
        )?;
    }

    Ok(())
}
