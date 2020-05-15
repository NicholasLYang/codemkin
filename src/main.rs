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
extern crate uuid;

use clap::{App, AppSettings, Arg, SubCommand};
use ignore::{DirEntry, Walk};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs, io};
use uploader::login;
use uuid::Uuid;
use watcher::{initialize_tables, insert_file, watch_file};

mod uploader;
mod watcher;

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenCredentials {
    pub token: String,
    pub client: String,
    pub expiry: String,
    pub token_type: String,
    pub uid: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    repository_id: String,
    token_credentials: Option<TokenCredentials>,
}

impl Config {
    pub fn new() -> Config {
        Config {
            repository_id: Uuid::new_v4().to_string(),
            token_credentials: None,
        }
    }
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

fn read_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string("./.cdmkn/config.toml")?;
    let config = toml::from_str(&config_str)?;
    Ok(config)
}

fn write_config(config: &Config) -> Result<(), io::Error> {
    let config_str = toml::to_string(config).unwrap();
    fs::write("./.cdmkn/config.toml", config_str)
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
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("login").about("Login to API"))
        .subcommand(
            SubCommand::with_name("init")
                .about("Initialize repo")
                .arg(Arg::with_name("dir")),
        )
        .get_matches();
    if let Some(watch_matches) = matches.subcommand_matches("watch") {
        let dir = if let Some(folder_name) = watch_matches.value_of("dir") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            env::current_dir()?
        };
        init_watch(dir).await?;
    } else if matches.subcommand_matches("login").is_some() {
        let mut config = match read_config() {
            Ok(config) => config,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Could not read config for repository. Did you initialize the repo?",
                ));
            }
        };
        let (user, credentials) = match login().await {
            Ok((u, c)) => (u, c),
            Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Could not login")),
        };
        config.token_credentials = Some(credentials);
        write_config(&config)?;
        println!("Successfully logged in");
    } else if let Some(init_matches) = matches.subcommand_matches("init") {
        let dir = if let Some(folder_name) = init_matches.value_of("dir") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            env::current_dir()?
        };
        init(dir)?;
    }
    Ok(())
}

fn init(directory: PathBuf) -> Result<(), io::Error> {
    let mut directory = directory;
    directory.push(".cdmkn");
    fs::create_dir_all(&directory)?;
    let config_path = {
        let mut dir = directory.clone();
        dir.push("config.toml");
        dir
    };
    if config_path.exists() {
        println!("Config already exists, skipping...");
    } else {
        let config = Config::new();
        fs::write(config_path, toml::to_string(&config).unwrap())?;
    }
    let db_path = {
        let mut dir = directory.clone();
        dir.push("files.db");
        dir
    };
    if db_path.exists() {
        println!("Database already exists, skipping...");
    } else {
        match Connection::open(&db_path) {
            Ok(conn) => conn,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Could not create database",
                ))
            }
        };
    }
    println!(
        "Sucessfully initialized in directory {}",
        directory.to_str().unwrap()
    );
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
