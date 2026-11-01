//! `work child`: prepare, read, recover and record a native child of a Work.
use super::*;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    match a.positional.get(1).map(String::as_str) {
        Some("prepare") => {
            args::assert_options(
                "work child prepare",
                a,
                &[
                    "config",
                    "context",
                    "agent-type",
                    "perspectives",
                    "replace",
                    "current-binding",
                ],
            )?;
            args::assert_positionals("work child prepare", a, 4)?;
            let work = positional(a, 2, "work child prepare requires WORK SHORT_ASSIGNMENT")?;
            let assignment = positional(a, 3, "work child prepare requires WORK SHORT_ASSIGNMENT")?;
            // Without --context: the native session that holds the Work.
            let fence = a.options.contains_key("context");
            let fence = fence.then(|| context_fence(a, work)).transpose()?;
            print_work_json(&crate::session_goal::prepare_child(
                l,
                work,
                crate::session_goal::PrepareFence {
                    continuation: fence,
                    action: a.options.get("current-binding").map(String::as_str),
                },
                assignment,
                a.options.get("agent-type").map(String::as_str),
                a.options.get("perspectives").map(String::as_str),
                a.flags.contains_key("replace"),
            )?)
        }
        Some("context") => {
            args::assert_options("work child context", a, &["config", "view", "index"])?;
            let work = positional(a, 2, "work child context requires WORK INTENT")?;
            match a.options.get("view").map(String::as_str) {
                Some("results") => {
                    args::assert_positionals("work child context --view results", a, 3)?;
                    let index = a.options.get("index").map_or(Ok(0), |value| {
                        value
                            .parse::<usize>()
                            .map_err(|_| "raw observation index must be an integer".to_owned())
                    })?;
                    print_work_json(&crate::session_goal::raw_child_results(l, work, index)?)
                }
                view @ (None | Some("result")) => {
                    args::assert_positionals("work child context", a, 4)?;
                    if a.options.contains_key("index") {
                        return Err("--index requires --view results".into());
                    }
                    let intent = positional(a, 3, "work child context requires WORK INTENT")?;
                    if view.is_some() {
                        print_work_json(&crate::session_goal::raw_child_result(l, work, intent)?)
                    } else {
                        print_work_json(&crate::session_goal::child_context(l, work, intent)?)
                    }
                }
                Some(_) => Err("work child context --view accepts result or results".into()),
            }
        }
        Some("recover") => {
            args::assert_options("work child recover", a, &["config", "context"])?;
            args::assert_positionals("work child recover", a, 4)?;
            let work = positional(a, 2, "work child recover requires WORK INTENT")?;
            let intent = positional(a, 3, "work child recover requires WORK INTENT")?;
            // Without --context: the native session that holds the Work.
            let fence = a.options.contains_key("context");
            let fence = fence.then(|| context_fence(a, work)).transpose()?;
            print_work_json(&crate::session_goal::recover_child(l, work, intent, fence)?)
        }
        _ => {
            args::assert_options(
                "work child",
                a,
                &["config", "context", "native-child", "result"],
            )?;
            args::assert_positionals("work child", a, 3)?;
            let work = positional(a, 1, "work child requires WORK ASSIGNMENT")?;
            let assignment = positional(a, 2, "work child requires WORK ASSIGNMENT")?;
            let assignment = bounded_cli(assignment, "assignment", 128)?;
            let native_child = bounded_cli(
                option(a, "native-child", "work child requires --native-child ID")?,
                "native child",
                256,
            )?;
            let result = child_result(a)?;
            let (expected_revision, binding_revision) = context_fence(a, work)?;
            print_work_json(&crate::session_goal::continuation_child(
                l,
                work,
                expected_revision,
                binding_revision,
                assignment,
                native_child,
                &result,
            )?)
        }
    }
}
