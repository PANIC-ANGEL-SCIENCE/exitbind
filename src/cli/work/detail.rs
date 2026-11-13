// Dispatches complete Work detail and its declared diagnostic view.
// Refuses unknown views; see `src/work/readable.rs`.
//! `work detail`: the complete current detail, or one declared view of it.
use super::*;

pub(super) fn command(l: &config::Loaded, a: &Arguments) -> Result<(), String> {
    args::assert_options("work detail", a, &["config", "json", "view"])?;
    args::assert_positionals("work detail", a, 2)?;
    let work = positional(a, 1, "work detail requires WORK")?;
    let mut value = match a.options.get("view").map(String::as_str) {
        None => crate::work::readable::current(l, work)?,
        Some(view) if crate::work::readable::DETAIL_VIEWS.contains(&view) => {
            crate::work::readable::action_view(l, work)?
        }
        Some(_) => {
            return Err(format!(
                "work detail --view accepts {}; omit it for complete detail",
                crate::work::readable::DETAIL_VIEWS.join(", ")
            ))
        }
    };
    if !a.options.contains_key("view") {
        if let Some(checker) = value
            .pointer("/sections/assignment/checkPreparation")
            .cloned()
        {
            value["beginPreparation"] =
                crate::project::readiness::work_preparation_from_checker(l, checker);
        }
    }
    if a.options.contains_key("view") && !a.flags.contains_key("json") {
        let mut view = crate::work::effective_action::attach(&value);
        crate::work::action_forms::public_permit_guidance(&mut view);
        print!("{}", crate::work::action_text::render(&view, &l.path)?);
        return Ok(());
    }
    if a.options.contains_key("view") {
        print_work_json(&crate::producer::observed(value))
    } else {
        print_step_json(l, &crate::producer::observed(value))
    }
}
