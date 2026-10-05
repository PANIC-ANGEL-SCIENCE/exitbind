//! Disposable stdio MCP adapter. The session is fixed by the trusted launch;
//! JSON requests cannot select an actor, assignment, config or operation ID.
use super::{interact_content, state};
use crate::{config::Loaded, run};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};

const MAX_MESSAGE: u64 = 2 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: String,
    path: String,
    content: Option<String>,
}

pub(crate) fn serve(loaded: &Loaded, session: &str) -> Result<(), String> {
    let binding = state::load_binding(loaded, session)?;
    let ledger = crate::work::resolve(loaded, &binding.work)?;
    let lock = run::ledger::ledger_path(&loaded.state_root, &ledger, false)?;
    run::ledger::with_lock(&lock, || {
        if !state::preparation(loaded, &lock, session, &binding)? {
            return Err("file transport has no immutable host preparation".into());
        }
        Ok(())
    })?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        (&mut input)
            .take(MAX_MESSAGE + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() as u64 > MAX_MESSAGE {
            return Err("file transport request exceeds its bound".into());
        }
        let request: Value =
            serde_json::from_slice(&bytes).map_err(|_| "file transport requires JSON-RPC")?;
        let Some(id) = request.get("id") else {
            continue;
        };
        let response = match dispatch(loaded, session, &request) {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(message) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":message}})
            }
        };
        serde_json::to_writer(&mut output, &response).map_err(|e| e.to_string())?;
        output
            .write_all(b"\n")
            .and_then(|()| output.flush())
            .map_err(|e| e.to_string())?;
    }
}

fn dispatch(loaded: &Loaded, session: &str, request: &Value) -> Result<Value, String> {
    if request["jsonrpc"] != "2.0" {
        return Err("expected JSON-RPC 2.0".into());
    }
    match request["method"].as_str() {
        Some("initialize") => Ok(
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"exitbind-file","version":"1"}}),
        ),
        Some("ping") => Ok(json!({})),
        Some("tools/list") => Ok(
            json!({"tools":[{"name":"file","description":"Assignment-bound UTF-8 read, replace, inspection and intentional baseline refresh. Read before edit; exact replay reconciles the same intent.","inputSchema":{"type":"object","properties":{"action":{"type":"string","enum":["read","edit","inspect","refresh"]},"path":{"type":"string"},"content":{"type":"string"}},"required":["action","path"],"additionalProperties":false}}]}),
        ),
        Some("tools/call") => {
            if request["params"]["name"] != "file" {
                return Err("unknown file tool".into());
            }
            let args: Arguments = serde_json::from_value(request["params"]["arguments"].clone())
                .map_err(|_| "file tool accepts only action, path and edit content")?;
            // A long-lived transport must not reuse startup configuration for
            // reconciliation. The fixed preparation rejects a changed governing
            // configuration, including an already-completed replay.
            let current = crate::config::load(Some(
                loaded
                    .path
                    .to_str()
                    .ok_or("file transport configuration is not UTF-8")?,
            ))?;
            let result = interact_content(
                &current,
                &args.action,
                session,
                &args.path,
                args.content.as_deref(),
            );
            Ok(match result {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                }
                Err(message) => json!({"content":[{"type":"text","text":message}],"isError":true}),
            })
        }
        _ => Err("unsupported file transport method".into()),
    }
}
