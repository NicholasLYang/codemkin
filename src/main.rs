extern crate anyhow;
extern crate clap;
extern crate difference;
extern crate futures;
extern crate ignore;
extern crate reqwest;
extern crate rusqlite;
extern crate serde;
extern crate serde_json;
extern crate tokio;
extern crate toml;

use crate::types::TokenCredentials;
use crate::uploader::{create_repo, login, push_repo, register};
use anyhow::Result;
use clap::{App, AppSettings, Arg, SubCommand};
use ignore::{DirEntry, Walk};
use init::init;
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{env, fs, io};
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::Terminal;
use types::InternalConfig;
use watcher::{insert_file, watch_file};

mod init;
mod types;
mod uploader;
mod watcher;

// If is file and is less than 200kb
// TODO: Figure out better criterion for valid files.
// Maybe file extensions?
pub fn is_valid_file(entry: &DirEntry) -> bool {
    entry
        .metadata()
        .map(|e| e.is_file() && e.len() < 200_000)
        .unwrap_or(false)
}

fn read_config() -> Result<InternalConfig> {
    let config_path = Path::new("./.cdmkn/config.toml");
    if !config_path.exists() {
        eprintln!("Config doesn't exist. Did you initialize?");
        return Err(io::Error::new(io::ErrorKind::NotFound, "Could not find config").into());
    }
    let config_str = fs::read_to_string(&config_path)?;
    let config = toml::from_str(&config_str).expect("Could not read .cdmkn/config.toml");
    Ok(config)
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
        .get_matches();
    let stdout = io::stdout().into_raw_mode()?;
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    if let Some(watch_matches) = matches.subcommand_matches("watch") {
        let folder = watch_matches.value_of("dir").unwrap_or(".");
        let mut dir = PathBuf::new();
        dir.push(folder);
        init_watch(Arc::new(dir)).await?;
    } else if matches.subcommand_matches("login").is_some() {
        let creds = login_user().await?;
        let mut config = read_config()?;
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
        let config = read_config()?;
        let credentials = if let Some(creds) = config.token_credentials {
            creds
        } else {
            login_user().await?
        };
        let conn = connect_to_db(&env::current_dir()?)?;
        push_repo(&conn, &config.id, &credentials).await?;
    } else if let Some(init_matches) = matches.subcommand_matches("history") {
        print!("\x1B[2J");
        loop {}
    } else if matches.subcommand_matches("register").is_some() {
        let creds = register().await?;
        if let Ok(config) = read_config() {
            let mut config = config;
            config.token_credentials = Some(creds);
            write_config(&config)?;
        }
    }
    Ok(())
}

pub fn connect_to_db(dir: &PathBuf) -> Result<Connection> {
    let db_path = {
        let mut db_pathbuf = dir.clone();
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
    let conn = connect_to_db(&*dir)?;

    let mut watched_files = HashSet::new();
    println!(
        "Listening in directory {}",
        (&*dir).to_str().unwrap_or("??")
    );
    loop {
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
    }
}
