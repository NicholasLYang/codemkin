use crate::{connect_to_db, read_pid_file};
use anyhow::Result;
use difference::{Changeset, Difference};
use rusqlite::{params, Connection};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};
use tokio::time;

fn diffs_to_json(diffs: &Vec<Difference>) -> String {
    let mut values = Vec::new();
    for diff in diffs {
        let (type_, content) = match diff {
            Difference::Add(c) => ("Add", c),
            Difference::Rem(c) => ("Remove", c),
            Difference::Same(c) => ("Same", c),
        };
        let val = json!({
           "type": type_,
           "content": content
        });
        values.push(val.to_string());
    }
    format!("[{}]", values.join(","))
}

pub fn insert_file(conn: &Connection, file_path: &Arc<PathBuf>) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO documents (relative_path, canonical_path) VALUES (?1, ?2)",
        &[
            file_path.to_str().unwrap(),
            file_path.canonicalize()?.to_str().unwrap(),
        ],
    )?;
    let id = conn.query_row(
        "SELECT id FROM documents WHERE canonical_path = ?1",
        &[file_path.canonicalize()?.to_str().unwrap()],
        |row| row.get(0),
    )?;
    Ok(id)
}

pub async fn watch_file(dir_path: Arc<PathBuf>, file_path: Arc<PathBuf>, id: i64) -> Result<()> {
    let conn = connect_to_db(&*dir_path)?;
    let mut interval = time::interval(Duration::from_millis(10000));
    let mut last_modified = file_path.metadata()?.modified()?;
    let mut previous_contents = match fs::read_to_string(&*file_path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!("Could not open file: {:?}", file_path);
            return Ok(());
        }
    };
    println!("Watching file {:?}", file_path);
    while read_pid_file(&*dir_path)?.is_some() {
        interval.tick().await;

        let metadata = (&*file_path).metadata()?;
        // If the last modified date still matches, we don't do anything
        let modified = metadata.modified()?;
        if modified == last_modified {
            continue;
        } else {
            last_modified = modified;
        }
        let current_contents = fs::read_to_string(&*file_path)?;
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        if changeset.distance > 0 {
            let diffs_str = diffs_to_json(&changeset.diffs);
            match conn.execute(
                "INSERT INTO changes (document_id, change_elements) VALUES (?1, ?2)",
                params![id, diffs_str],
            ) {
                Ok(_) => (),
                Err(_) => {
                    return Err(
                        io::Error::new(io::ErrorKind::Other, "Could not insert changes").into(),
                    )
                }
            };
            previous_contents = current_contents;
        }
    }
    Ok(())
}
