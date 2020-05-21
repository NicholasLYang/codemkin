use crate::types::InternalConfig;
use crate::uploader::create_repo;
use crate::watcher::initialize_tables;
use crate::{connect_to_db, login_user};
use std::path::PathBuf;
use std::{fs, io};

pub async fn init(directory: PathBuf) -> Result<()> {
    let mut cdmkn_dir = directory.clone();
    cdmkn_dir.push(".cdmkn");
    fs::create_dir_all(&cdmkn_dir)?;
    if db_path.exists() {
        println!("Database already exists, skipping...");
    } else {
        let conn = connect_to_db(&directory)?;
        if initialize_tables(&conn).is_err() {
            return Err(io::Error::new(io::ErrorKind::Other, "Could not initialize tables").into());
        };
    }
    let mut name = String::new();
    read_line(&mut name, "Repo name: ")?;
    init_internal_config(&cdmkn_dir, &name)
    init_cdmkn_toml(&directory, &name)?;
    println!(
        "Successfully initialized in directory {}",
        directory.to_str().unwrap()
    );
    Ok(())
}

fn init_internal_config(cdmkn_dir: &PathBuf, repo_name: &str) {
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
        let repo= create_repo(&credentials, repo_name).await?;
        let config = InternalConfig {
            id: repo.id,
            token_credentials: Some(creds),
        };
        fs::write(config_path, toml::to_string(&config).unwrap())?;
    }
    let db_path = {
        let mut dir = cdmkn_dir.clone();
        dir.push("files.db");
        dir
    };
}

fn init_cmdkn_toml(directory: &PathBuf) {
    let cdmkn_toml_path = {
        let mut dir = directory.clone();
        dir.push("cdmkn.toml");
        dir
    };
    fs::write(cdmkn_toml_path)
}
