//! Bounded SessionStart text for the Works derived from the native Goal's
//! journey and the checkout trees that hold uncommitted changes.
//!
//! Each Work line degrades before it is dropped: open assignments become a
//! count, then the next action is left out. The first Work's locator and its
//! `work detail` command are always kept, so a long configuration path or
//! many assignments never lose the Work name.

use super::{checkout, current_work};

/// Room kept after the shown Works for the count of Works not shown.
const MORE_RESERVE: usize = 96;

pub(super) fn bounded_hook_text(value: &str, max_bytes: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim();
    if sanitized.len() <= max_bytes {
        return sanitized.to_owned();
    }
    let mut end = max_bytes.saturating_sub('…'.len_utf8());
    while !sanitized.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &sanitized[..end])
}

/// The forms of one Work line, from complete to the locator and command only.
fn work_line_forms(
    item: &current_work::CurrentWork,
    goal: &str,
    index: Option<usize>,
    executable: &str,
) -> [String; 3] {
    let core = format!(
        "{}current Work {} ({} of Goal {goal}); read: {executable} work detail {} --view action --config {}",
        index.map(|value| format!("{value}) ")).unwrap_or_default(),
        item.work,
        if item.source == "journey_member" { "member" } else { "mapped Work" },
        item.work,
        item.config_path,
    );
    let next = format!(
        "{core}; next action: {}.",
        bounded_hook_text(
            item.next_action
                .as_deref()
                .unwrap_or("read the Work detail"),
            192
        ),
    );
    let assignments = item
        .assignments
        .iter()
        .map(|value| {
            format!(
                "{} ({} '{}', open)",
                value.id,
                bounded_hook_text(&value.role, 32),
                bounded_hook_text(&value.agent, 64)
            )
        })
        .collect::<Vec<_>>();
    match assignments.len() {
        0 => [next.clone(), next, format!("{core}.")],
        count => [
            format!("{next} Open assignments: {}.", assignments.join(", ")),
            format!(
                "{next} {count} open assignment{}.",
                if count == 1 { "" } else { "s" }
            ),
            format!("{core}."),
        ],
    }
}

/// One line per derived Work in the `work detail` form the agent can run as
/// printed, then the checkout trees that hold uncommitted changes. Every
/// command names `executable`, the one the hook runs as. Only the first
/// Work's minimal form may exceed `budget`; the caller shortens its own text
/// around it.
pub(super) fn derived_hook_text(
    derived: &current_work::Derived,
    trees: &checkout::Trees,
    budget: usize,
    executable: &str,
) -> String {
    let goal = bounded_hook_text(&derived.goal_id, 128);
    let total = derived.works.len() + derived.more;
    let mut out = String::new();
    let mut shown = 0;
    for item in &derived.works {
        let forms = work_line_forms(item, &goal, (total > 1).then_some(shown + 1), executable);
        let separator = usize::from(shown > 0);
        let reserve = if total > shown + 1 { MORE_RESERVE } else { 0 };
        let room = budget.saturating_sub(out.len() + separator + reserve);
        let line = match forms.iter().find(|line| line.len() <= room) {
            Some(line) => line.clone(),
            None if shown == 0 => forms[2].clone(),
            None => break,
        };
        if shown > 0 {
            out.push(' ');
        }
        out.push_str(&line);
        shown += 1;
    }
    if total > shown {
        out.push_str(&format!(
            " {} more open Work(s): `{executable} session recovery show --json`.",
            total - shown
        ));
    }
    if !trees.dirty.is_empty() {
        let mut listed = Vec::new();
        let mut used = out.len() + "\nUncommitted checkout trees: .".len();
        for tree in &trees.dirty {
            let at = match (&tree.branch, &tree.head) {
                (Some(branch), _) => format!("branch {}", bounded_hook_text(branch, 96)),
                (None, Some(head)) => format!("detached {head}"),
                (None, None) => "no commit".to_owned(),
            };
            let item = format!(
                "{} ({at}, {} changed)",
                bounded_hook_text(&tree.path, 256),
                tree.changed
            );
            if used + item.len() + 2 + 16 > budget {
                break;
            }
            used += item.len() + 2;
            listed.push(item);
        }
        let omitted = trees.dirty.len() - listed.len() + trees.more;
        if !listed.is_empty() || (omitted > 0 && used + 16 <= budget) {
            out.push_str("\nUncommitted checkout trees: ");
            out.push_str(&listed.join("; "));
            if omitted > 0 {
                out.push_str(&format!(
                    "{}{omitted} more",
                    if listed.is_empty() { "" } else { "; " }
                ));
            }
            out.push('.');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/opt/r55/bin/exitbind-candidate";

    fn work(index: usize, config_path: &str, assignments: usize) -> current_work::CurrentWork {
        current_work::CurrentWork {
            work: format!("smw_{}", format!("{index}").repeat(64)),
            config_path: config_path.into(),
            product_root: "/tmp/project".into(),
            source: "journey_member",
            next_action: Some("lead_decision".into()),
            assignments: (0..assignments)
                .map(|slot| current_work::Assignment {
                    id: format!("sma_{}", format!("{slot}").repeat(64)),
                    agent: "worker".into(),
                    role: "worker".into(),
                })
                .collect(),
        }
    }

    fn trees() -> checkout::Trees {
        checkout::Trees {
            dirty: vec![checkout::Tree {
                path: "/tmp/project".into(),
                branch: Some("feature/x".into()),
                head: None,
                changed: 3,
            }],
            more: 0,
            complete: true,
        }
    }

    #[test]
    fn several_derived_works_are_listed_up_to_the_limit_with_a_count_of_the_rest() {
        let derived = current_work::Derived {
            goal_id: "R54".into(),
            works: (1..=4)
                .map(|index| work(index, "/tmp/project/exitbind.json", 1))
                .collect(),
            more: 2,
            unverified: 0,
            incomplete: false,
        };
        let text = derived_hook_text(&derived, &trees(), 2048, EXE);
        assert!(text.len() <= 2048, "{text}");
        assert!(
            text.contains(&format!("1) current Work smw_{}", "1".repeat(64))),
            "{text}"
        );
        assert!(text.contains("more open Work(s)"), "{text}");
        assert!(
            text.contains("/tmp/project (branch feature/x, 3 changed)"),
            "{text}"
        );
        let listed = (1..=4)
            .filter(|index| text.contains(&format!("{index}) current Work")))
            .count();
        assert!((1..=4).contains(&listed), "{text}");
        let shown_more = 6 - listed;
        assert!(
            text.contains(&format!(" {shown_more} more open Work(s)")),
            "{text}"
        );

        let small = derived_hook_text(&derived, &trees(), 400, EXE);
        assert!(
            small.contains("1) current Work"),
            "the first Work is always named"
        );
        assert!(small.contains(" 5 more open Work(s)"), "{small}");
    }

    /// W1 review finding 3: a long configuration path and eight assignments
    /// must not push the first Work's locator and command out of the text.
    #[test]
    fn the_first_work_keeps_its_locator_and_command_within_the_budget() {
        let config = format!("/tmp/{}/exitbind.json", "deep/".repeat(160));
        let derived = current_work::Derived {
            goal_id: "R54".into(),
            works: vec![work(7, &config, 8)],
            more: 0,
            unverified: 3,
            incomplete: false,
        };
        let command = format!(
            "{EXE} work detail smw_{} --view action --config {config}",
            "7".repeat(64)
        );
        for budget in [2048, 1400, 1000] {
            let text = derived_hook_text(&derived, &trees(), budget, EXE);
            assert!(text.contains(&command), "{budget}: {text}");
            assert!(
                text.len() <= budget.max(command.len() + 128),
                "{budget}: {text}"
            );
            assert!(!text.contains("could not be verified"), "{text}");
        }
        let full = derived_hook_text(&derived, &trees(), 8192, EXE);
        assert!(full.contains("Open assignments: sma_"), "{full}");
        let counted = derived_hook_text(&derived, &trees(), 1400, EXE);
        assert!(
            counted.contains("8 open assignments.") || counted.ends_with(&format!("{config}.")),
            "{counted}"
        );
    }
}
