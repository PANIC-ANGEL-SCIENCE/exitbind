//! Emitted actions after a real linked retry ends without passing.
use super::*;

#[test]
fn failed_linked_retry_offers_executable_inspection_and_blocked_report_only() {
    let f = Fixture::new("failed-retry-guidance", false);
    let preview = f.release();
    successful(f.apply(&approved(&preview)));
    fs::remove_file(f.root.join(".exitbind/allow-check")).unwrap();
    let failed = f.call(
        &[
            "work",
            "return",
            &f.work,
            &f.assignment,
            "--outcome",
            "completed",
            "--result-ref",
            &f.held,
        ],
        None,
    );
    assert!(!failed.status.success(), "{failed:?}");
    assert_eq!(f.count(), b"xx");
    let detail = f.detail();
    let forms = &detail["actionForms"];
    assert_eq!(forms["state"], "pre_return_retry_failed", "{detail}");
    let choices = forms["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 1, "{forms}");
    assert_eq!(choices[0]["label"], "blocked");
    let before = f.ledger();
    let inspected = rendered(&f.root, &forms["diagnostic"]["command"], None);
    assert!(inspected.status.success(), "{inspected:?}");
    assert_eq!(f.ledger(), before);
    assert_eq!(f.count(), b"xx");
    let mut route = choices[0]["command"].clone();
    for word in route["argv"].as_array_mut().unwrap() {
        if *word == "<REASON>" {
            *word = json!("The one linked retry ended without passing.");
        }
    }
    let reported = rendered(
        &f.root,
        &route,
        Some(b"Truthful blocked report; no third checker execution."),
    );
    assert!(reported.status.success(), "{reported:?}");
    assert_eq!(f.count(), b"xx");
}
