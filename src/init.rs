use crate::connect_to_db;
use crate::types::{InternalConfig, UserConfig};
use anyhow::Result;
use dialoguer::Confirm;
use rusqlite::{Connection, NO_PARAMS};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::{fs, io};

fn init() -> Result<()> {
    println!("INIT");
    let cdmkn_folder = Path::new("~/.cdmkn");
    if !cdmkn_folder.exists() {
        println!("FOLDER DOES NOT exist");
        fs::create_dir_all(cdmkn_folder)?;

        OpenOptions::new()
            .create(true)
            .write(true)
            .open(cdmkn_folder.join("database.db"))?;

        let conn = connect_to_db()?;

        init_tables(&conn)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, "Could not initialize tables"))?;
    } else {
        println!("FOLDER DOES exist");
    }

    Ok(())
}

pub async fn add_repository(repo_path: PathBuf) -> Result<()> {
    init()?;
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
            "INSERT INTO repositories (absolute_path) VALUES (?1)",
            &[&absolute_repo_path],
        )?;
        println!("Added repository");
    }

    Ok(())
}

pub fn init_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS repositories (\
                  id integer primary key,\
                  absolute_path text not null unique,\
                  created_at DATE DEFAULT (datetime('now','utc')),\
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

fn init_user_config(directory: &PathBuf) -> Result<()> {
    let cdmkn_toml_path = {
        let mut dir = directory.clone();
        dir.push("cdmkn.toml");
        dir
    };
    fs::write(cdmkn_toml_path, toml::to_string(&UserConfig::new())?)?;
    Ok(())
}
