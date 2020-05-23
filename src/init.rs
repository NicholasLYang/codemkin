use crate::types::{InternalConfig, UserConfig};
use crate::uploader::{create_repo, read_line};
use crate::{connect_to_db, login_user};
use anyhow::Result;
use rusqlite::{Connection, NO_PARAMS};
use std::path::PathBuf;
use std::{fs, io};

pub async fn init(directory: PathBuf) -> Result<()> {
    let mut cdmkn_dir = directory.clone();
    cdmkn_dir.push(".cdmkn");
    fs::create_dir_all(&cdmkn_dir)?;
    let db_path = {
        let mut path = cdmkn_dir.clone();
        path.push("files.db");
        path
    };
    if db_path.exists() {
        println!("Database already exists, skipping...");
    } else {
        let conn = connect_to_db(&directory)?;
        if init_tables(&conn).is_err() {
            return Err(io::Error::new(io::ErrorKind::Other, "Could not initialize tables").into());
        };
    }
    let mut name = String::new();
    read_line(&mut name, "Repo name: ")?;
    init_internal_config(&cdmkn_dir, &name).await?;
    init_user_config(&directory)?;
    println!(
        "Successfully initialized in directory {}",
        directory.to_str().unwrap()
    );
    Ok(())
}

pub fn init_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS documents (\
                        id integer primary key,\
                        relative_path text not null,\
                        canonical_path text not null unique
                        )",
        NO_PARAMS,
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS changes (\
                        id integer primary key,\
                        document_id text not null,\
                        change_elements text not null,\
                        created_at DATE DEFAULT (datetime('now','utc'))
                        )",
        NO_PARAMS,
    )?;
    Ok(())
}

async fn init_internal_config(cdmkn_dir: &PathBuf, repo_name: &str) -> Result<()> {
    let config_path = {
        let mut dir = cdmkn_dir.clone();
        dir.push("config.toml");
        dir
    };
    if config_path.exists() {
        // TODO: Add some sort of validation to check if config
        // is actually valid
        println!("Config already exists, skipping...");
    } else {
        let credentials = login_user().await?;
        let repo = create_repo(&credentials, repo_name).await?;
        let config = InternalConfig {
            id: repo.id,
            token_credentials: Some(credentials),
        };
        fs::write(config_path, toml::to_string(&config).unwrap())?;
    }
    Ok(())
}

fn init_user_config(directory: &PathBuf) -> Result<()> {
    let cdmkn_toml_path = {
        let mut dir = directory.clone();
        dir.push("cdmkn.toml");
        dir
    };
    fs::write(cdmkn_toml_path, toml::to_string(&UserConfig::new())?)?;
    Ok(())
}
