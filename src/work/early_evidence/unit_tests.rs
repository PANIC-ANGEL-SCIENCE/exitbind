use super::*;
use crate::project::layout_types::Mode;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

struct Artifacts {
    root: PathBuf,
    loaded: Loaded,
}

impl Artifacts {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "exitbind-early-evidence-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let loaded = Loaded {
            config: json!({}),
            agents: BTreeMap::new(),
            lead: None,
            architecture_contract: None,
            capabilities: Vec::new(),
            workflows: BTreeMap::new(),
            path: root.join("exitbind.json"),
            control_root: root.clone(),
            product_root: root.clone(),
            state_root: root.clone(),
            mode: Mode::Portable,
            project_id: None,
            source: String::new(),
        };
        Self { root, loaded }
    }

    fn event(&self, name: &str, bytes: &[u8]) -> Value {
        fs::write(self.root.join(name), bytes).unwrap();
        json!({"action":"submit","eventSha256":format!("event-{name}"),
            "subjectSha256":format!("subject-{name}"),
            "artifact":{"root":"state","path":name,"bytes":bytes.len(),
                "sha256":hash::bytes(bytes)}})
    }

    fn missing_event(&self, name: &str) -> Value {
        let bytes = br#"{"summary":"missing candidate"}"#;
        json!({"action":"submit","eventSha256":format!("event-{name}"),
            "subjectSha256":format!("subject-{name}"),
            "artifact":{"root":"state","path":name,"bytes":bytes.len(),
                "sha256":hash::bytes(bytes)}})
    }
}

impl Drop for Artifacts {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn report_bytes() -> Vec<u8> {
    br#"{"earlyEvidence":{"schema":"exitbind-early-evidence-v1"}}"#.to_vec()
}

#[test]
fn newest_missing_candidate_is_unknown_and_never_falls_back_to_older_report() {
    let fixture = Artifacts::new();
    let older = fixture.event("older.json", &report_bytes());
    let newest = fixture.missing_event("missing.json");

    let LatestReport::Unknown { event, reason } =
        latest_report(&fixture.loaded, &[older, newest.clone()]).unwrap()
    else {
        panic!("missing newest candidate must stop report selection")
    };
    assert_eq!(reason, "latest_candidate_artifact_missing");
    let projection = unknown_projection(&event, reason);
    assert_eq!(projection["state"], "unknown");
    assert_eq!(projection["authority"], "none");
    assert_eq!(projection["reportEventSha256"], newest["eventSha256"]);
    assert_eq!(projection["artifactSha256"], newest["artifact"]["sha256"]);
    assert!(projection.get("report").is_none());
}

#[test]
fn changed_candidate_is_unknown_without_reusing_older_report() {
    let fixture = Artifacts::new();
    let older = fixture.event("older.json", &report_bytes());
    let newest_bytes = br#"{"summary":"candidate"}"#;
    let newest = fixture.event("newest.json", newest_bytes);
    fs::write(fixture.root.join("newest.json"), b"tampered").unwrap();

    let LatestReport::Unknown { event, reason } =
        latest_report(&fixture.loaded, &[older, newest]).unwrap()
    else {
        panic!("changed newest candidate must stop report selection")
    };
    assert_eq!(reason, "latest_candidate_artifact_changed");
    assert_eq!(unknown_projection(&event, reason)["state"], "unknown");
}

#[test]
fn verified_complete_non_json_submission_does_not_select_early_evidence() {
    let fixture = Artifacts::new();
    let event = fixture.event("ordinary.md", b"Ordinary result text.\n");
    assert!(matches!(
        latest_report(&fixture.loaded, &[event]).unwrap(),
        LatestReport::None
    ));
}

#[test]
fn truncated_candidate_preview_is_unknown_instead_of_falling_back() {
    let fixture = Artifacts::new();
    let older = fixture.event("older.json", &report_bytes());
    let bytes = vec![b'x'; 65_537];
    let newest = fixture.event("large.json", &bytes);

    let LatestReport::Unknown { event, reason } =
        latest_report(&fixture.loaded, &[older, newest.clone()]).unwrap()
    else {
        panic!("truncated newest candidate must stop report selection")
    };
    assert_eq!(reason, "latest_candidate_artifact_truncated");
    assert_eq!(
        unknown_projection(&event, reason)["reportEventSha256"],
        newest["eventSha256"]
    );
}

#[test]
fn invalid_path_and_symlink_candidates_remain_strict_refusals() {
    let fixture = Artifacts::new();
    let mut bad_path = fixture.event("safe.json", &report_bytes());
    bad_path["artifact"]["path"] = json!("../outside.json");
    assert!(latest_report(&fixture.loaded, &[bad_path])
        .unwrap_err()
        .contains("escapes"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let target = fixture.root.join("target.json");
        fs::write(&target, report_bytes()).unwrap();
        let link = fixture.root.join("link.json");
        symlink(Path::new("target.json"), link).unwrap();
        let bytes = fs::read(&target).unwrap();
        let event = json!({"action":"submit","eventSha256":"event-link",
            "artifact":{"root":"state","path":"link.json","bytes":bytes.len(),
                "sha256":hash::bytes(&bytes)}});
        assert!(latest_report(&fixture.loaded, &[event])
            .unwrap_err()
            .contains("artifact must not be a symlink"));
    }
}
