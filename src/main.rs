use crate::init::init_folder;
use crate::types::Repository;
use crate::utils::{cdmkn_dir, connect_to_db};
use crate::watcher::{delete_pid_file, read_pid_file, update_document, write_pid_file};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use color_eyre::Report;
use eyre::Result;
use ignore::{DirEntry, Walk};
use init::add_repository;
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, process};
use thiserror::Error;
use tokio::time;

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
        let conn = connect_to_db();
        let count =
            conn.query_row::<u32, _, _>("SELECT COUNT(*) FROM repositories;", [], |row| {
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
        let repos = get_repositories()?;

        if repos.is_empty() {
            println!("No repositories! Add one with `cdmkn add`");
        }

        println!("Repositories:");
        for Repository { absolute_path, .. } in repos {
            println!("- {}", absolute_path);
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
        add_repository(Path::new(dir)).await?;

        Ok(())
    } else if matches.subcommand_matches("status").is_some() {
        print_status()
    } else {
        Ok(())
    }
}

fn add_event(repo_id: i64, current_event: Option<i64>) -> Result<i64> {
    let mut conn = connect_to_db();
    let tx = conn.transaction()?;

    let event_id = {
        let mut insert_events_stmt =
            tx.prepare("INSERT INTO events (repository_id, parent_event) VALUES (?1, ?2)")?;
        insert_events_stmt.insert(params![repo_id, current_event])?
    };

    {
        let mut update_repository_stmt =
            tx.prepare("UPDATE repositories SET current_event = ?1 WHERE id = ?2")?;
        update_repository_stmt.execute(&[&event_id, &repo_id])?;
    }

    tx.commit()?;

    Ok(event_id)
}

fn get_repositories() -> Result<Vec<Repository>> {
    let conn = connect_to_db();
    let mut stmt = conn.prepare("SELECT id, absolute_path, current_event FROM repositories")?;

    let res = stmt
        .query_map([], |row| {
            Ok(Repository {
                id: row.get(0)?,
                absolute_path: row.get(1)?,
                current_event: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

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

    let mut interval = time::interval(Duration::from_millis(5000));

    println!("Watching on pid {}", pid);

    let mut file_contents: HashMap<PathBuf, String> = HashMap::new();

    while read_pid_file()?.is_some() {
        // To make sure we're not watching the same file twice
        let mut visited_files: HashMap<PathBuf, String> = HashSet::new();
        let repos = get_repositories()?;

        for Repository {
            id: repo_id,
            absolute_path: repo_path,
            current_event,
        } in &repos
        {
            let event_id = add_event(*repo_id, *current_event)?;
            let walker = Walk::new(repo_path);

            for file_entry in walker {
                let file_entry = match file_entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                let is_valid_file = is_valid_file(&file_entry);

                let file_path = file_entry.into_path();
                let not_visited = !visited_files.contains(&file_path);

                if is_valid_file && not_visited {
                    let old_content = file_contents.get(&file_path);
                    let new_content = fs::read_to_string(&file_path)?;

                    update_document(
                        *repo_id,
                        event_id,
                        &file_path,
                        old_content.map(|c| c.as_str()).unwrap_or(""),
                        &new_content,
                    )?;

                    visited_files.insert(file_path.clone());
                    file_contents.insert(file_path, new_content);
                }
            }
        }
        interval.tick().await;
    }
    Ok(())
}
