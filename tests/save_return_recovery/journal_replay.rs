use super::*;

#[test]
fn completed_same_work_journal_is_replayed_without_provider_spawn() {
    let fixture = Fixture::new("save-return-recovery");
    let (work, body) = fixture.worker_journal();
    let marker = fixture.marker();
    let before = fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap()
    .lines()
    .count();
    let output = fixture.call(&["work", "act", &work, "--resume", "--themed"], b"");
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("completed"));
    let themed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(themed["world"]["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["name"] == "passing_black_cat" && m["active"] == true));
    let later = fixture.value(&["work", "world", &work, "--json"], b"");
    assert!(later["motifs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["name"] == "passing_black_cat" && m["active"] == false));
    assert!(!marker.exists());
    let after = fs::read_to_string(
        fixture
            .root
            .join(format!(".exitbind/runs/work-{}.jsonl", &work[4..])),
    )
    .unwrap()
    .lines()
    .count();
    assert_eq!(after, before);
    assert!(text(&output).contains(&sha256(&body)));
}

#[test]
fn uncertain_native_journal_refuses_without_provider_spawn() {
    let fixture = Fixture::new("save-return-uncertain");
    let (work, _) = fixture.worker_journal_with_submission(false);
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let journal_path = fs::read_dir(directory)
        .unwrap()
        .find_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|value| value.to_str()) == Some("json")).then_some(path)
        })
        .unwrap();
    let mut journal: Value = serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
    journal["status"] = json!("running");
    let provisional = json!({"outcome":"completed","summary":"held","reason":""});
    journal["provisionalFinalResultBytes"] =
        json!(hex_encode(&serde_json::to_vec(&provisional).unwrap()));
    journal["provisionalFinalResult"] = provisional;
    fs::write(&journal_path, serde_json::to_vec(&journal).unwrap()).unwrap();
    let before = fs::read(&journal_path).unwrap();
    let inspected = fixture.value(&["work", "act", &work, "--inspect"], b"");
    assert_eq!(inspected["effect"], "no-change");
    assert_eq!(inspected["reconciliation"], "inspection_required");
    assert_eq!(inspected["nextAction"]["safe"], false);
    assert_eq!(fs::read(&journal_path).unwrap(), before);
    let marker = fixture.marker();
    let output = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!output.status.success());
    assert!(
        text(&output).contains("execution is uncertain"),
        "{}",
        text(&output)
    );
    assert!(!marker.exists());
}

#[test]
fn multiple_saved_operations_require_an_exact_selector() {
    let fixture = Fixture::new("save-return-select");
    let (work, _) = fixture.worker_journal();
    let directory = fixture.root.join(".exitbind/native-actions").join(&work);
    let path = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|part| part.to_str()) == Some("json"))
        .unwrap();
    let original: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let assignment = original["assignment"].as_str().unwrap();
    let mut other = original.clone();
    other["assignment"] = json!("sma_other_saved_operation");
    fs::write(
        directory.join("sma_other_saved_operation.json"),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let ambiguous = fixture.call(&["work", "act", &work, "--resume"], b"");
    assert!(!ambiguous.status.success());
    assert!(text(&ambiguous).contains("--operation"));
    let selected = fixture.call(
        &["work", "act", &work, "--resume", "--operation", assignment],
        b"",
    );
    assert!(selected.status.success(), "{}", text(&selected));
    let conflict = fixture.call(
        &[
            "work",
            "act",
            &work,
            "--resume",
            "--operation",
            assignment,
            "--model",
            "different-model",
        ],
        b"",
    );
    assert!(!conflict.status.success());
    assert!(text(&conflict).contains("changed execution parameters"));
}
