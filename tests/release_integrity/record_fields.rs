//! R55 F12 (review B1): the rollback boundary is read from the allowlists.
//! Every run-ledger and record field the product's allowlists admit now but
//! did not admit at the previous published release is named, in backticks,
//! in the mixed-version paragraph of the newest CHANGELOG entry (the text
//! before its first bullet). Run-ledger fields read as `action.field`
//! (`src/run/state/unknown_fields.rs`, with the constants and
//! `append_allowed_fields` lists it pushes); record fields are the fields of
//! every `deny_unknown_fields` struct and every `FIELD`/`*_FIELD` extension
//! constant. The previous release's set is the snapshot in `SNAPSHOT`, which
//! names its release; it must be the entry directly below the newest
//! CHANGELOG heading (the release before the current one, so each release
//! commit regenerates it), and where that release's tag resolves (always under `EXITBIND_RELEASE_CHECKS`)
//! the snapshot must equal the set read from the tag's own source.
use super::rust_source::{blank, function, literals, production_in};
use super::*;
use std::collections::BTreeSet;

const SNAPSHOT: &str = "tests/fixtures/release-record-fields.txt";
const LEDGER_ALLOWLIST: &str = "src/run/state/unknown_fields.rs";
const RELEASE_ENV: &str = "EXITBIND_RELEASE_CHECKS";

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `snake_case` to `camelCase`.
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The string values of `const NAME: &str = "..."` and
/// `const NAME: &[&str] = &[...]` declared in `text`.
fn string_constant(text: &str, name: &str) -> Option<Vec<String>> {
    let code = blank(text);
    let lits = literals(text);
    let at = code
        .match_indices(&format!("const {name}:"))
        .map(|(at, _)| at)
        .next()?;
    let end = at + code[at..].find(';')?;
    let declared = &code[at..end];
    if !declared.contains("&str") {
        return None;
    }
    Some(
        lits.iter()
            .filter(|lit| lit.start > at && lit.end <= end)
            .map(|lit| lit.text.clone())
            .collect(),
    )
}

/// The file of the module that a path's second-to-last segment names: the
/// one production file `.../NAME.rs` or `.../NAME/mod.rs`.
fn module_file<'a>(sources: &'a [(String, String)], module: &str) -> &'a str {
    let found: Vec<&(String, String)> = sources
        .iter()
        .filter(|(path, _)| {
            path.ends_with(&format!("/{module}.rs")) || path.ends_with(&format!("/{module}/mod.rs"))
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "module `{module}` must name one source file"
    );
    &found[0].1
}

/// Field names a run-ledger allowlist segment admits: its identifier
/// literals, the `module::CONST` string constants it pushes, and every
/// `append_allowed_fields` list when it calls one.
fn segment_fields(
    sources: &[(String, String)],
    text: &str,
    start: usize,
    end: usize,
) -> Vec<String> {
    let code = blank(text);
    let mut fields: Vec<String> = literals(text)
        .into_iter()
        .filter(|lit| lit.start >= start && lit.end <= end && identifier(&lit.text))
        .map(|lit| lit.text)
        .collect();
    let segment = &code[start..end];
    for (at, _) in segment.match_indices("::") {
        let tail: String = segment[at + 2..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if tail.is_empty() || !tail.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        let module: String = segment[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Some(values) = string_constant(module_file(sources, &module), &tail) {
            fields.extend(values);
        }
    }
    if segment.contains("append_allowed_fields(") {
        for (_, file) in sources {
            let Some(body) = function(file, "append_allowed_fields") else {
                continue;
            };
            let body_code = blank(body);
            fields.extend(literals(body).into_iter().map(|lit| lit.text));
            for word in body_code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                if !word.is_empty() && word.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    fields.extend(string_constant(file, word).unwrap_or_default());
                }
            }
        }
    }
    fields
}

/// `action.field` for every field the run-ledger allowlist admits per action
/// (`other` for the closing `else`).
fn ledger_fields(sources: &[(String, String)]) -> BTreeSet<String> {
    let (_, file) = sources
        .iter()
        .find(|(path, _)| path == LEDGER_ALLOWLIST)
        .expect("the run-ledger allowlist exists");
    let body = function(file, "reject_unknown").expect("reject_unknown exists");
    let code = blank(body);
    let lits = literals(body);
    // (condition start, condition end, actions)
    let mut conditions: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for (at, _) in code.match_indices("action ==") {
        let lit = lits
            .iter()
            .find(|lit| lit.start > at)
            .expect("an action name");
        conditions.push((at, lit.end, vec![lit.text.clone()]));
    }
    for (at, _) in code.match_indices("matches!(") {
        if !code[at + "matches!(".len()..]
            .trim_start()
            .starts_with("action,")
        {
            continue;
        }
        let close = at + code[at..].find(')').expect("a closed matches!");
        let names = lits
            .iter()
            .filter(|lit| lit.start > at && lit.end <= close)
            .map(|lit| lit.text.clone())
            .collect();
        conditions.push((at, close, names));
    }
    for (at, _) in code.match_indices("} else {") {
        conditions.push((at, at + "} else {".len(), vec!["other".to_owned()]));
    }
    conditions.sort_by_key(|(at, _, _)| *at);
    let mut fields = BTreeSet::new();
    for (index, (_, after, actions)) in conditions.iter().enumerate() {
        let end = conditions.get(index + 1).map_or(body.len(), |next| next.0);
        for field in segment_fields(sources, body, *after, end) {
            for action in actions {
                fields.insert(format!("{action}.{field}"));
            }
        }
    }
    fields
}

/// The offset of the `}` closing the `{` that `code` starts with.
fn matching_brace(code: &str) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in code.bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return offset;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// Fields of every `deny_unknown_fields` struct and every `FIELD`/`*_FIELD`
/// string constant.
fn record_fields(sources: &[(String, String)]) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    for (_, text) in sources {
        let code = blank(text);
        for (at, _) in code.match_indices("const ") {
            let name: String = code[at + 6..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name == "FIELD" || name.ends_with("_FIELD") {
                fields.extend(string_constant(text, &name).unwrap_or_default());
            }
        }
        for (at, _) in code.match_indices("deny_unknown_fields") {
            let attribute = &text[code[..at].rfind("#[").unwrap()..at];
            let renamed = attribute.contains("camelCase");
            let rest = &code[at..];
            let Some(open) = rest.find('{') else { continue };
            if !rest[..open].contains("struct ") || rest[..open].contains(';') {
                continue;
            }
            let close = open + matching_brace(&rest[open..]);
            let mut rename: Option<String> = None;
            for line in text[at + open + 1..at + close].lines() {
                let line = line.trim();
                if line.starts_with("#[serde(") {
                    rename = line
                        .split("rename = \"")
                        .nth(1)
                        .and_then(|value| value.split('"').next())
                        .map(str::to_owned);
                    continue;
                }
                let line = line
                    .trim_start_matches("pub(crate) ")
                    .trim_start_matches("pub(super) ");
                let line = line.trim_start_matches("pub ");
                let Some((name, _)) = line.split_once(':') else {
                    continue;
                };
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    continue;
                }
                fields.insert(rename.take().unwrap_or_else(|| {
                    if renamed {
                        camel(name)
                    } else {
                        name.to_owned()
                    }
                }));
            }
        }
    }
    fields
}

/// Every field the allowlists of the source tree at `root` admit.
fn admitted(root: &Path) -> BTreeSet<String> {
    let sources = production_in(root);
    let mut fields = ledger_fields(&sources);
    fields.extend(record_fields(&sources));
    fields
}

/// The snapshot's release and fields.
fn snapshot(text: &str) -> (String, BTreeSet<String>) {
    let mut release = None;
    let mut fields = BTreeSet::new();
    for line in text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
    {
        match line.strip_prefix("release ") {
            Some(version) => release = Some(version.trim().to_owned()),
            None => {
                fields.insert(line.trim().to_owned());
            }
        }
    }
    (release.expect("the snapshot names its release"), fields)
}

fn render_snapshot(release: &str, fields: &BTreeSet<String>) -> String {
    let mut out = String::from(
        "# Run-ledger (`action.field`) and record fields the allowlists admitted at\n\
         # the release below, read by tests/release_integrity/record_fields.rs.\n\
         # Regenerate it from that test's printed set in the next release commit.\n",
    );
    out.push_str(&format!("release {release}\n"));
    for field in fields {
        out.push_str(field);
        out.push('\n');
    }
    out
}

/// The entry directly below the newest CHANGELOG heading: the release before
/// the current one. Read from the structure, not from a status sentence.
fn previous_release(changelog: &str) -> String {
    changelog
        .split("\n## ")
        .nth(2)
        .and_then(|section| section.lines().next())
        .expect("an entry below the newest CHANGELOG heading")
        .trim()
        .to_owned()
}

/// Fields added since the snapshot that the newest entry's mixed-version
/// paragraph (its text before the first bullet) does not name in backticks;
/// a record field also counts as named by an `action.field` of the ledger.
fn unnamed(added: &BTreeSet<String>, changelog: &str) -> Vec<String> {
    let newest = changelog.split("\n## ").nth(1).expect("a CHANGELOG entry");
    let paragraph = newest.split("\n- ").next().unwrap_or_default();
    added
        .iter()
        .filter(|field| {
            // A record field is named as itself or as some event's `action.field`.
            !(paragraph.contains(&format!("`{field}`"))
                || (!field.contains('.') && paragraph.contains(&format!(".{field}`"))))
        })
        .cloned()
        .collect()
}

fn changelog() -> String {
    fs::read_to_string(manifest().join("CHANGELOG.md")).unwrap()
}

#[test]
fn every_field_admitted_since_the_previous_release_is_named_in_the_mixed_version_paragraph() {
    let (release, previous) = snapshot(&fs::read_to_string(manifest().join(SNAPSHOT)).unwrap());
    let changelog = changelog();
    assert_eq!(
        release,
        previous_release(&changelog),
        "{SNAPSHOT} must be the fields of the entry below the newest CHANGELOG heading; regenerate it in the release commit"
    );
    let added: BTreeSet<String> = admitted(manifest())
        .difference(&previous)
        .cloned()
        .collect();
    let missing = unnamed(&added, &changelog);
    assert!(
        missing.is_empty(),
        "{release} cannot read these fields; name each in backticks in the newest CHANGELOG entry's mixed-version paragraph: {missing:?}"
    );
}

#[test]
fn a_start_field_left_out_of_the_mixed_version_paragraph_fails() {
    // The seeded field comes from the snapshot fixture, so this test does not
    // depend on which fields the current release happens to add.
    let (_, previous) = snapshot(&fs::read_to_string(manifest().join(SNAPSHOT)).unwrap());
    let field = previous
        .iter()
        .find(|field| field.starts_with("start."))
        .expect("the snapshot lists a start field")
        .clone();
    let record = field["start.".len()..].to_owned();
    let added = BTreeSet::from([field.clone(), record]);
    let named = format!(
        "# Changelog\n\n## 9.8.7\n\n{}\n\nOlder readers refuse `{field}`.\n\n- A change.\n\n## 9.8.6\n",
        release_line("9.8.7")
    );
    assert!(unnamed(&added, &named).is_empty());
    let seeded = named.replacen(&format!("`{field}`"), "the new marker", 1);
    assert_eq!(
        unnamed(&added, &seeded),
        added.iter().cloned().collect::<Vec<_>>(),
        "the start field and its record field are both unnamed"
    );
    assert_eq!(previous_release(&named), "9.8.6");
}

#[test]
fn the_reader_follows_constants_appended_lists_and_renamed_struct_fields() {
    let root = support::temp("record-fields-seeded");
    let write = |path: &str, text: &str| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    write(
        LEDGER_ALLOWLIST,
        "fn reject_unknown(action: &str) {\n    let allowed = if action == \"start\" {\n        allowed.push(super::novelty::FIELD);\n        crate::run::extra::append_allowed_fields(&mut allowed);\n        vec![\"runId\"]\n    } else if matches!(\n        action,\n        \"a\" | \"b\"\n    ) {\n        vec![\"seen\"]\n    } else {\n        vec![\"reason\"]\n    };\n}\n",
    );
    write(
        "src/run/state/novelty.rs",
        "pub(crate) const FIELD: &str = \"novelProtocol\";\n",
    );
    write(
        "src/run/extra.rs",
        "const NAMES: &[&str] = &[\"extraField\"];\npub(crate) fn append_allowed_fields(fields: &mut Vec<&str>) {\n    fields.extend(NAMES.iter().copied());\n}\n",
    );
    write(
        "src/hint.rs",
        "#[derive(Deserialize)]\n#[serde(rename_all = \"camelCase\", deny_unknown_fields)]\nstruct Hint {\n    latest_works: Vec<String>,\n    #[serde(rename = \"by\")]\n    recorded_by: String,\n}\nconst ADOPTED_FIELD: &str = \"journalKey\";\n",
    );
    let fields = admitted(&root);
    for expected in [
        "start.novelProtocol",
        "start.extraField",
        "start.runId",
        "a.seen",
        "b.seen",
        "other.reason",
        "latestWorks",
        "by",
        "journalKey",
    ] {
        assert!(
            fields.contains(expected),
            "{expected} missing from {fields:?}"
        );
    }
    assert!(
        !fields.contains("start.reason") && !fields.contains("recordedBy"),
        "{fields:?}"
    );
}

/// The snapshot equals the set read from its release tag's source, wherever
/// the tag resolves; a release check requires the tag.
#[test]
fn the_snapshot_is_what_the_published_release_admitted() {
    let text = fs::read_to_string(manifest().join(SNAPSHOT)).unwrap();
    let (release, listed) = snapshot(&text);
    let tag = format!("v{release}");
    let resolved = support::git_topology::git(manifest())
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{tag}^{{commit}}"),
        ])
        .output()
        .unwrap();
    if !resolved.status.success() {
        assert!(
            std::env::var_os(RELEASE_ENV).is_none(),
            "a release check needs tag {tag} to verify {SNAPSHOT}"
        );
        eprintln!("{SNAPSHOT}: tag {tag} is not in this checkout; compared at release");
        return;
    }
    let root = support::temp("record-fields-release");
    let archive = support::git_topology::git(manifest())
        .args(["archive", "--format=tar", &tag, "src"])
        .output()
        .unwrap();
    assert!(archive.status.success(), "git archive {tag} failed");
    let mut untar = support::git_topology::command("tar")
        .arg("-x")
        .arg("-C")
        .arg(root.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    untar
        .stdin
        .take()
        .unwrap()
        .write_all(&archive.stdout)
        .unwrap();
    assert!(untar.wait().unwrap().success(), "tar failed");
    let read = admitted(&root);
    assert!(
        read == listed,
        "{SNAPSHOT} differs from {tag}; replace it with:\n{}",
        render_snapshot(&release, &read)
    );
}
