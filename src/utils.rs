use crate::init_folder;
use dirs::home_dir;
use rusqlite::Connection;
use std::path::PathBuf;

pub fn connect_to_db() -> Connection {
    let database_path = cdmkn_dir().join("database.db");

    if !database_path.exists() {
        println!("Database doesn't exist. Creating...");
        init_folder().expect("Unable to create folder");
    }
    Connection::open(database_path).expect("Failed to connect to database")
}

// TODO: Make a lazy_static or once_cell
#[inline]
pub fn cdmkn_dir() -> PathBuf {
    home_dir()
        .expect("Cannot find home directory")
        .join(".cdmkn")
}
