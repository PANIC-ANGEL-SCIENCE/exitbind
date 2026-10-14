// Current Exitbind and preserved historical-reader contracts run in the default suite.
use serde_json::Value;
use sha2::{Digest, Sha256};
#[path = "../support/mod.rs"]
mod support;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const CHECK: &str = "exitbind check --config verification.json";

fn project(label: &str) -> PathBuf {
    let root = support::temp(label);
    let output = invoke(&root, &["init", "--mode", "portable", "--root", "."]);
    assert!(output.status.success(), "{}", text(&output));
    root
}

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn invoke_owned(root: &Path, arguments: Vec<String>) -> Output {
    Command::new(env!("CARGO_BIN_EXE_exitbind"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; output: {}", text(output)))
}

fn state_artifact(root: &Path, name: &str, content: &str) -> String {
    let path = root.join(".exitbind/artifacts").join(name);
    fs::write(path, content).unwrap();
    format!(".exitbind/artifacts/{name}")
}

fn submit(root: &Path, agent: &str, ledger: &str, outcome: &str, artifact: &str) -> Value {
    let output = invoke(
        root,
        &[
            "run",
            "submit",
            agent,
            ledger,
            "--outcome",
            outcome,
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    json_output(&output)
}

fn checked_start(root: &Path, ledger: &str, origin: &str) -> Value {
    let output = invoke(
        root,
        &[
            "run",
            "start",
            "change",
            "--goal",
            "checked test",
            "--ledger",
            ledger,
            "--check-command",
            CHECK,
            "--proof-origin",
            origin,
            "--config",
            "exitbind.json",
        ],
    );
    assert!(output.status.success(), "{}", text(&output));
    json_output(&output)
}

fn record_check(root: &Path, ledger: &str, target: &str, exit_code: &str) -> Output {
    record_check_duration(root, ledger, target, exit_code, None)
}

fn record_check_duration(
    root: &Path,
    ledger: &str,
    target: &str,
    exit_code: &str,
    duration_ms: Option<&str>,
) -> Output {
    let mut arguments = vec![
        "run".into(),
        "record-check".into(),
        ledger.into(),
        "--target".into(),
        target.into(),
        "--check-command".into(),
        CHECK.into(),
        "--exit-code".into(),
        exit_code.into(),
    ];
    if let Some(duration_ms) = duration_ms {
        arguments.push("--duration-ms".into());
        arguments.push(duration_ms.into());
    }
    arguments.extend(["--json".into(), "--config".into(), "exitbind.json".into()]);
    invoke_owned(root, arguments)
}

fn event_hash(submission: &Value) -> String {
    submission["event"]["eventSha256"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical(&object[key])
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap(),
    }
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value).as_bytes()))
}

fn rehash(mut event: Value) -> Value {
    event.as_object_mut().unwrap().remove("eventSha256");
    let hash = digest(&event);
    event["eventSha256"] = serde_json::json!(hash);
    event
}

fn read_events(root: &Path, ledger: &str) -> Vec<Value> {
    fs::read_to_string(root.join(ledger))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn write_events(root: &Path, ledger: &str, events: &[Value]) {
    let contents = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(root.join(ledger), format!("{contents}\n")).unwrap();
}

fn configure_workers(root: &Path, workers: &[&str]) {
    let config_path = root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let base = config["agents"]["worker"].clone();
    for worker in workers.iter().copied().filter(|worker| *worker != "worker") {
        let mut agent = base.clone();
        agent["profile"] = serde_json::json!(format!("exitbind/agents/{worker}.md"));
        agent["purpose"] = serde_json::json!(format!("Complete bounded work for {worker}."));
        config["agents"][worker] = agent;
        fs::write(
            root.join(format!("exitbind/agents/{worker}.md")),
            format!("# {worker}\n\nComplete bounded work.\n"),
        )
        .unwrap();
    }
    config["workflows"]["change"]["workers"] = serde_json::json!(workers);
    fs::write(config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
}

fn split_worker_stages(root: &Path, ledger: &str) {
    let mut events = read_events(root, ledger);
    let mut start = events[0].clone();
    let stages = start["plan"]["stages"].as_array().unwrap().clone();
    assert_eq!(stages.len(), 4);
    let worker_agents = stages[1]["agents"].as_array().unwrap().clone();
    assert_eq!(worker_agents.len(), 3);
    let mut first_worker = stages[1].clone();
    first_worker["stage"] = serde_json::json!(2);
    first_worker["agents"] = serde_json::json!([worker_agents[0].clone()]);
    first_worker["dependsOn"] = serde_json::json!([1]);
    let mut later_workers = stages[1].clone();
    later_workers["stage"] = serde_json::json!(3);
    later_workers["agents"] =
        serde_json::json!([worker_agents[1].clone(), worker_agents[2].clone()]);
    later_workers["dependsOn"] = serde_json::json!([2]);
    let mut reviewer = stages[2].clone();
    reviewer["stage"] = serde_json::json!(4);
    reviewer["dependsOn"] = serde_json::json!([3]);
    let mut final_lead = stages[3].clone();
    final_lead["stage"] = serde_json::json!(5);
    final_lead["dependsOn"] = serde_json::json!([4]);
    start["plan"]["stages"] = serde_json::json!([
        stages[0].clone(),
        first_worker,
        later_workers,
        reviewer,
        final_lead
    ]);
    events[0] = rehash(start);
    write_events(root, ledger, &events);
}
