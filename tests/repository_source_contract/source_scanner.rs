//! R55 F11: the shared source scanner (`tests/inventory/rust_source.rs`)
//! every inventory reads `src/` with. Seeded: a test module that comes before
//! production code and holds brace literals is cut by its own extent; every
//! literal spelling is one literal; dispatcher arms and declared lists read
//! code only.
use super::rust_source::{
    gated_items, literals, match_arms, production_in, string_list, strip_test_modules, Gate,
};
use super::*;

const SEEDED: &str = concat!(
    "#[cfg(test)]\n",
    "mod tests {\n",
    "    const OPEN: char = '{';\n",
    "    const CLOSE: char = '}';\n",
    "    const QUOTE: char = '\\'';\n",
    "    const SLASH: char = '\\\\';\n",
    "    const TEXT: &str = \"}} \\\" {\";\n",
    "    const RAW: &str = r#\"}\"#;\n",
    "    // }\n",
    "    /* { */\n",
    "    mod nested { fn n<'a>(x: &'a str) -> &'a str { x } }\n",
    "    fn t() { let _ = \"TEST_ONLY\"; }\n",
    "}\n",
    "pub fn production() -> &'static str { \"PRODUCTION\" }\n",
    "#[cfg(all(test, unix))]\n",
    "#[allow(dead_code)]\n",
    "pub(crate) mod unix_tests { fn u() { let _ = \"UNIX_TEST_ONLY\"; } }\n",
    "#[cfg(any(test, feature = \"fixtures\"))]\n",
    "mod fixtures { fn f() { let _ = \"REACHABLE\"; } }\n",
    "#[cfg(not(test))]\n",
    "fn shipped() { let _ = \"NOT_TEST\"; }\n",
    "#[cfg(test)]\n",
    "fn helper() { let _ = \"TEST_FN\"; }\n",
    "fn tail() { let _ = \"TAIL\"; }\n",
);

#[test]
fn a_test_module_before_production_code_is_cut_by_its_own_extent() {
    let code = strip_test_modules(SEEDED);
    for kept in ["PRODUCTION", "REACHABLE", "NOT_TEST", "TAIL"] {
        assert!(code.contains(kept), "{kept} was cut:\n{code}");
    }
    for cut in ["TEST_ONLY", "UNIX_TEST_ONLY", "TEST_FN", "nested"] {
        assert!(!code.contains(cut), "{cut} was kept:\n{code}");
    }
    assert_eq!(
        code.lines().count(),
        SEEDED.lines().count(),
        "line numbers kept"
    );
    let gates: Vec<Gate> = gated_items(SEEDED)
        .into_iter()
        .map(|item| item.gate)
        .collect();
    assert_eq!(
        gates,
        [
            Gate::TestOnly,
            Gate::TestOnly,
            Gate::TestReachable,
            Gate::TestOnly
        ]
    );
    // The base (5ffbe1c) route-scan stripper cut from the first test module
    // to the end of the file: on this text it keeps no production code.
    let text = format!("\n{SEEDED}");
    let base = &text[..text.find("\n#[cfg(test)]\n").unwrap()];
    assert!(!base.contains("PRODUCTION"));
}

#[test]
fn every_literal_spelling_is_one_literal_and_comments_are_not_code() {
    let text = concat!(
        "let a = \"x \\\" y\"; let b = r#\"raw \" here\"#; let c = br\"bytes\";\n",
        "let d = b\"byte\"; let e = '\"'; let f = '\\''; fn g<'a>(_: &'a str) {}\n",
        "// \"comment\"\n/* \"block\" */ let h = \"after\";\n",
    );
    let found: Vec<String> = literals(text).into_iter().map(|lit| lit.text).collect();
    assert_eq!(found, ["x \\\" y", "raw \" here", "bytes", "byte", "after"]);
}

#[test]
fn dispatcher_arms_and_declared_lists_read_code_only() {
    let text = concat!(
        "const ACTIONS: &[&str] = &[\n    \"a]\",\n    // \"commented\",\n    \"b\",\n];\n",
        "fn f(x: &str) { match x {\n",
        "    \"one\" | \"two\" => 1,\n",
        "    \"three\" if ready() => { let _ = \"x => y\"; 2 }\n",
        "    // \"comment\" => 3,\n",
        "    Some(\"wrapped\") => 4,\n",
        "    _ => 0,\n",
        "} }\n",
    );
    assert_eq!(string_list(text, "ACTIONS"), ["a]", "b"]);
    assert_eq!(match_arms(text), ["one", "two", "three"]);
}

#[test]
fn production_excludes_files_only_test_code_declares() {
    let root = support::temp("source-scanner-production");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(
        "src/main.rs",
        "mod shipped;\n#[cfg(test)]\n#[path = \"fixture.rs\"]\nmod fixture;\n#[cfg(test)]\nmod tests;\n",
    );
    write("src/shipped.rs", "pub fn s() {}\n#[cfg(test)]\nmod unit;\n");
    write("src/shipped/unit.rs", "fn u() {}\n");
    write("src/fixture.rs", "mod inner;\n");
    write("src/fixture/inner.rs", "fn i() {}\n");
    write("src/tests.rs", "fn t() {}\n");
    let files: Vec<String> = production_in(&root)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(files, ["src/main.rs", "src/shipped.rs"]);
}
