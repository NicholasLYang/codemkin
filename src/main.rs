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
extern crate uuid;

use crate::types::TokenCredentials;
use crate::uploader::{init_repo, push_repo};
use anyhow::Result;
use clap::{App, AppSettings, Arg, SubCommand};
use ignore::{DirEntry, Walk};
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs, io};
use types::Config;
use uploader::login;
use watcher::{initialize_tables, insert_file, watch_file};

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

fn read_config() -> Result<Config> {
    let config_str = fs::read_to_string("./.cdmkn/config.toml")?;
    let config = toml::from_str(&config_str).expect("Could not read .cdmkn/config.toml");
    Ok(config)
}

fn write_config(config: &Config) -> Result<()> {
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
        .subcommand(
            SubCommand::with_name("init")
                .about("Initialize repo")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("push").about("Push repo to server"))
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
            env::current_dir()?
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

async fn init(directory: PathBuf) -> Result<()> {
    let mut directory = directory;
    directory.push(".cdmkn");
    fs::create_dir_all(&directory)?;
    let config_path = {
        let mut dir = directory.clone();
        dir.push("config.toml");
        dir
    };
    if config_path.exists() {
        // TODO: Add some sort of validation to check if config
        // is actually valid
        println!("Config already exists, skipping...");
    } else {
        let credentials = login_user().await?;
        let (creds, repo) = init_repo(&credentials).await?;
        let config = Config {
            id: repo.id,
            token_credentials: Some(creds),
        };
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
        let conn = connect_to_db(&directory)?;
        match initialize_tables(&conn) {
            Ok(()) => (),
            Err(_) => {
                return Err(
                    io::Error::new(io::ErrorKind::Other, "Could not initialize tables").into(),
                )
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
async fn init_watch(dir: PathBuf) -> Result<()> {
    let conn = connect_to_db(&dir)?;

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
                        )
                        .into())
                    }
                };
                tokio::spawn(watch_file(dir.clone(), entry_path.clone(), id));
            }
        }
    }
}
