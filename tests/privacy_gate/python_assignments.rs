//! Recognize computed Python expressions from full source, never patch snippets.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

type Lines = BTreeMap<usize, (String, String)>;

fn computed_lines(source: &str) -> Lines {
    let mut child = match super::support::git_topology::command("python3")
        .args(["-I", "-S", "-c", include_str!("python_assignment_spans.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Lines::new(),
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(source.as_bytes()).is_ok());
    let Ok(output) = child.wait_with_output() else {
        return Lines::new();
    };
    if !written || !output.status.success() {
        return Lines::new();
    }
    let Ok(spans) = serde_json::from_slice::<Vec<[usize; 3]>>(&output.stdout) else {
        return Lines::new();
    };
    let original: Vec<_> = source.lines().collect();
    let mut grouped: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for [line, start, end] in spans {
        grouped.entry(line).or_default().push((start, end));
    }
    let mut result = Lines::new();
    for (line, mut spans) in grouped {
        let Some(text) = line.checked_sub(1).and_then(|index| original.get(index)) else {
            continue;
        };
        let mut masked = text.to_string();
        spans.sort_unstable();
        for (start, end) in spans.into_iter().rev() {
            if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                // The RHS is computed, not a literal credential. Keep every other byte.
                masked.replace_range(start..end, &" ".repeat(end - start));
            }
        }
        result.insert(line, (text.to_string(), masked));
    }
    result
}

pub fn file(source: &str, path: &Path) -> String {
    if path.extension().and_then(|x| x.to_str()) != Some("py")
        || !super::credential_assignment(source)
    {
        return source.to_string();
    }
    let lines = computed_lines(source);
    source
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            lines.get(&(index + 1)).map_or_else(
                || line.to_string(),
                |(_, masked)| format!("{masked}{}", if line.ends_with('\n') { "\n" } else { "" }),
            )
        })
        .collect()
}

#[derive(Clone)]
struct Blob {
    lines: Vec<String>,
    computed: Lines,
}

fn blob(root: &Path, id: &str, cache: &mut BTreeMap<String, Option<Blob>>) -> Option<Blob> {
    if id.len() != 40 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    cache
        .entry(id.to_string())
        .or_insert_with(|| {
            let source = String::from_utf8(super::git(root, &["cat-file", "blob", id])?).ok()?;
            let computed = if super::credential_assignment(&source) {
                computed_lines(&source)
            } else {
                Lines::new()
            };
            Some(Blob {
                lines: source.lines().map(str::to_string).collect(),
                computed,
            })
        })
        .clone()
}

fn range(field: &str, prefix: char) -> Option<(usize, usize)> {
    let field = field.strip_prefix(prefix)?;
    let (start, count) = field.split_once(',').unwrap_or((field, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn checked_hunk(
    lines: &[&str],
    before: Option<&Blob>,
    after: Option<&Blob>,
) -> Option<(usize, String)> {
    let fields: Vec<_> = lines.first()?.split_whitespace().collect();
    if fields.len() < 4 || fields[0] != "@@" || fields[3] != "@@" {
        return None;
    }
    let (mut old, mut nold) = range(fields[1], '-')?;
    let (mut new, mut nnew) = range(fields[2], '+')?;
    let mut result = lines[0].to_string();
    let mut consumed = 1;
    while nold > 0 || nnew > 0 {
        let raw = *lines.get(consumed)?;
        let text = raw.strip_suffix('\n').unwrap_or(raw);
        if text == "\\ No newline at end of file" {
            result.push_str(raw);
            consumed += 1;
            continue;
        }
        let body = text.get(1..)?;
        let marker = *text.as_bytes().first()?;
        let old_used = marker == b'-' || marker == b' ';
        let new_used = marker == b'+' || marker == b' ';
        if !old_used && !new_used {
            return None;
        }
        if old_used && (nold == 0 || before?.lines.get(old.checked_sub(1)?)?.as_str() != body) {
            return None;
        }
        if new_used && (nnew == 0 || after?.lines.get(new.checked_sub(1)?)?.as_str() != body) {
            return None;
        }
        let safe = if new_used {
            after?.computed.get(&new)
        } else {
            before?.computed.get(&old)
        };
        if let Some((original, masked)) = safe {
            if original != body {
                return None;
            }
            result.push(marker as char);
            result.push_str(masked);
            if raw.ends_with('\n') {
                result.push('\n');
            }
        } else {
            result.push_str(raw);
        }
        if old_used {
            old = old.checked_add(1)?;
            nold -= 1;
        }
        if new_used {
            new = new.checked_add(1)?;
            nnew -= 1;
        }
        consumed += 1;
    }
    Some((consumed, result))
}

pub fn patch(root: &Path, source: &str) -> String {
    let lines: Vec<_> = source.split_inclusive('\n').collect();
    let mut result = String::with_capacity(source.len());
    let mut ids = None;
    let mut before_path = None;
    let mut after_path = None;
    let mut cache = BTreeMap::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let text = line.strip_suffix('\n').unwrap_or(line);
        if text.starts_with("diff --git ") {
            ids = None;
            before_path = None;
            after_path = None;
        } else if let Some(value) = text.strip_prefix("index ") {
            ids = value
                .split_whitespace()
                .next()
                .and_then(|s| s.split_once(".."))
                .map(|(a, b)| (a.to_string(), b.to_string()));
        } else if let Some(path) = text.strip_prefix("--- ") {
            before_path = path.strip_prefix("a/").map(str::to_string);
        } else if let Some(path) = text.strip_prefix("+++ ") {
            after_path = path.strip_prefix("b/").map(str::to_string);
        } else if text.starts_with("@@ ") {
            if let Some((old, new)) = &ids {
                let is_python = |path: &Option<String>| {
                    path.as_deref().is_some_and(|p| {
                        Path::new(p).extension().and_then(|x| x.to_str()) == Some("py")
                    })
                };
                // Both sides must be Python or the absent side of an add/delete.
                let absent = |id: &str| id.len() == 40 && id.bytes().all(|b| b == b'0');
                if (is_python(&before_path) || absent(old))
                    && (is_python(&after_path) || absent(new))
                {
                    let before = blob(root, old, &mut cache);
                    let after = blob(root, new, &mut cache);
                    if let Some((consumed, masked)) =
                        checked_hunk(&lines[index..], before.as_ref(), after.as_ref())
                    {
                        result.push_str(&masked);
                        index += consumed;
                        continue;
                    }
                }
            }
        }
        result.push_str(line);
        index += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computed_python_assignments_preserve_literals_comments_and_other_languages() {
        let key = ["to", "ken"].concat();
        let expr = "marker.group(1)";
        let name = ["pass", "word"].concat();
        let benign = format!("{key} = {expr}\n");
        assert!(!super::super::credential_assignment(&file(
            &benign,
            Path::new("parser.py")
        )));
        for path in ["settings.ini", "settings.env", "notes.txt", "unknown"] {
            assert!(super::super::credential_assignment(&file(
                &benign,
                Path::new(path)
            )));
        }
        for text in [
            format!("{key} = '{expr}'\n"),
            format!("doc = '''\n{benign}'''\n"),
            format!("# {benign}"),
            format!("{benign}# {name} = ordinary_fixture_value\n"),
            format!("{key} = {expr}; {name} = 'ordinary_fixture_value'\n"),
            format!("{key} = {expr} # {name} = ordinary_fixture_value\n"),
            format!("{key} = marker.group('ordinary_fixture_value')\n"),
            format!("invalid python\n{benign}"),
            format!("é = 1; {key} = '{expr}'\n"),
        ] {
            assert!(
                super::super::credential_assignment(&file(&text, Path::new("parser.py"))),
                "literal fixture was missed"
            );
        }
        let unicode = format!("é = 1; {benign}");
        assert!(!super::super::credential_assignment(&file(
            &unicode,
            Path::new("parser.py")
        )));
    }

    #[test]
    fn malformed_hunks_are_not_partially_masked() {
        let key = ["to", "ken"].concat();
        let source = format!("{key} = marker.group(1)\npass\n");
        let blob = Blob {
            lines: source.lines().map(str::to_string).collect(),
            computed: computed_lines(&source),
        };
        for hunk in [
            format!("@@ -0,0 +1,2 @@\n+{key} = marker.group(1)\n"),
            format!("@@ -0,0 +1,2 @@\n+{key} = marker.group(1)\n+wrong\n"),
            format!("@@ -0,0 +1,2 @@\n+{key} = marker.group(1)\nunexpected\n"),
        ] {
            let lines: Vec<_> = hunk.split_inclusive('\n').collect();
            assert!(checked_hunk(&lines, None, Some(&blob)).is_none());
        }
    }

    fn git_ok(root: &Path, args: &[&str]) {
        assert!(super::super::git(root, args).is_some());
    }

    fn commit(root: &Path, message: &str) {
        git_ok(root, &["add", "."]);
        git_ok(
            root,
            &[
                "-c",
                "user.name=envy404",
                "-c",
                "user.email=287204764+envy404@users.noreply.github.com",
                "commit",
                "-qm",
                message,
            ],
        );
    }

    #[test]
    fn deleted_python_code_is_verified_against_blobs_but_deleted_secrets_remain_detected() {
        let root = super::super::temp("python-expression-history");
        git_ok(&root, &["init", "-q"]);
        let key = ["to", "ken"].concat();
        let benign = format!("{key} = marker.group(1)\n");
        std::fs::write(root.join("parser.py"), &benign).unwrap();
        commit(&root, "computed fixture");
        std::fs::write(root.join("parser.py"), "pass\n").unwrap();
        commit(&root, "remove computed fixture");
        assert!(super::super::scan(&root).is_empty());
        let raw = String::from_utf8(
            super::super::git(&root, &["log", "--full-index", "--format=", "-p"]).unwrap(),
        )
        .unwrap();
        assert!(!super::super::credential_assignment(&patch(&root, &raw)));
        let incorrect = raw.replace("@@ -1 +1 @@", "@@ -99 +99 @@");
        assert!(super::super::credential_assignment(&patch(
            &root, &incorrect
        )));
        let unknown = raw
            .replace("--- a/parser.py", "--- unknown")
            .replace("+++ b/parser.py", "+++ unknown");
        assert!(super::super::credential_assignment(&patch(&root, &unknown)));
        std::fs::write(root.join("parser.py"), format!("doc = '''\n{benign}'''\n")).unwrap();
        std::fs::write(root.join("settings.ini"), &benign).unwrap();
        commit(&root, "literal fixtures");
        std::fs::write(root.join("parser.py"), "pass\n").unwrap();
        std::fs::write(root.join("settings.ini"), "clean\n").unwrap();
        commit(&root, "remove literal fixtures");
        assert!(super::super::scan(&root)
            .iter()
            .any(|(category, source)| category == "credential-assignment"
                && source == "git-history:patches"));
    }
}
