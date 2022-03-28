use crate::types::Document;
use crate::utils::{cdmkn_dir, connect_to_db};
use difference::{Changeset, Difference};
use eyre::Result;
use rusqlite::{params, Connection};
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

pub fn add_document(conn: &Connection, repository_id: i64, file_path: &Path) -> Result<Document> {
    let content = fs::read_to_string(file_path)?;
    let canonical_path = file_path.canonicalize()?;
    let canonical_path_str = canonical_path.to_str().unwrap();

    conn.execute(
        "INSERT OR IGNORE INTO documents (repository_id, relative_path, canonical_path, content) VALUES (?1, ?2, ?3, ?4)",
        params![
            repository_id,
            file_path.to_str().unwrap(),
            canonical_path_str,
            &content
        ],
    )?;

    Ok(conn.query_row(
        "SELECT id, content FROM documents WHERE repository_id = ?1 AND canonical_path = ?2",
        params![repository_id, canonical_path_str],
        |row| {
            Ok(Document {
                id: row.get(0)?,
                content: row.get(1)?,
            })
        },
    )?)
}

/// Gets document from the database and compares it with the content of the file.
pub fn update_document(
    repo_id: i64,
    event_id: i64,
    path: &Path,
    old_content: &str,
    new_content: &str,
) -> Result<bool> {
    let conn = connect_to_db();

    let doc = add_document(&conn, repo_id, &path)?;

    let changes = Changeset::new(old_content, new_content, "\n");

    let changes = changes
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

    conn.execute(
        "INSERT INTO changes (document_id, event_id, change_elements) VALUES (?1, ?2, ?3)",
        params![&doc.id, &event_id, &changes],
    )?;

    Ok(())
    // TODO: Figure out way to cache documents so we don't have to replay changes
    // if changes.distance > 0 {
    //     conn.execute(
    //         "UPDATE documents SET content = ?1 WHERE id = ?2",
    //         &[new_content, format!("{}", id)],
    //     )?;
    // }
}
