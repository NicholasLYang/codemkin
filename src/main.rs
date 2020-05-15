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
use std::collections::HashSet;
use std::io::{stdout, Write};
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

fn read_config() -> Result<Config, io::Error> {
    let config_str = fs::read_to_string("./.cdmkn/config.toml")?;
    let config = toml::from_str(&config_str).expect("Could not read .cdmkn/config.toml");
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
        login_user().await?;
    } else if let Some(init_matches) = matches.subcommand_matches("init") {
        let dir = if let Some(folder_name) = init_matches.value_of("dir") {
            let mut p = PathBuf::new();
            p.push(folder_name);
            p
        } else {
            env::current_dir()?
        };
        init(dir)?;
    } else if matches.subcommand_matches("push").is_some() {
        let config = read_config()?;
        let _credentials = if let Some(creds) = config.token_credentials {
            creds
        } else {
            login_user().await?.token_credentials.unwrap()
        };
        println!("TODO: Implement push");
    }
    Ok(())
}

async fn login_user() -> Result<Config, io::Error> {
    let mut config = match read_config() {
        Ok(config) => config,
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Could not read config for repository. Did you initialize the repo?",
            ));
        }
    };
    if config.token_credentials.is_some() {
        loop {
            println!("You're already logged in. Do you want to log in again? (y/n)");
            stdout().flush()?;
            let mut res = String::new();
            io::stdin().read_line(&mut res)?;
            if let Some('\n') = res.chars().next_back() {
                res.pop();
            }
            if let Some('\r') = res.chars().next_back() {
                res.pop();
            }
            match res.as_str() {
                "y" => break,
                "n" => return Ok(config),
                _ => {}
            }
        }
    }
    let credentials = match login().await {
        Ok(user) => user,
        Err(_) => return Err(io::Error::new(io::ErrorKind::Other, "Could not login")),
    };
    config.token_credentials = Some(credentials);
    write_config(&config)?;
    println!("Successfully logged in");
    Ok(config)
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
