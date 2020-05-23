extern crate anyhow;
extern crate chrono;
extern crate clap;
extern crate ctrlc;
extern crate difference;
extern crate futures;
extern crate ignore;
extern crate reqwest;
extern crate rusqlite;
extern crate serde;
extern crate serde_json;
extern crate thiserror;
extern crate tokio;
extern crate toml;

use crate::history::display_file_history;
use crate::types::{TokenCredentials, UserConfig};
use crate::uploader::{login, push_repo, register};
use anyhow::Result;
use clap::{App, AppSettings, Arg, SubCommand};
use ctrlc::set_handler;
use ignore::{DirEntry, Walk};
use init::init;
use rusqlite::Connection;
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
mod uploader;
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
    let mut dir = PathBuf::new();
    dir.push(".");
    let pid = read_pid_file(&dir)?;
    if let Some(pid) = pid {
        println!("Codemkin is watching this directory on pid {}", pid);
    } else {
        println!("Codemkin is not watching this directory");
    }
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
        .about("Code montages")
        .setting(AppSettings::ArgRequiredElseHelp)
        .setting(AppSettings::ColoredHelp)
        .subcommand(
            SubCommand::with_name("watch")
                .about("Watch folder")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("login").about("Login to API"))
        .subcommand(SubCommand::with_name("register").about("Register an account"))
        .subcommand(
            SubCommand::with_name("init")
                .about("Initialize repo")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("push").about("Push repo to server"))
        .subcommand(
            SubCommand::with_name("history")
                .about("See history for a file")
                .arg(Arg::with_name("file").required(true)),
        )
        .subcommand(SubCommand::with_name("status").about("See current status for codemkin"))
        .get_matches();

    if let Some(watch_matches) = matches.subcommand_matches("watch") {
        let folder = watch_matches.value_of("dir").unwrap_or(".");
        let mut dir = PathBuf::new();
        dir.push(folder);
        init_watch(Arc::new(dir)).await?;
    } else if matches.subcommand_matches("login").is_some() {
        let creds = login_user().await?;
        let mut config = read_internal_config()?.ok_or(CodemkinError::InvalidCdmknFolder)?;
        config.token_credentials = Some(creds);
        write_config(&config)?;
    } else if let Some(init_matches) = matches.subcommand_matches("init") {
        let dir = if let Some(folder_name) = init_matches.value_of("dir") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            let mut p = PathBuf::new();
            p.push(".");
            p
        };
        init(dir).await?;
    } else if matches.subcommand_matches("push").is_some() {
        let config = read_internal_config()?.ok_or(CodemkinError::InvalidCdmknFolder)?;
        let credentials = if let Some(creds) = config.token_credentials {
            creds
        } else {
            login_user().await?
        };
        let conn = connect_to_db(&env::current_dir()?)?;
        push_repo(&conn, &config.id, &credentials).await?;
    } else if let Some(history_matches) = matches.subcommand_matches("history") {
        if let Some(file_path) = history_matches.value_of("file") {
            display_file_history(Path::new(file_path), &connect_to_db(Path::new("."))?)?;
        }
    } else if matches.subcommand_matches("register").is_some() {
        let creds = register().await?;
        let mut config = read_internal_config()?.ok_or(CodemkinError::InvalidCdmknFolder)?;
        config.token_credentials = Some(creds);
        write_config(&config)?;
    } else if matches.subcommand_matches("status").is_some() {
        let dir_path = Path::new("./.cdmkn");
        if !dir_path.exists() {
            println!("Codemkin is not initialized in this directory")
        }
        print_status()?;
    }
    Ok(())
}

pub fn connect_to_db(dir: &Path) -> Result<Connection> {
    let db_path = {
        let mut db_pathbuf = dir.to_path_buf();
        db_pathbuf.push(".cdmkn");
        fs::create_dir_all(&db_pathbuf)?;
        db_pathbuf.push("files.db");
        Arc::new(db_pathbuf)
    };
    match Connection::open(&*db_path) {
        Ok(conn) => Ok(conn),
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "Could not connect to db").into()),
    }
}

async fn login_user() -> Result<TokenCredentials> {
    println!("Please login");
    let credentials = login().await?;
    println!("Successfully logged in");
    Ok(credentials)
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

    let conn = connect_to_db(&*dir)?;
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
