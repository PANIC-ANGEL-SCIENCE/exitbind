//! The focus transaction serializes real writers without adding a ledger.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (crate::test_root::TempRoot, Loaded) {
    let root = crate::test_root::temp("native-successor-focus-transaction");
    let config =
        crate::project::onboarding::init_with_options(crate::project::onboarding::InitOptions {
            product_root: root.to_str().unwrap(),
            coffee: false,
            skip_skills: true,
            mode: Some("portable"),
            project_id: None,
            control_root: None,
            state_root: None,
        })
        .unwrap();
    let loaded = crate::config::load(config.to_str()).unwrap();
    fs::create_dir_all(
        loaded
            .state_root
            .join(crate::project::layout_types::state_namespace()),
    )
    .unwrap();
    (root, loaded)
}

#[test]
fn transaction_fences_a_focus_writer_then_preserves_its_later_selection() {
    let (_root, loaded) = fixture();
    let predecessor = format!("smw_{}", "1".repeat(64));
    let successor_work = format!("smw_{}", "2".repeat(64));
    let other = format!("smw_{}", "3".repeat(64));
    write(&loaded, &predecessor).unwrap();
    let (ready_send, ready_receive) = mpsc::channel();
    let (finish_send, finish_receive) = mpsc::channel();
    let (outcome, ready, competing) = std::thread::scope(|scope| {
        let loaded = &loaded;
        let predecessor = &predecessor;
        let successor_work = &successor_work;
        let transaction = scope.spawn(move || {
            successor(loaded, predecessor, successor_work, || {
                ready_send.send(()).map_err(|error| error.to_string())?;
                finish_receive.recv().map_err(|error| error.to_string())?;
                Ok(((), SuccessorState::FreshLinked))
            })
        });
        let ready = ready_receive.recv_timeout(Duration::from_secs(5));
        let competing = ready.is_ok().then(|| write(loaded, &other));
        let _ = finish_send.send(());
        (transaction.join().unwrap(), ready, competing)
    });
    assert!(ready.is_ok(), "the transaction did not reach its barrier");
    assert!(
        competing.unwrap().is_err(),
        "a focus writer entered the transaction"
    );
    let (_, transitioned) = outcome.unwrap();
    assert_eq!(transitioned["current"], true);
    assert_eq!(transitioned["updated"], true);
    write(&loaded, &other).unwrap();
    let (_, replay) = successor(&loaded, &predecessor, &successor_work, || {
        Ok(((), SuccessorState::Recovery))
    })
    .unwrap();
    assert_eq!(replay["current"], false);
    assert!(matches!(read(&loaded).unwrap(), Focus::Work(current) if current == other));
}

#[test]
fn unavailable_focus_lock_refuses_before_work_and_offers_detail() {
    let (root, loaded) = fixture();
    let predecessor = format!("smw_{}", "4".repeat(64));
    let successor_work = format!("smw_{}", "5".repeat(64));
    write(&loaded, &predecessor).unwrap();
    let lock = root.join(".exitbind/current-work-focus.lock");
    fs::create_dir(&lock).unwrap();
    let mut calls = 0;
    let error = successor(&loaded, &predecessor, &successor_work, || {
        calls += 1;
        Ok(("started", SuccessorState::FreshLinked))
    })
    .unwrap_err();
    assert_eq!(calls, 0);
    assert!(
        error.contains("no native supersession was started"),
        "{error}"
    );
    assert!(
        error.contains(&format!("work detail {successor_work} --json")),
        "{error}"
    );
    fs::remove_dir(lock).unwrap();
    assert!(matches!(read(&loaded).unwrap(), Focus::Work(current) if current == predecessor));
}

#[test]
fn failed_link_never_moves_predecessor_focus() {
    let (_root, loaded) = fixture();
    let predecessor = format!("smw_{}", "6".repeat(64));
    let successor_work = format!("smw_{}", "7".repeat(64));
    write(&loaded, &predecessor).unwrap();
    let (_, focus) = successor(&loaded, &predecessor, &successor_work, || {
        Ok(((), SuccessorState::LinkPending))
    })
    .unwrap();
    assert_eq!(focus["current"], false);
    assert!(focus["error"].as_str().unwrap().contains("link is pending"));
    assert_eq!(focus["command"][3], successor_work);
    assert!(matches!(read(&loaded).unwrap(), Focus::Work(current) if current == predecessor));
}
