use super::*;
use serde::Deserialize;
use sha2::Digest;

fn run_route(root: &Path, route: &Value, override_option: Option<(&str, &str)>) -> Output {
    let command = route["readRoute"]["command"].as_array().unwrap();
    let executable = command[0].as_str().unwrap();
    let mut arguments: Vec<String> = command[1..]
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect();
    if let Some((option, replacement)) = override_option {
        if let Some(index) = arguments.iter().position(|argument| argument == option) {
            arguments[index + 1] = replacement.to_owned();
        } else {
            arguments.extend([option.to_owned(), replacement.to_owned()]);
        }
    }
    super::support::git_topology::command(executable)
        .args(arguments)
        .current_dir(root)
        .output()
        .unwrap()
}

impl Fixture {
    fn prepare_worker(&self, goal: &str) -> String {
        let started = self.value(
            &[
                "work",
                "begin",
                "change",
                "--goal",
                goal,
                "--check-command",
                "true",
                "--proof-origin",
                "synthetic",
                "--review-policy",
                "required",
            ],
            b"",
        );
        let work = started["work"].as_str().unwrap().to_owned();
        let lead = started["next"]["assignment"].as_str().unwrap();
        let returned = self.call(
            &["work", "return", &work, lead, "--outcome", "scoped"],
            b"scope",
        );
        assert!(returned.status.success(), "{}", text(&returned));
        work
    }

    pub(super) fn act_worker(&self, work: &str, executable: &Path) -> Output {
        let output = self.call(
            &[
                "work",
                "act",
                work,
                "--codex-bin",
                executable.to_str().unwrap(),
            ],
            b"",
        );
        assert!(output.status.success(), "{}", text(&output));
        output
    }

    pub(super) fn prepare_named_child(&self, work: &str) -> Value {
        let init = json!({"action":"init","sourceRef":"owner:request",
            "sourceText":"Preserve the frozen memory boundary.",
            "requirements":[{"id":"guidance","text":"Preserve the frozen memory boundary."}]});
        let recorded = self.call(&["work", "record", work], init.to_string().as_bytes());
        assert!(recorded.status.success(), "{}", text(&recorded));
        let continuation = self.value(&["work", "continuation", work], b"");
        let token = continuation["mutationContext"]["token"].as_str().unwrap();
        self.value(
            &[
                "work",
                "bind",
                work,
                "--context",
                token,
                "--host",
                "codex",
                "--session",
                "prepared-child-parent",
                "--host-version",
                "fixture",
            ],
            b"",
        );
        let continuation = self.value(&["work", "continuation", work], b"");
        let token = continuation["mutationContext"]["token"].as_str().unwrap();
        let next = self.value(&["work", "next", work, "--full"], b"");
        let assignment = next["next"]["assignment"].as_str().unwrap();
        let agent_type = next["next"]["packet"]["nativeTaskName"].as_str().unwrap();
        self.value(
            &[
                "work",
                "child",
                "prepare",
                work,
                assignment,
                "--context",
                token,
                "--perspectives",
                r#"[{"id":"frozen-memory","path":"child-perspective.md"}]"#,
                "--agent-type",
                agent_type,
            ],
            b"",
        )
    }
}

#[test]
fn native_work_and_readable_detail_share_complete_guidance_items() {
    let fixture = Fixture::new("agent-guidance-parity");
    fixture.accept(
        "selected.md",
        "selected current guidance\n",
        "memory/selected.jsonl",
    );
    let capture = fixture.root.join("prompt");
    let executable = fixture.fake_codex(&capture);
    let work = fixture.prepare_worker("same eligible guidance");
    let detail = fixture.value(&["work", "detail", &work], b"");
    let guidance = &detail["recipientContext"]["guidance"];
    let items = guidance["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{detail}");
    assert_eq!(items[0]["content"], "selected current guidance\n");
    fixture.act_worker(&work, &executable);
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains("selected current guidance"));
    assert!(prompt.contains(items[0]["reference"]["sourceSha256"].as_str().unwrap()));
    let marker = "CURRENT ELIGIBLE GUIDANCE:\n";
    let start = prompt.find(marker).expect("guidance marker is present") + marker.len();
    let mut deserializer = serde_json::Deserializer::from_str(&prompt[start..]);
    let prompt_guidance = Value::deserialize(&mut deserializer).unwrap();
    assert_eq!(prompt_guidance["items"], guidance["items"]);
    assert_eq!(prompt_guidance["overflow"], guidance["overflow"]);
    assert_eq!(prompt_guidance["unavailable"], guidance["unavailable"]);
}

#[test]
fn oversized_guidance_remains_visible_as_whole_item_overflow() {
    let fixture = Fixture::new("agent-guidance-oversized-item");
    let content = format!("{}TAIL-OVERSIZED-ITEM", "x".repeat(16 * 1024));
    let source_sha = format!("{:x}", sha2::Sha256::digest(content.as_bytes()));
    fixture.accept("oversized.md", &content, "memory/oversized.jsonl");

    let capture = fixture.root.join("prompt");
    let executable = fixture.fake_codex(&capture);
    let work = fixture.prepare_worker("deliver oversized item");
    let detail = fixture.value(&["work", "detail", &work], b"");
    let guidance = &detail["recipientContext"]["guidance"];
    assert_eq!(guidance["selectedCount"], 0);
    assert_eq!(guidance["overflowCount"], 1);
    assert_eq!(guidance["overflow"][0]["reason"], "item_size_limit");
    assert_eq!(
        guidance["overflow"][0]["reference"]["sourceSha256"],
        source_sha
    );
    assert_eq!(guidance["overflow"][0]["bytes"], content.len());
    assert!(guidance["overflow"][0]["readRoute"]["readOnly"] == true);
    let route = &guidance["overflow"][0];
    assert!(route["readRoute"]["command"]
        .as_array()
        .unwrap()
        .windows(2)
        .all(|pair| pair[0] != "--host"));
    let expanded = run_route(&fixture.root, route, None);
    assert!(expanded.status.success(), "{}", text(&expanded));
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["content"], content);
    assert_eq!(expanded["reference"], route["reference"]);
    // Consume the current assignment before revocation. Revocation correctly
    // invalidates a frozen Work action, so launch behavior is covered above.
    fixture.act_worker(&work, &executable);
    let prompt = std::fs::read_to_string(&capture).unwrap();
    assert!(prompt.contains("item_size_limit"));
    assert!(prompt.contains(&source_sha));
    assert!(!prompt.contains(&content));
    assert!(
        !run_route(&fixture.root, route, Some(("--agent", "reviewer")))
            .status
            .success()
    );
    assert!(
        !run_route(&fixture.root, route, Some(("--host", "unknown")))
            .status
            .success()
    );
    let wrong_source = "0".repeat(64);
    assert!(!run_route(
        &fixture.root,
        route,
        Some(("--expected-sha256", wrong_source.as_str()))
    )
    .status
    .success());
    fs::write(fixture.root.join("oversized.md"), "changed source bytes\n").unwrap();
    let changed = run_route(&fixture.root, route, None);
    assert!(!changed.status.success());
    assert!(!String::from_utf8_lossy(&changed.stdout).contains(&content));
    fs::write(fixture.root.join("oversized.md"), &content).unwrap();
    let revoked = fixture.call(&["memory", "revoke", "lead", "memory/oversized.jsonl"], b"");
    assert!(revoked.status.success(), "{}", text(&revoked));
    let revoked = run_route(&fixture.root, route, None);
    assert!(!revoked.status.success());
    assert!(!String::from_utf8_lossy(&revoked.stdout).contains(&content));
}

#[test]
fn recipient_budget_keeps_items_whole_and_reports_exact_overflow() {
    let fixture = Fixture::new("agent-guidance-recipient-budget");
    let content = format!("{}TAIL-BUDGET-ITEM", "y".repeat(12 * 1024));
    fixture.accept("budget.md", &content, "memory/budget.jsonl");

    let capture = fixture.root.join("prompt");
    let executable = fixture.fake_codex(&capture);
    let work = fixture.prepare_worker("deliver budgeted guidance");
    let detail = fixture.value(&["work", "detail", &work], b"");
    let guidance = &detail["recipientContext"]["guidance"];

    fixture.act_worker(&work, &executable);
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(prompt.contains(&content));
    assert_eq!(guidance["items"].as_array().unwrap().len(), 0);
    assert_eq!(guidance["selectedCount"], 0);
    assert_eq!(guidance["overflowCount"], 1);
    assert_eq!(guidance["overflow"][0]["reason"], "recipient_budget");
    assert!(guidance["overflow"][0]["bytes"].as_u64().unwrap() as usize > content.len());
    assert_eq!(guidance["overflow"][0]["readRoute"]["readOnly"], true);
}

#[test]
fn recipient_scoped_expansion_rechecks_task_host_and_selector() {
    let fixture = Fixture::new("agent-guidance-contextual-route");
    let task = "release guidance for current work";
    let mut config: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("exitbind.json")).unwrap()).unwrap();
    config["memory"]["contextual"] = json!({"version":1,"sourcePath":"current-task.json"});
    config["agents"]["worker"]["runtime"]["host"] = json!("codex");
    fs::write(
        fixture.root.join("exitbind.json"),
        serde_json::to_vec_pretty(&config).unwrap(),
    )
    .unwrap();
    let context = |selections: Value| json!({"version":1,"task":task,"revision":"r1","approvedBy":"lead","selections":selections});
    fs::write(
        fixture.root.join("current-task.json"),
        serde_json::to_vec(&context(json!([]))).unwrap(),
    )
    .unwrap();
    let content = format!("{}TAIL-CONTEXTUAL-ITEM", "z".repeat(16 * 1024));
    fixture.accept("contextual.md", &content, "memory/contextual.jsonl");
    let candidates = fixture.value(
        &[
            "memory",
            "candidates",
            "worker",
            "--task",
            task,
            "--host",
            "codex",
            "--json",
        ],
        b"",
    );
    let reference = &candidates["candidates"][0]["reference"];
    let mut choice = candidates["choiceTemplate"].clone();
    choice["resources"] = json!([{
        "itemId":reference["itemId"],
        "sourceSha256":reference["sourceSha256"],
        "ledgerHeadSha256":reference["ledgerHeadSha256"],
        "reason":"Lead selected this exact item for the current task."
    }]);
    fs::write(
        fixture.root.join("current-task.json"),
        serde_json::to_vec(&context(json!([choice]))).unwrap(),
    )
    .unwrap();

    let work = fixture.prepare_worker(task);
    let detail = fixture.value(&["work", "detail", &work], b"");
    let overflow = &detail["recipientContext"]["guidance"]["overflow"][0];
    assert_eq!(overflow["reference"]["itemId"], reference["itemId"]);
    let expanded = run_route(&fixture.root, overflow, None);
    assert!(expanded.status.success(), "{}", text(&expanded));
    let expanded: Value = serde_json::from_slice(&expanded.stdout).unwrap();
    assert_eq!(expanded["content"], content);
    assert!(
        !run_route(&fixture.root, overflow, Some(("--task", "unrelated task")))
            .status
            .success()
    );
    assert!(
        !run_route(&fixture.root, overflow, Some(("--host", "claude")))
            .status
            .success()
    );
    assert!(
        !run_route(&fixture.root, overflow, Some(("--agent", "reviewer")))
            .status
            .success()
    );
}

#[test]
fn marked_work_and_observed_check_survive_profile_and_presentation_updates() {
    let fixture = Fixture::new("agent-guidance-continuity-currentness");
    let ledger = ".exitbind/runs/guidance-continuity.jsonl";
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let rules_path = fixture.root.join("AGENTS.md");
    let rules = b"strict project rules\n";
    fs::write(&rules_path, rules).unwrap();
    fixture.call(
        &[
            "run",
            "start",
            "change",
            "--goal",
            "retain checked work across advisory guidance updates",
            "--ledger",
            ledger,
            "--check-command",
            "true",
            "--review-policy",
            "required",
        ],
        b"",
    );
    let events = fs::read_to_string(fixture.root.join(ledger)).unwrap();
    let start: Value = serde_json::from_str(events.lines().next().unwrap()).unwrap();
    assert_eq!(start["guidanceContinuityProtocol"], 1);
    assert!(start["guidanceConfigSha256"].is_string());

    let artifact = ".exitbind/artifacts/guidance-continuity.md";
    fs::write(fixture.root.join(artifact), "scoped work\n").unwrap();
    fixture.call(
        &[
            "run",
            "submit",
            "lead",
            ledger,
            "--outcome",
            "scoped",
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
        ],
        b"",
    );

    let profile = config["agents"]["worker"]["profile"]
        .as_str()
        .unwrap()
        .to_owned();
    let profile_path = fixture.root.join(profile);
    let prior_profile = fs::read_to_string(&profile_path).unwrap();
    fs::write(
        &profile_path,
        format!("{prior_profile}\nUpdated advisory launch guidance.\n"),
    )
    .unwrap();
    config["agents"]["worker"]["purpose"] = json!("updated implementation guidance");
    config["agents"]["worker"]["displayName"] = json!("Builder");
    config["agents"]["worker"]["skills"] = json!(["review"]);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    let worker = fixture.call(
        &[
            "run",
            "submit",
            "worker",
            ledger,
            "--outcome",
            "completed",
            "--artifact",
            artifact,
            "--artifact-root",
            "state",
        ],
        b"completed under the current assignment",
    );
    assert!(worker.status.success(), "{}", text(&worker));
    let worker: Value = serde_json::from_slice(&worker.stdout).unwrap();
    let target = worker["event"]["eventSha256"].as_str().unwrap();
    let checked = fixture.call(&["run", "observe-check", ledger, "--target", target], b"");
    assert!(checked.status.success(), "{}", text(&checked));
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["event"]["result"]["code"], 0);

    let status = fixture.call(&["run", "status", ledger, "--json"], b"");
    assert!(status.status.success(), "{}", text(&status));
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(
        status["warnings"].as_array().unwrap().is_empty(),
        "{status}"
    );

    fs::write(&rules_path, b"changed project rules\n").unwrap();
    let changed_rules = fixture.call(&["run", "observe-check", ledger, "--target", target], b"");
    assert!(!changed_rules.status.success(), "{}", text(&changed_rules));
    fs::write(&rules_path, rules).unwrap();

    config["agents"]["worker"]["runtime"]["host"] = json!("claude");
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let strict = fixture.call(&["run", "observe-check", ledger, "--target", target], b"");
    assert!(!strict.status.success());
    assert!(text(&strict).contains("configuration"), "{}", text(&strict));
}

#[test]
fn marked_current_selection_tracks_advisory_memory_changes_and_live_rights() {
    let fixture = Fixture::new("agent-guidance-continuity-live-rights");
    let capture = fixture.root.join("prompt");
    let executable = fixture.fake_codex(&capture);
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["project"]["verificationPreparation"] = json!({
        "version": 1, "pythonBytecode": "disabled", "scratch": "taskOwned"
    });
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let work = fixture.prepare_worker("deliver current selected memory");

    fixture.accept(
        "current-selection.md",
        "newly selected current guidance\n",
        "memory/current-selection.jsonl",
    );
    let selected = fixture.value(&["work", "detail", &work, "--json"], b"");
    let items = selected["recipientContext"]["guidance"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1, "{selected}");
    assert_eq!(items[0]["content"], "newly selected current guidance\n");

    config["agents"]["worker"]["memoryRead"] = json!([]);
    config["agents"]["worker"]["crossContext"] = json!("none");
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let revoked = fixture.value(&["work", "detail", &work, "--json"], b"");
    assert!(
        revoked["recipientContext"]["guidance"]["items"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{revoked}"
    );
    let acted = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--codex-bin",
            executable.to_str().unwrap(),
        ],
        b"",
    );
    assert!(acted.status.success(), "{}", text(&acted));
    let prompt = fs::read_to_string(capture).unwrap();
    assert!(!prompt.contains("newly selected current guidance"));
}

#[test]
fn prepared_child_guidance_keeps_the_frozen_memory_ceiling_after_live_widening() {
    let fixture = Fixture::new("agent-guidance-prepared-child-frozen-memory");
    let config_path = fixture.root.join("exitbind.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["memory"]["protocolScopes"] = json!(["invariants", "expanded"]);
    config["agents"]["worker"]["memoryWrite"] = json!(["invariants", "expanded"]);
    config["agents"]["worker"]["memoryReview"] = json!(["invariants", "expanded"]);
    config["agents"]["lead"]["memoryPromote"] = json!(["invariants", "expanded"]);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    fs::create_dir_all(fixture.root.join("memory")).unwrap();
    fs::write(
        fixture.root.join("child-perspective.md"),
        "named child perspective fixture\n",
    )
    .unwrap();
    fs::write(
        fixture.root.join("expanded.md"),
        "newly widened live-only guidance\n",
    )
    .unwrap();
    fixture.value(
        &[
            "memory",
            "propose",
            "worker",
            "expanded.md",
            "--scope",
            "expanded",
            "--ledger",
            "memory/expanded.jsonl",
        ],
        b"",
    );
    fixture.value(
        &["memory", "review", "worker", "memory/expanded.jsonl"],
        b"",
    );
    fixture.value(&["memory", "promote", "lead", "memory/expanded.jsonl"], b"");

    let started = fixture.value(
        &[
            "work",
            "begin",
            "change",
            "--goal",
            "deliver only the assignment's frozen memory scope",
            "--check-command",
            "true",
            "--proof-origin",
            "synthetic",
            "--review-policy",
            "required",
            "--preserve-requirement",
            "guidance:Preserve the frozen memory boundary.",
            "--preservation-check-command",
            "true",
        ],
        b"",
    );
    let work = started["work"].as_str().unwrap();
    let lead = started["next"]["assignment"].as_str().unwrap();
    let returned = fixture.call(
        &["work", "return", work, lead, "--outcome", "scoped"],
        b"scope",
    );
    assert!(returned.status.success(), "{}", text(&returned));
    let before = fixture.value(&["work", "detail", &work, "--json"], b"");
    assert_eq!(
        before["sections"]["assignment"]["assignment"]["guidanceContinuityProtocol"],
        1
    );
    assert_eq!(
        before["sections"]["assignment"]["assignment"]["frozenMemoryRead"],
        json!(["invariants"])
    );
    let prepared = fixture.prepare_named_child(work);
    assert_eq!(prepared["effect"], "prepared", "{prepared}");
    let intent = prepared["intent"].as_str().unwrap();

    config["agents"]["worker"]["memoryRead"] = json!(["invariants", "expanded"]);
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let detail = fixture.value(&["work", "detail", &work, "--json"], b"");
    let child = fixture.value(&["work", "child", "context", &work, intent], b"");
    assert_eq!(detail["recipientContext"]["guidance"]["items"], json!([]));
    assert_eq!(
        detail["recipientContext"]["guidance"]["overflow"],
        json!([])
    );
    assert_eq!(
        child["guidance"]["items"],
        detail["recipientContext"]["guidance"]["items"]
    );
    assert_eq!(
        child["guidance"]["overflow"],
        detail["recipientContext"]["guidance"]["overflow"]
    );
    assert_eq!(child["guidance"]["budgetBytes"], 3 * 1024);
    assert!(!child
        .to_string()
        .contains("newly widened live-only guidance"));
}

#[test]
fn missing_or_changed_optional_source_is_omitted_without_blocking_work() {
    for (name, mutation) in [
        ("agent-guidance-optional-absent", "absent"),
        ("agent-guidance-optional-changed", "changed"),
    ] {
        let fixture = Fixture::new(name);
        let original = "eligible optional guidance\n";
        fixture.accept("optional.md", original, "memory/optional.jsonl");
        let capture = fixture.root.join("prompt");
        let executable = fixture.fake_codex(&capture);
        let work = fixture.prepare_worker("unrelated ordinary work");
        if mutation == "absent" {
            fs::remove_file(fixture.root.join("optional.md")).unwrap();
        } else {
            fs::write(
                fixture.root.join("optional.md"),
                "unvalidated changed bytes\n",
            )
            .unwrap();
        }
        let output = fixture.call(
            &[
                "work",
                "act",
                &work,
                "--codex-bin",
                executable.to_str().unwrap(),
            ],
            b"",
        );
        let current = fixture.call(&["work", "next", &work, "--full", "--json"], b"");
        assert!(
            output.status.success(),
            "current action: {}; act result: {}",
            text(&current),
            text(&output)
        );
        let prompt = fs::read_to_string(capture).unwrap();
        assert!(prompt.contains("source_unavailable_or_changed"));
        assert!(!prompt.contains("eligible optional guidance"));
        assert!(!prompt.contains("unvalidated changed bytes"));
    }
}
