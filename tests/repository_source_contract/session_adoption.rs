//! Source contract for native Goal adoption route renderers.
use super::rust_source::{blank, code_words, literals, production};

/// Calls of `adoption::NAME` in code, in any spacing (`adoption :: NAME (`).
fn adoption_calls(source: &str, name: &str) -> usize {
    let code = blank(source);
    code_words(source, name)
        .into_iter()
        .filter(|at| {
            let before: String = code[..*at].chars().filter(|c| !c.is_whitespace()).collect();
            before.ends_with("adoption::") && code[at + name.len()..].trim_start().starts_with('(')
        })
        .count()
}

/// Every renderer of the native UNAVAILABLE reasons, read from `src/`: the
/// reason codes are spelled once, in the route builder, and every reason
/// value is passed to that builder, which attaches the route.
#[test]
fn every_native_unavailable_renderer_goes_through_the_adoption_route_builder() {
    const BUILDER: &str = "src/session_goal/adoption.rs";
    let sources = production();
    assert!(sources.iter().any(|(path, _)| path == BUILDER));
    let mut renderers = Vec::new();
    for (relative, source) in &sources {
        let relative = relative.as_str();
        for code in [
            "native_session_goal_unavailable",
            "native_session_goal_context_invalid",
            "native Goal owner record is absent",
        ] {
            assert!(
                relative == BUILDER || !literals(source).iter().any(|lit| lit.text.contains(code)),
                "{relative} renders {code} outside the adoption route builder"
            );
        }
        if relative == BUILDER {
            continue;
        }
        let calls = adoption_calls(source, "unavailable");
        let reasons = code_words(source, "Unavailable")
            .into_iter()
            .chain(code_words(source, "ContextInvalid"))
            .filter(|at| blank(source)[..*at].trim_end().ends_with("Reason::"))
            .count();
        assert_eq!(
            calls, reasons,
            "{relative}: every native reason is passed to adoption::unavailable"
        );
        for _ in 0..calls {
            renderers.push(relative.to_owned());
        }
        if adoption_calls(source, "hook_text") > 0 {
            renderers.push(relative.to_owned());
        }
    }
    renderers.sort();
    assert_eq!(
        renderers,
        [
            "src/host/session_recovery/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
            "src/session_goal/presentation.rs",
        ],
        "a new renderer joins this inventory"
    );
    let builder = &sources.iter().find(|(path, _)| path == BUILDER).unwrap().1;
    let body = builder
        .split("pub(crate) fn unavailable(")
        .nth(1)
        .unwrap()
        .split("\n}\n")
        .next()
        .unwrap();
    assert!(body.contains("route(loaded"), "{body}");
    assert!(body.contains("\"adoptRoute\""), "{body}");
    let hook = builder.split("pub(crate) fn hook_text(").nth(1).unwrap();
    assert!(hook.split("\n}\n").next().unwrap().contains("route.line"));
}
