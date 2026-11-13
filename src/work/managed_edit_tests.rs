// Tests queued managed-file reads and retained baselines.
// Same-file reads share one baseline in these cases; see `src/work/managed_edit.rs`.
//! Queued managed reads serialize baseline commits and effects remain fenced.
use super::*;
use std::{fs, sync::Barrier};

struct Fixture {
    root: crate::test_root::TempRoot,
    loaded: Loaded,
    work: String,
    assignment: String,
    session: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = crate::test_root::temp(label);
        let config = crate::project::onboarding::init_with_options(
            crate::project::onboarding::InitOptions {
                product_root: root.to_str().unwrap(),
                coffee: false,
                skip_skills: true,
                mode: Some("portable"),
                project_id: None,
                control_root: None,
                state_root: None,
            },
        )
        .unwrap();
        let mut settings: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
        settings["agents"]["worker"]["observe"] = json!(["src/**"]);
        settings["agents"]["worker"]["write"] = json!(["src/**"]);
        fs::write(&config, serde_json::to_vec(&settings).unwrap()).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        let loaded = crate::config::load(config.to_str()).unwrap();
        let begun = crate::work::begin(
            &loaded,
            crate::work::BeginOptions {
                workflow: "change",
                goal: "managed concurrent read",
                check_command: "true",
                boundary: None,
                harness_receipt: None,
                proof_origin: None,
                preserve_requirement: None,
                preservation_check_command: None,
                preservation_proof_origin: None,
                basis: crate::work::BasisOrigin::Caller(None),
                review_policy: Some("omitted"),
                auto_goal_context: false,
            },
        )
        .unwrap();
        let work = begun["work"].as_str().unwrap().to_owned();
        let lead = begun["next"]["assignment"].as_str().unwrap();
        crate::work::return_result_impl::return_result_with(
            &loaded,
            &work,
            lead,
            "scoped",
            None,
            None,
            None,
            Some(b"Scoped source work.\n".to_vec()),
            None,
        )
        .unwrap();
        let next = crate::work::next(&loaded, &work).unwrap();
        let assignment = next["next"]["assignment"].as_str().unwrap().to_owned();
        let prepared = prepare(&loaded, &work, &assignment).unwrap();
        let session = prepared["session"].as_str().unwrap().to_owned();
        Self {
            root,
            loaded,
            work,
            assignment,
            session,
        }
    }

    fn read_before_commit(
        &self,
        file: &str,
        before_commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<Value, String> {
        interact_content_before_read_commit(
            &self.loaded,
            "read",
            &self.session,
            file,
            None,
            before_commit,
        )
    }

    fn saved(&self) -> Reads {
        state::load_reads(&self.loaded, &self.session).unwrap().0
    }
}

#[test]
fn queued_distinct_file_reads_merge_without_losing_baselines() {
    let f = Fixture::new("managed-queued-read");
    let names = (0..6).map(|n| format!("src/{n}.txt")).collect::<Vec<_>>();
    for name in &names {
        fs::write(f.root.join(name), name).unwrap();
    }
    let barrier = Barrier::new(names.len());
    std::thread::scope(|scope| {
        let handles = names
            .iter()
            .map(|name| {
                let f = &f;
                let barrier = &barrier;
                scope.spawn(move || {
                    f.read_before_commit(name, || {
                        barrier.wait();
                        Ok(())
                    })
                })
            })
            .collect::<Vec<_>>();
        for (name, handle) in names.iter().zip(handles) {
            let result = handle.join().unwrap().unwrap();
            assert_eq!(result["content"], *name);
        }
    });
    let reads = f.saved();
    assert_eq!(reads.files.len(), names.len());
    for name in names {
        let baseline = &reads.files[&name];
        assert_eq!(baseline.generation, 1);
        assert_eq!(baseline.content.as_deref(), Some(name.as_str()));
    }
}

#[test]
fn queued_same_file_reads_share_one_baseline() {
    let f = Fixture::new("managed-same-read");
    fs::write(f.root.join("src/a"), "first").unwrap();
    let barrier = Barrier::new(4);
    std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| {
                let f = &f;
                let barrier = &barrier;
                scope.spawn(move || {
                    f.read_before_commit("src/a", || {
                        barrier.wait();
                        Ok(())
                    })
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(handle.join().unwrap().unwrap()["content"], "first");
        }
    });
    let reads = f.saved();
    assert_eq!(reads.files.len(), 1);
    assert_eq!(reads.files["src/a"].generation, 1);
}

#[test]
fn source_and_binding_are_checked_at_commit() {
    let f = Fixture::new("managed-read-source-change");
    fs::write(f.root.join("src/a"), "before").unwrap();
    let value = f
        .read_before_commit("src/a", || {
            fs::write(f.root.join("src/a"), "after").map_err(|e| e.to_string())
        })
        .unwrap();
    assert_eq!(value["content"], "after");
    assert_eq!(f.saved().files["src/a"].content.as_deref(), Some("after"));

    fs::write(f.root.join("src/b"), "safe").unwrap();
    let error = f
        .read_before_commit("src/b", || {
            fs::remove_file(f.root.join("src/b")).map_err(|e| e.to_string())?;
            std::os::unix::fs::symlink("a", f.root.join("src/b")).map_err(|e| e.to_string())
        })
        .unwrap_err();
    assert!(error.contains("unsafe"), "{error}");
    assert!(!f.saved().files.contains_key("src/b"));

    fs::write(f.root.join("src/c"), "private").unwrap();
    let error = f
        .read_before_commit("src/c", || {
            let mut source = fs::read(&f.loaded.path).map_err(|e| e.to_string())?;
            source.push(b' ');
            fs::write(&f.loaded.path, source).map_err(|e| e.to_string())
        })
        .unwrap_err();
    assert!(error.contains("binding changed"), "{error}");
    assert!(!f.saved().files.contains_key("src/c"));
}

#[test]
fn work_transition_before_commit_refuses_delivery_and_baseline() {
    let f = Fixture::new("managed-read-transition");
    fs::write(f.root.join("src/a"), "must not be delivered").unwrap();
    let error = f
        .read_before_commit("src/a", || {
            crate::work::return_result_impl::return_result_with(
                &f.loaded,
                &f.work,
                &f.assignment,
                "completed",
                None,
                None,
                None,
                Some(b"worker returned\n".to_vec()),
                None,
            )
            .map(|_| ())
        })
        .unwrap_err();
    assert!(error.contains("current worker assignment"), "{error}");
    assert!(f.saved().files.is_empty());
}

#[test]
fn active_effect_lock_stays_exclusive_and_is_not_removed_by_waiting_read() {
    let f = Fixture::new("managed-read-active-lock");
    fs::write(f.root.join("src/a"), "original").unwrap();
    let ledger = crate::work::resolve(&f.loaded, &f.work).unwrap();
    let lock = run::ledger::ledger_path(&f.loaded.state_root, &ledger, false).unwrap();
    let original_reads = fs::read(
        f.loaded
            .state_root
            .join(state::relative(&f.session, "reads.json").unwrap()),
    )
    .unwrap();
    run::ledger::with_lock(&lock, || {
        let owned = fs::read(&lock.lock).unwrap();
        let edit = interact_content(&f.loaded, "edit", &f.session, "src/a", Some("replacement"))
            .unwrap_err();
        assert!(edit.contains("run ledger is busy"), "{edit}");
        let read = interact_content(&f.loaded, "read", &f.session, "src/a", None).unwrap_err();
        assert!(read.contains("run ledger is busy"), "{read}");
        assert_eq!(fs::read(&lock.lock).unwrap(), owned);
        assert_eq!(fs::read(f.root.join("src/a")).unwrap(), b"original");
        assert_eq!(
            fs::read(
                f.loaded
                    .state_root
                    .join(state::relative(&f.session, "reads.json").unwrap())
            )
            .unwrap(),
            original_reads
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn edit_before_commit_keeps_the_original_baseline() {
    let f = Fixture::new("managed-read-concurrent-edit");
    fs::write(f.root.join("src/a"), "original").unwrap();
    assert_eq!(
        f.read_before_commit("src/a", || Ok(())).unwrap()["content"],
        "original"
    );
    crate::work::permit_response::permit(
        &f.loaded,
        &f.work,
        &f.assignment,
        "Replace the file",
        Some("concurrent-edit"),
    )
    .unwrap();
    let result = f
        .read_before_commit("src/a", || {
            interact_content(&f.loaded, "edit", &f.session, "src/a", Some("replacement"))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(result["content"], "original");
    assert_eq!(fs::read(f.root.join("src/a")).unwrap(), b"replacement");
    let inspection = interact_content(&f.loaded, "inspect", &f.session, "src/a", None).unwrap();
    assert_eq!(inspection["effectStatus"], "completed");
    assert_eq!(
        f.saved().files["src/a"].content.as_deref(),
        Some("original")
    );
}
