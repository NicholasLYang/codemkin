extern crate clap;
extern crate difference;
extern crate futures;
extern crate ignore;
extern crate rusqlite;
extern crate serde_json;
extern crate tokio;
extern crate walkdir;

use clap::{App, AppSettings, Arg, SubCommand};
use difference::{Changeset, Difference};
use ignore::{DirEntry, Walk};
use rusqlite::NO_PARAMS;
use rusqlite::{params, Connection, Result};
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs, io};
use tokio::time;

fn diffs_to_json(diffs: &Vec<Difference>) -> String {
    let mut values = Vec::new();
    for diff in diffs {
        let (type_, content) = match diff {
            Difference::Add(c) => ("add", c),
            Difference::Rem(c) => ("remove", c),
            Difference::Same(c) => ("same", c),
        };
        let val = json!({
           "type": type_,
           "content": content
        });
        values.push(val.to_string());
    }
    format!("[{}]", values.join(","))
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let matches = App::new("cdmkn")
        .version("0.1.0")
        .author("Nicholas Yang")
        .about("Code montages")
        .setting(AppSettings::ArgRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("watch")
                .about("Watch folder")
                .arg(Arg::with_name("folder")),
        )
        .get_matches();
    if let Some(watch_matches) = matches.subcommand_matches("watch") {
        let dir = if let Some(folder_name) = watch_matches.value_of("folder") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            env::current_dir()?
        };
        init_watch(dir).await?;
    }
    Ok(())
}

// I have started my watch
async fn init_watch(dir: PathBuf) -> Result<(), io::Error> {
    let db_path = {
        let mut db_pathbuf = dir.clone();
        db_pathbuf.push(".cdmkn");
        fs::create_dir_all(&db_pathbuf)?;
        db_pathbuf.push("files.db");
        Arc::new(db_pathbuf)
    };
    let conn = match Connection::open(&*db_path) {
        Ok(conn) => conn,
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Could not connect to db",
            ))
        }
    };

    match initialize_tables(&conn) {
        Ok(()) => (),
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Could not initialize tables",
            ))
        }
    };

    let mut watched_files = HashSet::new();
    println!("Listening in directory {}", dir.to_str().unwrap_or(""));
    loop {
        let walker = Walk::new(&dir).into_iter();
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let is_valid_file = is_valid_file(&entry);
            let entry_path = Arc::new(entry.into_path());
            let is_watched = watched_files.contains(&entry_path);
            if is_valid_file && !is_watched {
                watched_files.insert(entry_path.clone());
                let id = match insert_file(&conn, &entry_path) {
                    Ok(id) => id,
                    Err(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Could not insert file into db",
                        ))
                    }
                };
                tokio::spawn(watch_file(db_path.clone(), entry_path.clone(), id));
            }
        }
    }
}

fn initialize_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (\
                        id integer primary key,\
                        path text not null unique\
                        )",
        NO_PARAMS,
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS changes (\
                        id integer primary key,\
                        file_id text not null,\
                        change_elements text not null\
                        )",
        NO_PARAMS,
    )?;
    Ok(())
}

// If is file and is less than 200kb
// TODO: Figure out better criterion for valid files.
// Maybe file extensions?
fn is_valid_file(entry: &DirEntry) -> bool {
    entry
        .metadata()
        .map(|e| e.is_file() && e.len() < 200_000)
        .unwrap_or(false)
}

fn insert_file(conn: &Connection, file_path: &Arc<PathBuf>) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO files (path) VALUES (?1)",
        &[file_path.to_str().unwrap()],
    )?;
    let id = conn.query_row(
        "SELECT id FROM files WHERE path = ?1",
        &[file_path.to_str().unwrap()],
        |row| row.get(0),
    )?;
    Ok(id)
}

async fn watch_file(
    db_path: Arc<PathBuf>,
    file_path: Arc<PathBuf>,
    id: i64,
) -> Result<(), io::Error> {
    let conn = match Connection::open(&*db_path) {
        Ok(conn) => conn,
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Could not connect to db",
            ))
        }
    };
    let mut interval = time::interval(Duration::from_millis(500));
    let mut previous_contents = match fs::read_to_string(&*file_path) {
        Ok(content) => content,
        Err(_) => {
            eprintln!("Could not open file: {:?}", file_path);
            return Ok(());
        }
    };
    println!("Watching file {:?}", file_path);
    loop {
        interval.tick().await;
        let current_contents = fs::read_to_string(&*file_path)?;
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        if changeset.distance > 0 {
            let diffs_str = diffs_to_json(&changeset.diffs);
            match conn.execute(
                "INSERT INTO changes (file_id, change_elements) VALUES (?1, ?2)",
                params![id, diffs_str],
            ) {
                Ok(_) => (),
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "Could not insert changes",
                    ))
                }
            };
        }
        previous_contents = current_contents;
    }
}
