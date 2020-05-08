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
    let mut interval = time::interval(Duration::from_millis(1000));
    let mut previous_contents = fs::read_to_string("src/main.rs").expect("Something went wrong");
    let out_filename = format!("outfiles/{}.json", Utc::now());
    let mut out_file = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(out_filename)
        .unwrap();

    loop {
        interval.tick().await;
        let current_contents = fs::read_to_string("src/main.rs").expect("Something went wrong");
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        if changeset.distance > 0 {
            writeln!(out_file, "{}", diffs_to_json(&changeset.diffs))?;
        }
        previous_contents = current_contents;
    }
}
