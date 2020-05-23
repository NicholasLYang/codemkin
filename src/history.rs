use crate::types::ChangeElement;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::io::stdin;
use std::path::Path;
use termion::event::{Event, Key};
use termion::input::TermRead;
use termion::raw::IntoRawMode;

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
    let stdin = stdin();
    let mut file_states = Vec::new();
    for change in changes {
        let elem_str: String = change?;
        let elements = serde_json::from_str::<Vec<ChangeElement>>(&elem_str)?;
        let file = elements
            .into_iter()
            .map(|elem| elem.content)
            .collect::<Vec<String>>()
            .join("");
        file_states.push(file);
    }
    let mut index: usize = 0;
    print!("{}", file_states[index]);
    for event in stdin.events() {
        match event? {
            Event::Key(Key::Char(' ')) => {
                index += 1;
                print!("\x1B[2J");
                print!("{}", file_states[index]);
                println!("{}", index);
            }
            _ => {}
        }
    }
    Ok(())
}
