use crate::init::init_folder;
use crate::utils::{cdmkn_dir, connect_to_db};
use crate::watcher::{delete_pid_file, on_update, read_pid_file, write_pid_file};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use color_eyre::Report;
use eyre::Result;
use ignore::{DirEntry, Walk};
use init::add_repository;
use notify::{recommended_watcher, RecursiveMode, Watcher};
use rusqlite::NO_PARAMS;
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
use std::{fs, io, process};
use thiserror::Error;
use tokio::time;
use watcher::insert_file;

mod init;
mod types;
mod utils;
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
        println!("cdmkn is not active");
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
        .subcommand(SubCommand::with_name("repos").about("List repos"))
        .subcommand(
            SubCommand::with_name("add")
                .about("Add repository")
                .arg(Arg::with_name("dir")),
        )
        .subcommand(SubCommand::with_name("start").about("Start watcher process"))
        .subcommand(SubCommand::with_name("stop").about("Stop watcher process"))
        .subcommand(SubCommand::with_name("status").about("See current status for codemkin"))
        .get_matches();

    if matches.subcommand_matches("repos").is_some() {
        let repos = get_repository_paths()?;

        if repos.is_empty() {
            println!("No repositories! Add one with `cdmkn add`");
        }

        for repo in repos {
            println!("{}", repo);
        }

        Ok(())
    } else if matches.subcommand_matches("start").is_some() {
        match start_watcher().await {
            Err(e) => {
                let pid_path = cdmkn_dir().join("watcher.pid");
                fs::remove_file(pid_path)?;
                Err(e)
            }
            Ok(()) => Ok(()),
        }
    } else if matches.subcommand_matches("stop").is_some() {
        if read_pid_file()?.is_some() {
            println!("Shutting down watcher...");
            fs::remove_file(cdmkn_dir().join("watcher.pid"))?;
        } else {
            println!("Watcher process is not running");
        }

        Ok(())
    } else if let Some(init_matches) = matches.subcommand_matches("add") {
        let dir = get_dir_arg(init_matches);
        let mut interval = time::interval(Duration::from_millis(5000));

        // TODO: Figure out how to send this new repo to watcher process
        // For now, we just restart watcher process. Hacky and race condition-y
        // but let's just get this working

        add_repository(&Path::new(dir)).await?;
        print!("Restarting watcher process...");
        delete_pid_file()?;
        interval.tick().await;
        start_watcher().await?;
        println!("done");

        Ok(())
    } else if matches.subcommand_matches("status").is_some() {
        print_status()
    } else {
        Ok(())
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
    if let Some(pid) = read_pid_file()? {
        println!("Watcher already running on pid {}", pid);
        return Ok(());
    }
    let pid = process::id();
    write_pid_file(pid)?;

    ctrlc::set_handler(|| {
        println!("\nShutting down...");
        delete_pid_file().expect("Could not delete PID file at ~/.cdmkn/watcher.pid")
    })?;

    let conn = connect_to_db()?;
    let mut watched_files = HashSet::new();
    let mut interval = time::interval(Duration::from_millis(5000));
    let mut watcher = recommended_watcher(|res| on_update(res).expect("Error watching file"))?;
    let repos = get_repository_paths()?;

    println!("Watching on pid {}", pid);

    while read_pid_file()?.is_some() {
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

                    if let Err(err) = insert_file(&conn, 1, &entry_path) {
                        if cfg!(debug_assertions) {
                            eprintln!("{:?}", err);
                        }
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Could not insert file into db",
                        )
                        .into());
                    };
                }
            }
        }
        interval.tick().await;
    }
    Ok(())
}
