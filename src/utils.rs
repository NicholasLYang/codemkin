use dirs::home_dir;
use eyre::Result;
use rusqlite::Connection;
use std::io;
use std::path::PathBuf;

pub fn connect_to_db() -> Result<Connection> {
    let database_path = cdmkn_dir().join("database.db");

    match Connection::open(database_path) {
        Ok(conn) => Ok(conn),
        Err(_) => Err(io::Error::new(io::ErrorKind::Other, "Could not connect to db").into()),
    }
}

// TODO: Make a lazy_static or once_cell
#[inline]
pub fn cdmkn_dir() -> PathBuf {
    home_dir()
        .expect("Cannot find home directory")
        .join(".cdmkn")
}
