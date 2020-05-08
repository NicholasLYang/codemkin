extern crate chrono;
extern crate difference;
extern crate serde_json;
extern crate tokio;

use chrono::Utc;
use difference::{Changeset, Difference};
use serde_json::json;
use std::{fs, io};
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;
use tokio::time;
use tokio::try_join;
use std::path::{Path};

fn diffs_to_json(diffs: &Vec<Difference>) -> String {
    let mut values = Vec::new();
    for diff in diffs {
        let (type_, content) = match diff {
            Difference::Add(c) => ("add", c),
            Difference::Rem(c) => ("remove", c),
            Difference::Same(c) => ("same", c),
        };
        let val = json!({
           "type": type_,
           "content": content
        });
        values.push(val.to_string());
    }
    format!("[{}]", values.join(","))
}

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    let out_path = format!("outfiles/{}", Utc::now().format("%Y-%m-%d-%H-%M-%S"));
    let out_dir = Path::new(&out_path);
    let future1 = watch_file(Path::new("src/main.rs"), out_dir, 0);
    let future2 = watch_file(Path::new("Cargo.toml"), out_dir, 1);
    try_join!(future1, future2)?;
    Ok(())
}


async fn watch_file(file_path: &Path, out_dir: &Path, index: usize) -> Result<(), io::Error>{
    let mut interval = time::interval(Duration::from_millis(1000));
    let mut previous_contents = fs::read_to_string(file_path).expect("Something went wrong");

    let mut out_dir = out_dir.to_path_buf();
    out_dir.push(format!("{}", index));
    fs::create_dir_all(&out_dir)?;
    let changes_filename = {
        let mut buf = out_dir.clone();
        buf.push("changes.json");
        buf
    };
    let mut changes_file = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(changes_filename)
        .unwrap();
    let original_filename = {
        let mut buf = out_dir.clone();
        buf.push(file_path.file_name().unwrap());
        buf
    };
    let mut original_file = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(original_filename)
        .unwrap();

    original_file.write_all(previous_contents.as_bytes())?;

    loop {
        interval.tick().await;
        let current_contents = fs::read_to_string(file_path).expect("Something went wrong");
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        if changeset.distance > 0 {
            writeln!(changes_file, "{}", diffs_to_json(&changeset.diffs))?;
        }
        previous_contents = current_contents;
    }
}