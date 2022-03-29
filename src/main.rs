use crate::init::init_folder;
use crate::types::{Document, Repository};
use crate::utils::{cdmkn_dir, connect_to_db};
use crate::watcher::{delete_pid_file, read_pid_file, update_document, write_pid_file};
use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use color_eyre::Report;
use eyre::Result;
use ignore::{DirEntry, Walk};
use init::add_repository;
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
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
pub fn validate_file_size(entry: &DirEntry) -> bool {
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

fn get_document(repo_id: i64, canonical_path: &Path) -> Result<Option<Document>> {
    let conn = connect_to_db();
    let mut stmt = conn.prepare(
        "SELECT id, content FROM documents WHERE repository_id = ?1 AND canonical_path = ?2",
    )?;

    Ok(stmt
        .query_row(params![repo_id, canonical_path.to_str().unwrap()], |row| {
            Ok(Document {
                id: row.get(0)?,
                content: row.get(1)?,
            })
        })
        .optional()?)
}

struct WatchedFile {
    content: String,
    accessed_time: SystemTime,
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

    // Create map of metadata around files such as current content, last accessed time, etc.
    let mut watched_files: HashMap<PathBuf, WatchedFile> = HashMap::new();

    while read_pid_file()?.is_some() {
        // Creating a new map of watched files so that any file that gets deleted doesn't stay in memory
        let mut new_watched_files: HashMap<PathBuf, WatchedFile> = HashMap::new();
        let repos = get_repositories()?;

        for Repository {
            id: repo_id,
            absolute_path: repo_path,
            current_event,
        } in &repos
        {
            let mut event_id: Option<i64> = None;
            let walker = Walk::new(repo_path);

            for file_entry in walker {
                let file_entry = match file_entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                // We first check if the file is within our size limits, i.e. less than 200kb
                if !validate_file_size(&file_entry) {
                    continue;
                }

                // Next we check if the file has been visited already, i.e. we're watching two directories
                // with the same file (can happen if one directory contains another)
                let file_path = file_entry.path();
                if new_watched_files.contains_key(file_path) {
                    continue;
                }

                // And finally, we check if the file has actually been changed at this time.
                let watched_file_entry = watched_files.get(file_path);
                let current_accessed_time = file_entry.metadata()?.accessed()?;

                let is_unchanged = watched_file_entry
                    .map(|entry| entry.accessed_time == current_accessed_time)
                    .unwrap_or(false);
                if is_unchanged {
                    continue;
                }

                let new_content = fs::read_to_string(&file_path)?;

                // If event has already been added, we use it
                let new_event_id = if let Some(e) = event_id {
                    e
                } else {
                    // Otherwise...we initialize the event
                    let new_event_id = add_event(*repo_id, *current_event)?;
                    event_id = Some(new_event_id);
                    new_event_id
                };

                if let Some(entry) = watched_file_entry {
                    let old_content = entry.content.as_str();

                    update_document(
                        *repo_id,
                        new_event_id,
                        &file_path,
                        old_content,
                        &new_content,
                    )?;
                } else {
                    let document = get_document(*repo_id, file_path)?;
                    let old_content = document.as_ref().map(|d| d.content.as_str()).unwrap_or("");

                    update_document(
                        *repo_id,
                        new_event_id,
                        &file_path,
                        old_content,
                        &new_content,
                    )?;
                };

                new_watched_files.insert(
                    file_path.to_path_buf(),
                    WatchedFile {
                        content: new_content,
                        accessed_time: current_accessed_time,
                    },
                );
            }
        }

        watched_files = new_watched_files;
        interval.tick().await;
    }
    Ok(())
}
