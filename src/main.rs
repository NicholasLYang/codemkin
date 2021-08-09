use crate::init::init_folder;
use crate::watcher::{on_update, read_pid_file, write_pid_file};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use color_eyre::Report;
use dirs::home_dir;
use eyre::Result;
use ignore::{DirEntry, Walk};
use init::add_repository;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use rusqlite::{Connection, NO_PARAMS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io, process};
use thiserror::Error;
use tokio::time;
use watcher::insert_file;

mod init;
mod types;
mod watcher;

#[derive(Error, Debug)]
pub enum CodemkinError {
    #[error("Invalid configuration. Did you initialize?")]
    InvalidCdmknFolder,
    #[error("Codemkin is already running on pid {pid}")]
    CdmknAlreadyRunning { pid: u32 },
}

// If is file and is less than 200kb
// TODO: Figure out better criterion for valid files.
// Maybe file extensions?
pub fn is_valid_file(entry: &DirEntry) -> bool {
    entry
        .metadata()
        .map(|e| e.is_file() && e.len() < 200_000)
        .unwrap_or(false)
}

fn print_status() -> Result<()> {
    if let Some(pid) = read_pid_file()? {
        let conn = connect_to_db()?;
        let count =
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM repositories;", NO_PARAMS, |row| {
                row.get(0)
            })?;
        println!("Watching {} repositories on pid {}", count, pid);
    } else {
        println!("cmdkn is not active");
    }

    Ok(())
}

fn get_dir_arg<'a>(matches: &'a ArgMatches) -> &'a str {
    if let Some(folder_name) = matches.value_of("dir") {
        folder_name
    } else {
        "."
    }
}

fn setup() -> Result<(), Report> {
    if std::env::var("RUST_LIB_BACKTRACE").is_err() {
        std::env::set_var("RUST_LIB_BACKTRACE", "1")
    }
    color_eyre::install()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    setup()?;

    let matches = App::new("cdmkn")
        .version("0.1.0")
        .author("Nicholas Yang")
        .about("Code watcher utility")
        .setting(AppSettings::ArgRequiredElseHelp)
        .setting(AppSettings::ColoredHelp)
        .subcommand(
            SubCommand::with_name("add")
                .about("Add repository")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(
            SubCommand::with_name("start")
                .about("Start watching current repo")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(
            SubCommand::with_name("stop")
                .about("Stop watching current repo")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("status").about("See current status for codemkin"))
        .get_matches();

    if let Some(start_matches) = matches.subcommand_matches("start") {
        match start_watcher().await {
            Err(e) => {
                let pid_path = home_dir()
                    .expect("Could not find home directory")
                    .join(".cdmkn")
                    .join("watcher.pid");

                fs::remove_file(pid_path)?;

                return Err(e);
            }
            Ok(()) => {}
        }
        //let dir = get_dir_arg(watch_matches);
        // TODO: Figure out how to send this new repo to watcher process
    } else if let Some(init_matches) = matches.subcommand_matches("add") {
        let dir = get_dir_arg(init_matches);
        add_repository(&Path::new(dir)).await?;
    } else if matches.subcommand_matches("status").is_some() {
        print_status()?;
    }
    Ok(())
}

pub fn connect_to_db() -> Result<Connection> {
    let database_path = home_dir()
        .expect("Cannot find home directory")
        .join(".cdmkn/database.db");

    match Connection::open(database_path) {
        Ok(conn) => Ok(conn),
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "Could not connect to db").into()),
    }
}

fn get_repository_paths() -> Result<Vec<String>> {
    let conn = connect_to_db()?;
    let mut stmt = conn.prepare("SELECT absolute_path FROM repositories")?;

    let res = stmt
        .query_map::<String, _, _>(NO_PARAMS, |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(res)
}

async fn start_watcher() -> Result<()> {
    init_folder()?;
    // NOTE: This is a (not-so) subtle race condition where
    // in between reading and writing the pid file another process
    // could spin up. I'm not going to worry about that right now.
    if read_pid_file()?.is_some() {
        return Ok(());
    }
    let pid = process::id();
    write_pid_file(pid)?;

    let conn = connect_to_db()?;
    let mut watched_files = HashSet::new();
    let mut interval = time::interval(Duration::from_millis(5000));
    let mut watcher = recommended_watcher(|res| on_update(res))?;
    let repos = get_repository_paths()?;

    while read_pid_file()?.is_some() {
        println!("{:?}", watched_files);

        for repo in &repos {
            let walker = Walk::new(repo).into_iter();

            for entry in walker {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let is_valid_file = is_valid_file(&entry);
                let entry_path = entry.into_path();
                let is_watched = watched_files.contains(&entry_path);

                if is_valid_file && !is_watched {
                    watched_files.insert(entry_path.clone());
                    watcher.watch(&entry_path, RecursiveMode::NonRecursive)?;

                    match insert_file(&conn, 1, &entry_path) {
                        Ok(id) => id,
                        Err(err) => {
                            if cfg!(debug_assertions) {
                                eprintln!("{:?}", err);
                            }
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                "Could not insert file into db",
                            )
                            .into());
                        }
                    };
                }
            }
        }
        interval.tick().await;
    }
    Ok(())
}
