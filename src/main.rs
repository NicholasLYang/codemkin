extern crate chrono;
extern crate difference;
extern crate futures;
extern crate ignore;
extern crate rusqlite;
extern crate serde_json;
extern crate tokio;
extern crate walkdir;

use difference::{Changeset, Difference};
use futures::future::try_join_all;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs, io};
use ignore::{Walk, DirEntry};
use rusqlite::{params, Connection, Result};
use rusqlite::NO_PARAMS;
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
    let args: Vec<String> = env::args().collect();
    let dir = if args.len() < 2 {
        env::current_dir()?
    } else {
        let mut p = PathBuf::new();
        p.push(&args[1]);
        p
    };
    let db_path = {
        let mut db_pathbuf = dir.clone();
        db_pathbuf.push(".cdmkn");
        fs::create_dir_all(&db_pathbuf)?;
        db_pathbuf.push("files.db");
        db_pathbuf
    };
    let conn = match Connection::open(db_path) {
        Ok(conn) => conn,
        Err(_) => {
            return Err(io::Error::new(io::ErrorKind::Other, "Could not connect to db" ))
        }
    };

    match initialize_tables(&conn) {
        Ok(()) => (),
        Err(_) => {
            return Err(io::Error::new(io::ErrorKind::Other, "Could not initialize tables"))
        }
    };

    let mut futures = Vec::new();
    let walker = Walk::new(&dir).into_iter();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue
        };

        if is_file(&entry) {
            let id = match insert_file(&conn, entry.path()) {
                Ok(id) => id,
                Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Could not insert file into db"))
            };
            futures.push(watch_file(&conn, entry.into_path(), id));
        }
    }
    println!("Listening in directory {}", dir.to_str().unwrap_or(""));
    try_join_all(futures).await?;
    Ok(())
}

fn initialize_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("CREATE TABLE IF NOT EXISTS files (\
                        id integer primary key,\
                        path text not null unique\
                        )",
     NO_PARAMS
    )?;
    conn.execute("CREATE TABLE IF NOT EXISTS changes (\
                        id integer primary key,\
                        file_id text not null,\
                        change_elements text not null\
                        )",
                 NO_PARAMS
    )?;
    Ok(())
}

fn is_file(entry: &DirEntry) -> bool {
    entry.metadata().map(|e| e.is_file()).unwrap_or(false)
}

fn insert_file(conn: &Connection, file_path: &Path) -> Result<i64, rusqlite::Error>{
    conn.execute("INSERT OR IGNORE INTO files (path) VALUES (?1)", &[file_path.to_str().unwrap()])?;
    let id = conn.query_row("SELECT id FROM files WHERE path = ?1", &[file_path.to_str().unwrap()], |row| row.get(0))?;
    Ok(id)
}

async fn watch_file(conn: &Connection, file_path: PathBuf, id: i64) -> Result<(), io::Error> {
    let mut interval = time::interval(Duration::from_millis(500));
    let mut previous_contents = fs::read_to_string(&file_path)?;
    println!("WATCHING {:?}", file_path);
    loop {
        interval.tick().await;
        let current_contents = fs::read_to_string(&file_path).expect("Something went wrong");
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        println!("{:?}", changeset.diffs);
        if changeset.distance > 0 {
            println!("DIFF");
            let diffs_str = diffs_to_json(&changeset.diffs);
            match conn.execute("INSERT INTO changes (file_id, change_elements) VALUES (?1, ?2)", params![id, diffs_str]) {
                Ok(_) => {
                    println!("CHANGE INSERTED");
                },
                Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Could not insert changes"))
            };
        }
        previous_contents = current_contents;
    }
}
