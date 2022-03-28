use crate::connect_to_db;
use crate::types::RepoStatus;
use crate::utils::cdmkn_dir;
use eyre::Result;
use rusqlite::Connection;
use std::fs::OpenOptions;
use std::path::Path;
use std::{fs, io};

// If try_init fails, we roll back
// This would be a lot easier with try blocks
pub fn init_folder() -> Result<()> {
    let cdmkn_folder = cdmkn_dir();

    if let Err(err) = try_init_folder(&cdmkn_folder) {
        fs::remove_dir(&cdmkn_folder)?;
        Err(err)
    } else {
        Ok(())
    }
}

// Tries to initialize the folder and create the database if it doesn't exist.
fn try_init_folder(cdmkn_folder: &Path) -> Result<()> {
    if !cdmkn_folder.exists() {
        fs::create_dir(&cdmkn_folder)?;
    }

    let db_path = cdmkn_folder.join("database.db");
    let db_path_exists = db_path.exists();

    if !db_path_exists {
        OpenOptions::new().create(true).write(true).open(db_path)?;

        let conn = connect_to_db();

        init_tables(&conn).map_err(|err| {
            eprintln!("{:?}", err);
            io::Error::new(io::ErrorKind::Other, "Could not initialize tables")
        })?;
    }

    Ok(())
}

pub async fn add_repository(repo_path: &Path) -> Result<()> {
    let absolute_repo_path = repo_path.canonicalize()?.display().to_string();
    let conn = connect_to_db();
    let row_count = conn.query_row::<u32, _, _>(
        "SELECT COUNT(*) FROM repositories WHERE absolute_path = ?1",
        &[&absolute_repo_path],
        |row| row.get(0),
    )?;

    if row_count == 1 {
        println!("Repository is already added");
    } else {
        conn.execute(
            "INSERT INTO repositories (absolute_path, status) VALUES (?1, ?2)",
            &[
                &absolute_repo_path,
                &(RepoStatus::Active as usize).to_string(),
            ],
        )?;
        println!("Added repository at {}", absolute_repo_path);
    }

    Ok(())
}

pub fn init_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS repositories (\
                  id integer primary key,\
                  absolute_path text not null unique,\
                  status integer not null,\
                  current_event integer,\
                  created_at DATE DEFAULT (datetime('now','utc'))\
             )",
        [],
    )?;

    // A document is a file in a repository. It is usually source code or text
    conn.execute(
        "CREATE TABLE IF NOT EXISTS documents (\
                 id integer primary key,\
                 repository_id integer not null,\
                 relative_path text not null,\
                 canonical_path text not null unique,\
                 content text not null,\
                 created_at DATE DEFAULT (datetime('now','utc')),\
                 FOREIGN KEY (repository_id) REFERENCES repositories(id)\
             )",
        [],
    )?;

    // An event is a snapshot of a repository at a point in time.
    // We take these snapshots in a specific interval.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS events (\
                  id integer primary key,\
                  repository_id integer not null,\
                  parent_event integer,\
                  created_at DATE DEFAULT (datetime('now','utc')),\
                  FOREIGN KEY (repository_id) REFERENCES repositories(id)\
             )",
        [],
    )?;

    // A change is an update to a document for a specific event.
    // `change_elements` contain a JSON object that represents text additions/removals to a document.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS changes (\
                 id integer primary key,\
                 document_id integer not null,\
                 event_id integer not null,\
                 change_elements text not null,\
                 created_at DATE DEFAULT (datetime('now','utc')),
                 FOREIGN KEY (document_id) REFERENCES documents(id)
             )",
        [],
    )?;

    Ok(())
}
