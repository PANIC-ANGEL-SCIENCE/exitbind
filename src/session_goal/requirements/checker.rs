//! Name the project checker a declared check command runs.
//!
//! A check command names its checker directly (`sh scripts/check.sh`) or
//! through one outside script it runs (`sh /abs/check.sh` whose text runs a
//! project script). Both `goal prepare` and the named `work begin` route use
//! this owner to derive an omitted `--artifact` and to accept a given one; the
//! frozen checker hash and coverage rules stay in `work::checker_input`.

use crate::config::Loaded;
use std::path::Path;

/// Largest outside script whose text is read for a checker name.
const MAX_SCRIPT_BYTES: u64 = 64 * 1024;

/// The checker a command was found to run, and the script that named it.
pub(crate) struct Derived {
    pub(crate) path: String,
    pub(crate) via: Option<String>,
}

/// Use the declared checker when one is given; otherwise derive it from the
/// check command. Validation of either result stays with `checker_input`.
pub(crate) fn resolve(
    loaded: &Loaded,
    declared: Option<&str>,
    command: &str,
) -> Result<Derived, String> {
    match declared {
        Some(path) => Ok(Derived {
            path: path.into(),
            via: None,
        }),
        None => derive(loaded, command),
    }
}

/// True when the command names the checker directly (the original textual
/// rule) or runs one outside script whose text names it.
pub(crate) fn command_names(loaded: &Loaded, command: &str, checker: &str) -> bool {
    if command.contains(checker) {
        return true;
    }
    let wanted = checker.trim_start_matches("./");
    outside_scripts(loaded, command).iter().any(|(_, text)| {
        tokens(text).any(|token| project_relative(loaded, token).as_deref() == Some(wanted))
    })
}

fn derive(loaded: &Loaded, command: &str) -> Result<Derived, String> {
    let direct = covered_named(loaded, command)?;
    if direct.len() == 1 {
        return Ok(Derived {
            path: direct.into_iter().next().unwrap_or_default(),
            via: None,
        });
    }
    if direct.len() > 1 {
        return Err(ambiguous("check command", &direct));
    }
    let mut found: Vec<(String, String)> = Vec::new();
    for (script, text) in outside_scripts(loaded, command) {
        for path in covered_named(loaded, &text)? {
            if !found.iter().any(|(existing, _)| *existing == path) {
                found.push((path, script.clone()));
            }
        }
    }
    match found.len() {
        1 => {
            let (path, script) = found.remove(0);
            Ok(Derived {
                path,
                via: Some(script),
            })
        }
        0 => Err(format!("check command names no tested project file, directly or through one outside script it runs; pass --artifact {} naming the project file the check runs", crate::emitted::slot("PROJECT_CHECKER"))),
        _ => Err(ambiguous(
            "check command's script",
            &found.into_iter().map(|(path, _)| path).collect::<Vec<_>>(),
        )),
    }
}

fn ambiguous(what: &str, found: &[String]) -> String {
    format!(
        "{what} names several tested project files ({}); pass --artifact {} to choose the checker",
        found.join(", "),
        crate::emitted::slot("PROJECT_CHECKER")
    )
}

/// Covered project files named by the text's tokens, in first-seen order.
fn covered_named(loaded: &Loaded, text: &str) -> Result<Vec<String>, String> {
    let mut found: Vec<String> = Vec::new();
    for token in tokens(text) {
        let Some(relative) = project_relative(loaded, token) else {
            continue;
        };
        if found.contains(&relative) {
            continue;
        }
        let is_file = std::fs::symlink_metadata(loaded.product_root.join(&relative))
            .is_ok_and(|meta| meta.file_type().is_file());
        if is_file && crate::run::inputs::covered_file(loaded, &relative)? {
            found.push(relative);
        }
    }
    Ok(found)
}

/// Existing regular UTF-8 files outside the project named by absolute command
/// tokens, with their text. Unreadable or oversized files are not candidates.
fn outside_scripts(loaded: &Loaded, command: &str) -> Vec<(String, String)> {
    let mut scripts: Vec<(String, String)> = Vec::new();
    for token in tokens(command) {
        let path = Path::new(token);
        if !path.is_absolute()
            || project_relative(loaded, token).is_some()
            || scripts
                .iter()
                .any(|(existing, _)| existing.as_str() == token)
        {
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_SCRIPT_BYTES {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(path) {
            scripts.push((token.to_owned(), text));
        }
    }
    scripts
}

/// Shell-ish words: whitespace and shell punctuation separate path candidates.
/// This is the textual splitter the checker derivation has always used; it is
/// deliberately not a shell parser, so a comment's apostrophe or an
/// assignment's `=` neither hides a later path nor offers one.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || ";&|()<>\"'`={}".contains(c))
        .filter(|token| !token.is_empty())
}

/// The one program a check command executes: the first word after any leading
/// `NAME=value` environment assignments, read with quote grouping. Readiness
/// asks only this question, so quoting and the environment prefix reach the
/// same executable test; path candidates stay with `tokens`.
pub(crate) fn first_program(command: &str) -> Option<String> {
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars();
    while let Some(character) = chars.next() {
        match quote {
            Some(delimiter) => {
                if character == delimiter {
                    quote = None;
                } else if character == '\\' && delimiter == '"' {
                    if let Some(escaped) = chars.next() {
                        word.push(escaped);
                    }
                } else {
                    word.push(character);
                }
            }
            None => {
                if character == '\'' || character == '"' {
                    quote = Some(character);
                } else if character == '\\' {
                    if let Some(escaped) = chars.next() {
                        word.push(escaped);
                    }
                } else if character.is_whitespace() || ";&|()<>{}`".contains(character) {
                    if !word.is_empty() {
                        if !is_assignment(&word) {
                            return Some(word);
                        }
                        word.clear();
                    }
                } else {
                    word.push(character);
                }
            }
        }
    }
    if !word.is_empty() && !is_assignment(&word) {
        return Some(word);
    }
    None
}

/// `NAME=value` before the command word is the shell's environment prefix, not
/// a command or a path.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut characters = name.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// The project-relative form of a token naming a path inside the project,
/// placed by the one root helper.
fn project_relative(loaded: &Loaded, token: &str) -> Option<String> {
    crate::project::path::inputs::relative(&loaded.product_root, token)
}

#[cfg(test)]
mod tests {
    use super::{first_program, tokens};

    #[test]
    fn tokens_split_shell_punctuation_as_path_candidates() {
        let words: Vec<_> = tokens("exec ./a.sh --x=\"b/c.sh\"; (sh d)").collect();
        assert_eq!(words, ["exec", "./a.sh", "--x", "b/c.sh", "sh", "d"]);
        let assignments: Vec<_> = tokens("X=1 \"/a b/check.sh\"").collect();
        assert_eq!(assignments, ["X", "1", "/a", "b/check.sh"]);
    }

    #[test]
    fn first_program_skips_assignments_and_honours_quotes() {
        assert_eq!(first_program("./check.sh").as_deref(), Some("./check.sh"));
        assert_eq!(
            first_program("X=1 ./check.sh").as_deref(),
            Some("./check.sh")
        );
        assert_eq!(
            first_program("X=1 Y='a b' ./check.sh").as_deref(),
            Some("./check.sh")
        );
        assert_eq!(
            first_program("\"/a b/check.sh\"").as_deref(),
            Some("/a b/check.sh")
        );
        assert_eq!(first_program("sh ./check.sh").as_deref(), Some("sh"));
        assert_eq!(first_program("X=1").as_deref(), None);
        assert_eq!(first_program("   ").as_deref(), None);
    }
}
