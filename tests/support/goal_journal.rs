//! Test oracle for the stored Goal journal (`session-goal.jsonl`): a line is
//! either a complete record or a change record
//! (`{"kind":"lead_session_goal_delta","version":1,"base":..,"eventSha256":..,"changes":[..]}`)
//! applied to the complete record before it. Written independently of the
//! product reader in `src/session_goal/journal_form.rs`.

use serde_json::Value;
use std::path::Path;

/// Every complete Goal record of the journal at `path`, in order.
pub fn records(path: &Path) -> Vec<Value> {
    let raw = std::fs::read_to_string(path).expect("read Goal journal");
    let mut records: Vec<Value> = Vec::new();
    for line in raw.lines() {
        let line: Value = serde_json::from_str(line).expect("Goal journal line is JSON");
        if line["kind"] != "lead_session_goal_delta" {
            records.push(line);
            continue;
        }
        let base = records.last().expect("a change record follows a record");
        assert_eq!(line["base"], base["eventSha256"], "change record base");
        let mut record = base.clone();
        for change in line["changes"].as_array().expect("change list") {
            let path = change[1].as_array().expect("change path");
            match change[0].as_str() {
                Some("set") => place(&mut record, path, change[2].clone()),
                Some("copy") => {
                    let source = change[2].as_array().expect("copy source");
                    let from = source.iter().fold(base, |value, segment| match segment {
                        Value::String(key) => &value[key.as_str()],
                        other => &value[other.as_u64().expect("path index") as usize],
                    });
                    place(&mut record, path, from.clone());
                }
                Some("del") => {
                    let (key, parent) = path.split_last().expect("removed field");
                    at(&mut record, parent)
                        .as_object_mut()
                        .expect("object")
                        .remove(key.as_str().expect("field name"))
                        .expect("removed field exists");
                }
                Some("len") => {
                    let length = change[2].as_u64().expect("length") as usize;
                    at(&mut record, path)
                        .as_array_mut()
                        .expect("list")
                        .truncate(length);
                }
                other => panic!("unknown Goal journal change {other:?}"),
            }
        }
        record["eventSha256"] = line["eventSha256"].clone();
        records.push(record);
    }
    records
}

/// The latest complete Goal record of the journal at `path`.
pub fn latest(path: &Path) -> Value {
    records(path).pop().expect("Goal journal has a record")
}

/// Rewrite the journal at `path` as complete `records`.
pub fn write(path: &Path, records: &[Value]) {
    let lines = records.iter().map(|record| format!("{record}\n"));
    std::fs::write(path, lines.collect::<String>()).expect("write Goal journal");
}

enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

fn index(segment: &Value) -> Segment<'_> {
    match segment {
        Value::String(key) => Segment::Key(key),
        other => Segment::Index(other.as_u64().expect("path segment") as usize),
    }
}

fn at<'a>(record: &'a mut Value, path: &[Value]) -> &'a mut Value {
    path.iter()
        .fold(record, |value, segment| match index(segment) {
            Segment::Key(key) => value.get_mut(key).expect("path field"),
            Segment::Index(index) => value.get_mut(index).expect("path index"),
        })
}

fn place(record: &mut Value, path: &[Value], value: Value) {
    let Some((last, parent)) = path.split_last() else {
        *record = value;
        return;
    };
    let parent = at(record, parent);
    match index(last) {
        Segment::Key(key) => {
            parent
                .as_object_mut()
                .expect("object")
                .insert(key.to_owned(), value);
        }
        Segment::Index(index) => {
            let list = parent.as_array_mut().expect("list");
            if index == list.len() {
                list.push(value);
            } else {
                list[index] = value;
            }
        }
    }
}
