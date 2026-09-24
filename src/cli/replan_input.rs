use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;

use serde::Deserialize;

use super::args::Arguments;

const MAX_INPUT_BYTES: u64 = 16 * 1024;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReplanInput {
    pub hypothesis: Option<String>,
    pub evidence_request: Option<String>,
    pub scope_decision: Option<String>,
    pub blocker: Option<String>,
}

impl ReplanInput {
    pub fn from_args(args: &Arguments) -> Result<Self, String> {
        if let Some(path) = args.options.get("replan-file") {
            if [
                "hypothesis",
                "evidence-request",
                "scope-decision",
                "blocker",
            ]
            .iter()
            .any(|name| args.options.contains_key(*name))
            {
                return Err("--replan-file conflicts with inline re-plan fields".into());
            }
            let bytes = if path == "-" {
                limited_read(io::stdin().lock())?
            } else {
                let file = open_regular(path)?;
                limited_read(file)?
            };
            return serde_json::from_slice(&bytes)
                .map_err(|error| format!("invalid re-plan input: {error}"));
        }
        Ok(Self {
            hypothesis: args.options.get("hypothesis").cloned(),
            evidence_request: args.options.get("evidence-request").cloned(),
            scope_decision: args.options.get("scope-decision").cloned(),
            blocker: args.options.get("blocker").cloned(),
        })
    }
}

fn open_regular(path: &str) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("cannot open re-plan file: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("cannot inspect re-plan file: {error}"))?
        .is_file()
    {
        return Err("re-plan input must be a regular file".into());
    }
    Ok(file)
}

fn limited_read(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read re-plan input: {error}"))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("re-plan input exceeds 16 KiB".into());
    }
    Ok(bytes)
}
