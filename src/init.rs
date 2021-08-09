use crate::connect_to_db;
use crate::types::RepoStatus;
use crate::watcher::read_pid_file;
use dirs::home_dir;
use eyre::Result;
use rusqlite::{Connection, NO_PARAMS};
use std::fs::OpenOptions;
use std::path::Path;
use std::{fs, io};

// If try_init fails, we roll back
// This would be a lot easier with try blocks
pub fn init_folder() -> Result<()> {
    let cdmkn_folder = home_dir()
        .expect("Cannot find home directory")
        .join(".cdmkn");

    if let Err(err) = try_init_folder(&cdmkn_folder) {
        fs::remove_dir(&cdmkn_folder)?;
        Err(err)
    } else {
        Ok(())
    }
}

fn try_init_folder(cdmkn_folder: &Path) -> Result<()> {
    if !cdmkn_folder.exists() {
        fs::create_dir(&cdmkn_folder)?;

        OpenOptions::new()
            .create(true)
            .write(true)
            .open(cdmkn_folder.join("database.db"))?;

        let conn = connect_to_db()?;

        init_tables(&conn).map_err(|err| {
            eprintln!("{:?}", err);
            io::Error::new(io::ErrorKind::Other, "Could not initialize tables")
        })?;
    }

    Ok(())
}

pub async fn add_repository(repo_path: &Path) -> Result<()> {
    init_folder()?;
    if read_pid_file()?.is_none() {
        // TODO: Turn on watcher here
    }

    let absolute_repo_path = repo_path.canonicalize()?.display().to_string();
    let conn = connect_to_db()?;
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
                  created_at DATE DEFAULT (datetime('now','utc'))\
             )",
        NO_PARAMS,
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS documents (\
                 id integer primary key,\
                 repository_id integer not null,\
                 relative_path text not null,\
                 canonical_path text not null unique,\
                 created_at DATE DEFAULT (datetime('now','utc')),\
                 FOREIGN KEY (repository_id) REFERENCES repositories(id)\
             )",
        NO_PARAMS,
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS changes (\
                 id integer primary key,\
                 document_id text not null,\
                 change_elements text not null,\
                 created_at DATE DEFAULT (datetime('now','utc')),
                 FOREIGN KEY (document_id) REFERENCES documents(id)
             )",
        NO_PARAMS,
    )?;

    Ok(())
}
