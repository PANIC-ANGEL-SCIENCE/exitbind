//! R55 F16 (FR55-14): a journey member accepted under configuration X stays
//! integrable under Y when X -> Y moves no authority, proven by X's bytes.
//! The proof is the copy kept under the stamp's hash: the one configuration
//! writer keeps it when a journey member carries the hash, and the existing
//! `memory revalidate ... <PREVIOUS_CONFIG>` route keeps the owner's copy.
//! Without it, `goal prepare`, `work begin --journey-integration` and `goal
//! status` carry that one route. Members are never restamped.
use super::*;
use sha2::{Digest, Sha256};

const CHANGED: &str = "journey member belongs to another or changed project/session configuration";
/// The agent properties that grant nothing (F4a `NotAuthority`).
const NOT_AUTHORITY: [&str; 4] = ["profile", "displayName", "nativeName", "phaseGuidance"];

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn config(p: &Project) -> PathBuf {
    p.root.join("exitbind.json")
}

fn write_config(p: &Project, value: &Value) {
    fs::write(
        config(p),
        serde_json::to_string_pretty(value).unwrap() + "\n",
    )
    .unwrap();
}

/// The owner's copy of X, outside the project's tested inputs.
fn backup(p: &Project, bytes: &[u8]) -> String {
    let path = p.root.with_file_name("owner-backup.json");
    fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_owned()
}

fn copies(p: &Project) -> Vec<Vec<u8>> {
    let Ok(entries) = fs::read_dir(p.root.join(".exitbind/memory")) else {
        return Vec::new();
    };
    let mut found = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("stamped-config-")
        })
        .map(|path| fs::read(path).unwrap())
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// One accepted journey member stamped with X, a configuration with a
/// memory policy (the kept copy lives beside the lesson ledgers).
fn accepted_member(label: &str) -> (Project, Vec<u8>, String) {
    let p = Project::new(label).with_native_session(&format!("{label}-session"));
    let mut value: Value = serde_json::from_slice(&fs::read(config(&p)).unwrap()).unwrap();
    value["memory"] = json!({"maxBytes":16384,"maxItems":8,"protocolScopes":["project-lessons.v1"],
        "root":".exitbind/memory","syntheticScopes":[]});
    write_config(&p, &value);
    p.setup();
    let work = p.begin("R1", "sh check.sh R1");
    p.drive(&work, true);
    let x = fs::read(config(&p)).unwrap();
    (p, x, work)
}

/// X with the named-agent change of 2026-10-22: profile, nativeName and
/// displayName of the worker and the reviewer.
fn named(p: &Project, x: &[u8]) -> Value {
    let mut value: Value = serde_json::from_slice(x).unwrap();
    for (agent, name) in [("worker", "sonic"), ("reviewer", "meow")] {
        let profile = format!("exitbind/agents/{name}.md");
        fs::copy(
            p.root
                .join(value["agents"][agent]["profile"].as_str().unwrap()),
            p.root.join(&profile),
        )
        .unwrap();
        value["agents"][agent]["profile"] = json!(profile);
        value["agents"][agent]["nativeName"] = json!(name);
        value["agents"][agent]["displayName"] = json!(name);
    }
    value
}

fn prepare(p: &Project) -> Value {
    p.value(
        &[
            "goal",
            "prepare",
            "--goal-id",
            "import",
            "--requirement",
            "R1,R2,R3",
            "--artifact",
            "check.sh",
            "--check-command",
            "sh check.sh all",
            "--scope",
            "integration",
            "--json",
        ],
        None,
    )
}

fn begin_argv(prepared: &Value) -> Vec<String> {
    prepared["beginRoute"]["command"]
        .as_array()
        .unwrap_or_else(|| panic!("no begin route: {prepared}"))
        .iter()
        .map(|item| match item.as_str().unwrap() {
            "<REVIEW_DECISION>" => "required".to_owned(),
            other => other.to_owned(),
        })
        .collect()
}

fn has_flag(prepared: &Value) -> bool {
    begin_argv(prepared)
        .iter()
        .any(|word| word == "--journey-integration")
}

fn stamps(p: &Project) -> Vec<String> {
    p.status()["journeyMembers"]
        .as_array()
        .map(|members| {
            members
                .iter()
                .map(|member| member["configSha256"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_product_change_keeps_the_stamped_bytes_and_the_journey_integration_begins() {
    let (p, x, _) = accepted_member("f16-writer");
    assert!(copies(&p).is_empty(), "no copy before a change");
    let root = p.root.to_str().unwrap().to_owned();
    // `setup --apply` replaces the configuration through the one writer.
    let applied = p.call_exact_argv(
        &[
            env!("CARGO_BIN_EXE_exitbind"),
            "setup",
            "--json",
            "--apply",
            "--mode",
            "portable",
            "--root",
            &root,
            "--scope",
            "worker",
            "--observe",
            "notes",
            "--write",
            "notes",
            "--commands",
            "true",
            "--check-command",
            "sh check.sh all",
            "--goal",
            "Import names safely",
            "--review-policy",
            "required",
            "--hosts",
            "claude",
            "--skip-skills",
        ]
        .map(str::to_owned),
    );
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(applied.status.success(), "{printed}");
    assert_ne!(fs::read(config(&p)).unwrap(), x, "setup changed nothing");
    assert_eq!(copies(&p), [x.clone()], "the journey stamp was not kept");
    assert!(!printed.contains("stamped"), "the keep printed: {printed}");
    // The Lead's named-agent change on X (setup has no profile or name fact).
    write_config(&p, &named(&p, &x));
    let prepared = prepare(&p);
    assert!(
        prepared["journeyIntegrationOmitted"].is_null(),
        "{prepared}"
    );
    assert!(has_flag(&prepared), "{prepared}");
    let started = p.call_exact_argv(&begin_argv(&prepared));
    assert!(started.status.success(), "{started:?}");
    let work = serde_json::from_slice::<Value>(&started.stdout).unwrap()["work"]
        .as_str()
        .unwrap()
        .to_owned();
    let start = p.start_basis(&work);
    assert!(
        start.contains("exitbind.session-journey-integration.v1.header:"),
        "{start}"
    );
    // The member keeps its own stamp; the header binds the current bytes.
    assert_eq!(stamps(&p)[0], sha(&x));
    assert!(stamps(&p)[1..]
        .iter()
        .all(|stamp| *stamp == sha(&fs::read(config(&p)).unwrap())));
}

#[test]
fn a_hand_edit_omits_with_one_route_and_the_owners_bytes_admit() {
    let (p, x, member) = accepted_member("f16-hand");
    let owner_copy = backup(&p, &x);
    write_config(&p, &named(&p, &x));
    let prepared = prepare(&p);
    let reason = prepared["journeyIntegrationOmitted"]
        .as_str()
        .unwrap_or_else(|| panic!("no omission: {prepared}"));
    for expected in [
        CHANGED,
        "journey_member_configuration_changed",
        &member,
        "memory revalidate",
        "<PREVIOUS_CONFIG>",
    ] {
        assert!(reason.contains(expected), "{expected}: {reason}");
    }
    assert!(!has_flag(&prepared), "{prepared}");
    // work begin --journey-integration refuses with the same typed route.
    let runs = p.run_entries();
    let mut argv = begin_argv(&prepared);
    argv.extend(["--journey-integration".to_owned(), "--json".to_owned()]);
    let refused = p.call_exact_argv(&argv);
    assert!(!refused.status.success(), "{refused:?}");
    assert_eq!(p.run_entries(), runs, "the refusal started a Work");
    let typed: Value =
        serde_json::from_slice(&refused.stdout).unwrap_or_else(|_| panic!("{refused:?}"));
    assert_eq!(
        typed["classification"], "journey_member_configuration_changed",
        "{typed}"
    );
    assert_eq!(typed["boundSha256"], sha(&x));
    let route = typed["nextAction"]["command"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(&route[1..4], ["memory", "revalidate", "lead"], "{route:?}");
    assert!(
        typed["error"].as_str().unwrap().contains(&member),
        "{typed}"
    );
    // goal status shows the member unavailable with that route.
    let status = p.status().to_string();
    assert!(
        status.contains("journey_member_configuration_changed")
            && status.contains("<PREVIOUS_CONFIG>"),
        "{status}"
    );
    let filled = |previous: &str| -> Vec<String> {
        route
            .iter()
            .map(|word| match word.as_str() {
                "<PREVIOUS_CONFIG>" => previous.to_owned(),
                "<REASON>" => {
                    "the owner's copy of the configuration before the named agents".to_owned()
                }
                other => other.to_owned(),
            })
            .collect()
    };
    // A file whose hash is no stamp refuses and keeps nothing.
    let mut other = x.clone();
    other.extend(b"\n");
    let not_a_stamp = p.root.with_file_name("not-a-stamp.json");
    fs::write(&not_a_stamp, &other).unwrap();
    let wrong = p.call_exact_argv(&filled(not_a_stamp.to_str().unwrap()));
    assert!(!wrong.status.success(), "{wrong:?}");
    assert!(
        String::from_utf8_lossy(&wrong.stderr).contains("nothing was kept"),
        "{wrong:?}"
    );
    assert!(copies(&p).is_empty());
    // Run unedited with X: the copy is kept and prepare emits the flag.
    let kept = p.call_exact_argv(&filled(&owner_copy));
    assert!(kept.status.success(), "{kept:?}");
    assert_eq!(copies(&p), [x.clone()]);
    let prepared = prepare(&p);
    assert!(
        prepared["journeyIntegrationOmitted"].is_null(),
        "{prepared}"
    );
    assert!(has_flag(&prepared), "{prepared}");
    assert_eq!(stamps(&p), [sha(&x)], "a member was restamped");
}

/// A changed value for every agent property of the schema.
fn changed_value(p: &Project, property: &str) -> Value {
    match property {
        "profile" => {
            fs::write(p.root.join("exitbind/agents/other.md"), "# Other\n").unwrap();
            json!("exitbind/agents/other.md")
        }
        "displayName" => json!("Other Worker"),
        "nativeName" => json!("other_worker"),
        "phaseGuidance" => {
            fs::write(
                p.root.join("exitbind/agents/test-phase.md"),
                "Test first.\n",
            )
            .unwrap();
            json!([{"id":"test-first","phase":"test","path":"exitbind/agents/test-phase.md"}])
        }
        "purpose" => json!("Another bounded purpose."),
        "observe" | "write" => json!(["notes"]),
        "commands" => json!(["true"]),
        "skills" => json!(["exitbind"]),
        "memoryRead" | "memoryWrite" | "memoryReview" | "memoryPromote" | "memoryReject"
        | "memoryRevoke" | "memoryExpire" | "memoryForget" => json!(["project-lessons.v1"]),
        "retention" => json!("until-revoked"),
        "crossContext" => json!("same-scope"),
        "runtime" => json!({"model":"other-model"}),
        other => panic!("give the new agent property {other} a changed value here"),
    }
}

#[test]
fn with_the_stamped_bytes_kept_only_a_change_that_moves_no_authority_admits() {
    let (p, x, _) = accepted_member("f16-table");
    let owner_copy = backup(&p, &x);
    let stamped: Value = serde_json::from_slice(&x).unwrap();
    write_config(&p, &named(&p, &x));
    p.value(
        &[
            "memory",
            "revalidate",
            "lead",
            "--from-config",
            &owner_copy,
            "--reason",
            "owner copy",
            "--apply",
        ],
        None,
    );
    assert_eq!(copies(&p), [x.clone()]);
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/exitbind.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let properties = schema["$defs"]["agent"]["properties"].as_object().unwrap();
    assert!(properties.len() >= 20, "{properties:?}");
    let mut table = Vec::new();
    for property in properties.keys() {
        let mut y = stamped.clone();
        let value = changed_value(&p, property);
        assert_ne!(y["agents"]["worker"][property], value, "{property}");
        y["agents"]["worker"][property] = value;
        write_config(&p, &y);
        let prepared = prepare(&p);
        let omitted = prepared["journeyIntegrationOmitted"].as_str().unwrap_or("");
        let admits = NOT_AUTHORITY.contains(&property.as_str());
        table.push(format!(
            "{property}\t{}",
            if omitted.is_empty() {
                "admits"
            } else {
                "refuses"
            }
        ));
        if admits {
            assert!(
                omitted.is_empty() && has_flag(&prepared),
                "{property}: {prepared}"
            );
        } else {
            assert!(omitted.contains(CHANGED), "{property}: {prepared}");
            assert!(
                !omitted.contains("memory revalidate") && !has_flag(&prepared),
                "{property}: {omitted}"
            );
        }
    }
    println!("{}", table.join("\n"));
    // Another state root or project identity refuses as before, copy present.
    fs::create_dir(p.root.join("elsewhere")).unwrap();
    for (field, value) in [("root", json!("elsewhere")), ("id", json!("other-project"))] {
        let mut y = named(&p, &x);
        y["project"][field] = value;
        write_config(&p, &y);
        let output = p.call(
            &[
                "goal",
                "prepare",
                "--goal-id",
                "import",
                "--requirement",
                "R1,R2,R3",
                "--artifact",
                "check.sh",
                "--check-command",
                "sh check.sh all",
                "--scope",
                "integration",
                "--json",
            ],
            None,
        );
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !text.contains("memory revalidate") && !text.contains("--journey-integration\""),
            "{field}: {text}"
        );
    }
    write_config(&p, &named(&p, &x));
    assert_eq!(stamps(&p), [sha(&x)], "a member was restamped");
}

/// Where a member's configuration is loaded and admitted.
const ADMIT_FILE: &str = "src/session_goal/journey/integration.rs";
/// A member configuration load that is no member admission: recovery reads
/// the current configuration as the selected one, then reaches its members
/// through `current_integration_for_project` and so `project_loaded`.
const SELECTED_LOADS: &[&str] = &["src/session_goal/journey.rs"];

/// The production function enclosing byte `at` of `code`.
fn enclosing(code: &str, at: usize) -> &str {
    let start = code[..at].rfind("fn ").expect("enclosing fn");
    code[start + 3..]
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap()
}

/// Every member consumer reaches a member's configuration through the one
/// admit rule, read from the source: the only load of a member's
/// configuration path is `member_loaded`, the only call of
/// `config_stamp::admitted` is there, every `validate_member_loaded` call
/// reads a configuration that rule returned, and every production call of
/// `project_loaded` / `member_loaded` is listed with its consumer.
#[test]
fn every_member_consumer_reads_the_configuration_through_the_one_admit_rule() {
    let sources = super::rust_source::production();
    let mut consumers = Vec::new();
    let mut failures = Vec::new();
    for (file, text) in &sources {
        let code = super::rust_source::blank(text);
        for (at, _) in code.match_indices("config::load(Some(") {
            let line = &code[at..code[at..].find('\n').map_or(code.len(), |end| at + end)];
            if line.contains("member.config_path")
                && !(file == ADMIT_FILE && enclosing(&code, at) == "member_loaded")
                && !SELECTED_LOADS.contains(&file.as_str())
            {
                failures.push(format!(
                    "{file}: {} loads a member configuration outside member_loaded",
                    enclosing(&code, at)
                ));
            }
        }
        for (at, _) in code.match_indices("config_stamp::admitted(") {
            if !(file == ADMIT_FILE && enclosing(&code, at) == "member_loaded") {
                failures.push(format!(
                    "{file}: {} calls the admit rule directly",
                    enclosing(&code, at)
                ));
            }
        }
        for (at, _) in code.match_indices("validate_member_loaded(") {
            if code[..at].ends_with("fn ") {
                continue;
            }
            let function = enclosing(&code, at);
            let body = &code[code[..at].rfind("fn ").unwrap()..at];
            if ![
                "project_loaded(",
                "member_loaded(",
                "config_stamp::admitted(",
            ]
            .iter()
            .any(|reader| body.contains(reader))
            {
                failures.push(format!(
                    "{file}: {function} validates a member without the admit rule"
                ));
            }
        }
        for reader in ["project_loaded(", "member_loaded("] {
            for (at, _) in code.match_indices(reader) {
                let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
                if code[..at].ends_with("fn ") || code[..at].ends_with(ident) {
                    continue;
                }
                consumers.push(format!("{file}\t{}\t{reader}", enclosing(&code, at)));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    println!("{}", consumers.join("\n"));
    let functions = consumers
        .iter()
        .map(|line| line.split('\t').take(2).collect::<Vec<_>>().join("::"))
        .collect::<std::collections::BTreeSet<_>>();
    for consumer in [
        "src/session_goal/journey/integration.rs::covered_snapshot",
        "src/session_goal/journey/integration.rs::current_integration_for_project",
        "src/session_goal/journey/readiness.rs::member_currentness",
        "src/session_goal/journey.rs::validate_member_for_session",
        "src/session_goal/journey/integration/lineage.rs::accepted_endpoint",
    ] {
        assert!(functions.contains(consumer), "{consumer}: {functions:#?}");
    }
}
