//! R55 F11 (P5 surfaces): every `--view` value a command accepts is read
//! from its own dispatch code (`Some("x")` arms, a declared list it checks,
//! a `view != "x"` test), and its refusal names exactly those values. A view
//! arm added without its refusal text fails here; the recipient inventory
//! (`agent_context_delivery::action_view`) covers the accepted set.
use super::cli_tables::{views_in, Views};
use super::rust_source::production;

fn mismatched(views: &[Views]) -> Vec<String> {
    views
        .iter()
        .filter(|view| view.accepted.is_empty() || view.accepted != view.refused)
        .map(|view| {
            format!(
                "{}: fn {} accepts --view {:?} but its refusal names {:?}",
                view.file, view.function, view.accepted, view.refused
            )
        })
        .collect()
}

#[test]
fn every_view_a_command_accepts_is_named_by_its_refusal() {
    let sources = production();
    let cli: Vec<(String, String)> = sources
        .iter()
        .filter(|(path, _)| path.starts_with("src/cli/"))
        .cloned()
        .collect();
    let views = views_in(&sources, &cli);
    let readers: Vec<String> = views
        .iter()
        .map(|view| format!("{}::{}", view.file, view.function))
        .collect();
    assert!(
        readers
            .iter()
            .any(|reader| reader == "src/cli/work/detail.rs::command"),
        "{readers:?}"
    );
    assert!(views.len() >= 3, "{readers:?}");
    let failures = mismatched(&views);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Seeded: a view arm without its refusal text, a declared list joined into
/// the refusal, and a `view != "x"` test each read correctly.
#[test]
fn a_view_arm_without_its_refusal_text_is_a_member() {
    let lists = vec![(
        "src/work/readable.rs".to_owned(),
        "pub(crate) const DETAIL_VIEWS: &[&str] = &[\"action\"];\n".to_owned(),
    )];
    let detail = concat!(
        "pub(super) fn command(l: &Loaded, a: &Arguments) -> Result<(), String> {\n",
        "    let value = match a.options.get(\"view\").map(String::as_str) {\n",
        "        None => full(l)?,\n",
        "        Some(\"compact\") => compact(l)?,\n",
        "        Some(view) if crate::work::readable::DETAIL_VIEWS.contains(&view) => action(l)?,\n",
        "        Some(_) => {\n",
        "            return Err(format!(\n",
        "                \"work detail --view accepts {}; omit it for complete detail\",\n",
        "                crate::work::readable::DETAIL_VIEWS.join(\", \")\n",
        "            ))\n",
        "        }\n",
        "    };\n",
        "    Ok(())\n",
        "}\n",
        "fn status(a: &Arguments) -> Result<(), String> {\n",
        "    if a.options.get(\"view\").is_some_and(|view| view != \"event\") {\n",
        "        return Err(\"goal status --view accepts only event\".into());\n",
        "    }\n",
        "    Ok(())\n",
        "}\n",
    );
    let files = vec![("src/cli/seeded.rs".to_owned(), detail.to_owned())];
    let mut sources = lists;
    sources.extend(files.clone());
    let views = views_in(&sources, &files);
    let failures = mismatched(&views);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].contains("fn command accepts --view {\"action\", \"compact\"}")
            && failures[0].contains("refusal names {\"action\"}"),
        "{failures:?}"
    );
    let fixed = detail.replace("        Some(\"compact\") => compact(l)?,\n", "");
    let files = vec![("src/cli/seeded.rs".to_owned(), fixed)];
    let views = views_in(&sources, &files);
    assert!(mismatched(&views).is_empty());
    assert_eq!(views.len(), 2);
}
