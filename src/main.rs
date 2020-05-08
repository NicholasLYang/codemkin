extern crate difference;
extern crate tokio;
extern crate serde_json;

use difference::{Changeset, Difference};
use tokio::time;
use std::time::Duration;
use serde_json::json;
use std::{fs};
use std::fs::OpenOptions;
use std::io::Write;


fn diffs_to_json(diffs: &Vec<Difference>) -> String {
    let mut values = Vec::new();
    for diff in diffs {
        let (type_ , content)= match diff {
            Difference::Add(c) => ("add", c),
            Difference::Rem(c) => ("remove", c),
            Difference::Same(c) => ("same", c)
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
async fn main() {
    let mut interval = time::interval(Duration::from_millis(500));
    let mut previous_contents = fs::read_to_string("test.js").expect("Something went wrong");
    let mut out_file = OpenOptions::new().write(true).append(true).create(true).open("out").unwrap();

    loop {
        interval.tick().await;
        let current_contents = fs::read_to_string("test.js").expect("Something went wrong");
        let changeset = Changeset::new(&previous_contents, &current_contents, "\n");
        if changeset.distance > 0 {
            writeln!(out_file, "{}", diffs_to_json(&changeset.diffs));
        }
        previous_contents = current_contents;
    }
}
