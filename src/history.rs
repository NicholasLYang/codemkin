use crate::types::ChangeElement;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub fn display_file_history(path: &Path, conn: &Connection) -> Result<()> {
    let mut changes_query = conn.prepare(
        "SELECT change_elements FROM changes \
    JOIN documents ON changes.document_id = documents.id \
    WHERE documents.canonical_path == ?1\
    ORDER BY created_at DESC \
    LIMIT 100",
    )?;
    let changes = changes_query
        .query_map(params![path.canonicalize()?.to_str().unwrap()], |row| {
            Ok(row.get(0)?)
        })?;
    for change in changes {
        let elem_str: String = change?;
        let elements = serde_json::from_str::<Vec<ChangeElement>>(&elem_str)?;
        for elem in elements {
            print!("{}", elem.content);
        }
    }
    Ok(())
}
