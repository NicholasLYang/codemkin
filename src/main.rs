use crate::types::{TokenCredentials, UserConfig};
use anyhow::Result;
use clap::{App, AppSettings, Arg, SubCommand};
use ctrlc::set_handler;
use ignore::{DirEntry, Walk};
use init::add_repository;
use rusqlite::{Connection, NO_PARAMS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs, io, process};
use thiserror::Error;
use tokio::time;
use types::InternalConfig;
use watcher::{insert_file, watch_file};

mod history;
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

fn read_internal_config() -> Result<Option<InternalConfig>> {
    let config_path = Path::new("./.cdmkn/config.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let config_str = fs::read_to_string(&config_path)?;
    let config = toml::from_str(&config_str)?;
    Ok(Some(config))
}

fn read_user_config() -> Result<Option<UserConfig>> {
    let config_path = Path::new("./cdmkn.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let config_str = fs::read_to_string(&config_path)?;
    let config = toml::from_str(&config_str)?;
    Ok(Some(config))
}

fn read_pid_file(dir: &PathBuf) -> Result<Option<u32>> {
    let pid_path = {
        let mut path = dir.clone();
        path.push(".cdmkn");
        path.push("watcher.pid");
        path
    };
    if !pid_path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(&pid_path)?.parse::<u32>()?))
}

fn write_pid_file(pid: u32, dir: &PathBuf) -> Result<()> {
    let pid_path = {
        let mut path = dir.clone();
        path.push(".cdmkn");
        path.push("watcher.pid");
        path
    };
    Ok(fs::write(pid_path, format!("{}", pid))?)
}

fn print_status() -> Result<()> {
    let conn = connect_to_db()?;
    let count =
        conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM repositories;", NO_PARAMS, |row| {
            row.get(0)
        })?;
    println!("{}", count);
    Ok(())
}

fn write_config(config: &InternalConfig) -> Result<()> {
    println!("{:?}", config);
    let config_str = toml::to_string(config).unwrap();
    Ok(fs::write("./.cdmkn/config.toml", config_str)?)
}

#[tokio::main]
async fn main() -> Result<()> {
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
            SubCommand::with_name("init")
                .about("Initialize watcher")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("status").about("See current status for codemkin"))
        .get_matches();

    if let Some(watch_matches) = matches.subcommand_matches("init") {
        let folder = watch_matches.value_of("dir").unwrap_or(".");
        let mut dir = PathBuf::new();
        dir.push(folder);
        init_watch(Arc::new(dir)).await?;
    } else if let Some(init_matches) = matches.subcommand_matches("add") {
        let dir = if let Some(folder_name) = init_matches.value_of("dir") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            let mut p = PathBuf::new();
            p.push(".");
            p
        };
        add_repository(dir).await?;
    } else if matches.subcommand_matches("status").is_some() {
        print_status()?;
    }
    Ok(())
}

pub fn connect_to_db() -> Result<Connection> {
    match Connection::open("~/.cdmkn/database.db") {
        Ok(conn) => Ok(conn),
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "Could not connect to db").into()),
    }
}

// I have started my watch
async fn init_watch(dir: Arc<PathBuf>) -> Result<()> {
    // NOTE: This is a (not-so) subtle race condition where
    // in between reading and writing the pid file another process
    // could spin up. I'm not going to worry about that right now.
    if let Some(pid) = read_pid_file(&dir)? {
        return Err(CodemkinError::CdmknAlreadyRunning { pid }.into());
    }
    let pid = process::id();
    write_pid_file(pid, &dir)?;
    set_handler(|| {
        fs::remove_file("./.cdmkn/watcher.pid").unwrap();
        println!("Cleaning up...");
    })?;

    let conn = connect_to_db()?;
    let mut watched_files = HashSet::new();
    println!(
        "Listening in directory {}",
        (&*dir).to_str().unwrap_or("??")
    );
    let mut interval = time::interval(Duration::from_millis(5000));
    while read_pid_file(&dir)?.is_some() {
        let walker = Walk::new(&*dir).into_iter();
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
                tokio::spawn(watch_file(dir.clone(), entry_path.clone(), id));
            }
        }
        interval.tick().await;
    }
    Ok(())
}
