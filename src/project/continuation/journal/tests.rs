use super::*;
use crate::{evidence::hash, project::onboarding};
use serde_json::{json, Value};
use std::fs;

struct Fixture {
    _root: crate::test_root::TempRoot,
    loaded: Loaded,
    relative: String,
    record: Record,
}

impl Fixture {
    fn new() -> Self {
        let root = crate::test_root::temp("journal-test");
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(status.success());
        let configuration = onboarding::init_with_options(onboarding::InitOptions {
            product_root: root.to_str().unwrap(),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        let source_bytes = b"# Direction\nPreserve journal history.\n";
        fs::write(root.join("docs/brief.md"), source_bytes).unwrap();
        let loaded = crate::config::load(Some(configuration.to_str().unwrap())).unwrap();
        let specification: Spec = serde_json::from_value(json!({
            "version":1,"projectId":"example","mandate":{
                "id":"advance","revision":1,"outcome":"Preserve journal history.",
                "mode":"continue","allowedSources":["brief"],"write":["src"],
                "commands":["true"],"roles":["worker"],
                "preserve":["Keep existing journal records."],
                "priorityPolicy":"Use a bounded fixture.","expiresAt":"2099-01-01T00:00:00Z",
                "maxWorks":2,"maxActions":12,"maxParallel":1,"maxElapsedSeconds":3600,
                "externalPublication":false
            },"sources":[{
                "id":"brief","kind":"export","repository":null,"revision":"export-1",
                "path":"docs/brief.md","section":"Direction",
                "sha256":hash::bytes(source_bytes),"acquiredAt":"2026-01-01T00:00:00Z",
                "currentness":"declared_export","trust":"ownerDirection"
            }]
        }))
        .unwrap();
        specification.validate().unwrap();
        let project = crate::session_goal::requirements::identity(&loaded).unwrap();
        let spec_digest = "a".repeat(64);
        let work = format!("smw_{}", "b".repeat(64));
        let task_id = "preserve-records".to_owned();
        let association = super::super::control::Association {
            version: 1,
            project: project.clone(),
            mandate_id: specification.mandate.id.clone(),
            mandate_revision: specification.mandate.revision,
            specification_sha256: spec_digest.clone(),
            task_id: task_id.clone(),
            claim_root: work.clone(),
        };
        let plan: Plan = serde_json::from_value(json!({
            "version":1,"id":"first-plan","taskId":task_id,"title":"Preserve records",
            "rationale":"Exercise a real snapshot chain.","outcome":"Preserve journal history.",
            "acceptance":["Read every appended revision."],
            "preservation":["Keep existing journal records."],"sourceIds":["brief"],
            "dependencies":[],"workflow":"change","write":["src"],"checkCommand":"true",
            "preservationCheckCommand":"true","estimatedActions":2,
            "overlapReason":"This isolated fixture owns the record.","goalId":"fixture-goal",
            "requirements":["records"],"checker":"checks.sh","integration":false
        }))
        .unwrap();
        plan.validate(&specification).unwrap();
        let policy = super::super::control::Policy {
            project: &project,
            specification_sha256: &spec_digest,
            mandate: &specification.mandate,
        };
        let selected_at = "2026-01-01T00:00:00Z".to_owned();
        let mut account = super::super::control::Account::new(&policy, 1_767_225_600).unwrap();
        account.revision = 2;
        account.operations.push(super::super::control::Operation {
            key: "admission-start".into(),
            intent: super::super::control::Intent {
                kind: super::super::control::Kind::Admission,
                work: work.clone(),
                role: super::super::control::Role::Worker,
                paths: vec![],
                commands: vec![],
                input_sha256: "c".repeat(64),
                predecessor: None,
            },
            association: association.clone(),
            outcome: super::super::control::Outcome::Failed,
        });
        account.operations.push(super::super::control::Operation {
            key: "verification-check".into(),
            intent: super::super::control::Intent {
                kind: super::super::control::Kind::Check,
                work: format!("smw_{}", "b".repeat(64)),
                role: super::super::control::Role::Worker,
                paths: vec![],
                commands: vec![],
                input_sha256: "f".repeat(64),
                predecessor: None,
            },
            association: association.clone(),
            outcome: super::super::control::Outcome::Succeeded,
        });
        super::super::control::validate_account(&account).unwrap();
        let claim = Claim {
            task_id: task_id.clone(),
            plan_sha256: super::super::admission::plan_digest(&plan).unwrap(),
            plan,
            work,
            start_intent_sha256: "d".repeat(64),
            checker_sha256: "e".repeat(64),
            admitted_at: selected_at.clone(),
            association,
        };
        let relative = relative(&loaded, &specification.mandate.id).unwrap();
        let record = Record {
            version: 2,
            revision: 0,
            project_identity: project,
            configuration_sha256: hash::text(&loaded.source),
            specification_path: "spec.json".into(),
            specification_sha256: spec_digest,
            specification: specification.clone(),
            selected_at,
            reason: "initial selection".into(),
            stopped: false,
            claims: vec![claim],
            account,
            previous_event_sha256: None,
            event_sha256: String::new(),
        };
        Self {
            _root: root,
            loaded,
            relative,
            record,
        }
    }

    fn grow_past_old_limit(&self) -> (ledger::LedgerPath, String, Record) {
        let selected = prepare(&self.loaded, &self.relative).unwrap();
        let mut previous: Option<Record> = None;
        let mut raw = String::new();
        for revision in 0..270 {
            let mut next = self.record.clone();
            next.reason = format!("r{revision:03}{}", "x".repeat(4_092));
            assert_eq!(next.reason.len(), 4_096);
            let written = append(&selected, next, previous.as_ref(), &raw).unwrap();
            raw.push_str(&serde_json::to_string(&serde_json::to_value(&written).unwrap()).unwrap());
            raw.push('\n');
            previous = Some(written);
        }
        assert!(raw.len() > 1024 * 1024);
        (selected, raw, previous.unwrap())
    }
}

#[test]
fn real_append_and_secure_read_preserve_large_valid_history() {
    let fixture = Fixture::new();
    let (_selected, raw, last) = fixture.grow_past_old_limit();
    let (read_last, read_raw) = read(&fixture.loaded, &fixture.relative).unwrap();
    let read_last = read_last.unwrap();
    assert_eq!(raw, read_raw);
    assert!(raw.len() > 1024 * 1024);
    assert_eq!(read_last.revision, 270);
    assert_eq!(read_last.selected_at, "2026-01-01T00:00:00Z");
    assert_eq!(read_last.claims, fixture.record.claims);
    assert_eq!(
        read_last.account.started_at,
        fixture.record.account.started_at
    );
    assert_eq!(
        read_last.account.operations,
        fixture.record.account.operations
    );
    assert_eq!(read_last.event_sha256, last.event_sha256);
    assert_eq!(
        read_last.account.operations[0].outcome,
        super::super::control::Outcome::Failed
    );
}

#[test]
fn aggregate_capacity_refuses_overflow_before_changing_the_file() {
    let fixture = Fixture::new();
    let selected = prepare(&fixture.loaded, &fixture.relative).unwrap();
    let first = append(&selected, fixture.record.clone(), None, "").unwrap();
    let raw = format!(
        "{}\n",
        serde_json::to_string(&serde_json::to_value(&first).unwrap()).unwrap()
    );
    let before = fs::read(&selected.path).unwrap();
    let over_bound = format!("{}{}", raw, "r".repeat(LIMIT as usize - raw.len()));
    let error = append(&selected, fixture.record.clone(), Some(&first), &over_bound).unwrap_err();
    assert!(error.contains("metadata bound reached"));
    assert_eq!(fs::read(&selected.path).unwrap(), before);
    let limit = LIMIT as usize;
    assert!(!exceeds_limit(limit - 1, 1));
    assert!(exceeds_limit(limit, 1));
    assert!(exceeds_limit(usize::MAX, 1));
}

#[test]
fn secure_reader_rejects_files_over_the_journal_limit() {
    let fixture = Fixture::new();
    let selected = prepare(&fixture.loaded, &fixture.relative).unwrap();
    fs::write(selected.path, vec![b'x'; LIMIT as usize + 1]).unwrap();
    assert!(read(&fixture.loaded, &fixture.relative).is_err());
}

#[test]
fn malformed_tampered_and_incomplete_chains_are_refused() {
    let fixture = Fixture::new();
    let (selected, valid, _) = fixture.grow_past_old_limit();

    fs::write(&selected.path, b"{bad json}\n").unwrap();
    assert!(read(&fixture.loaded, &fixture.relative)
        .unwrap_err()
        .contains("malformed"));

    let mut lines: Vec<String> = valid.lines().map(str::to_owned).collect();
    let mut value: Value = serde_json::from_str(lines[0].as_str()).unwrap();
    value["reason"] = json!("tampered without updating the chain hash");
    lines[0] = serde_json::to_string(&value).unwrap();
    fs::write(&selected.path, format!("{}\n", lines.join("\n"))).unwrap();
    assert!(read(&fixture.loaded, &fixture.relative)
        .unwrap_err()
        .contains("chain is invalid"));

    fs::write(&selected.path, valid.trim_end()).unwrap();
    assert!(read(&fixture.loaded, &fixture.relative)
        .unwrap_err()
        .contains("incomplete last record"));
}
