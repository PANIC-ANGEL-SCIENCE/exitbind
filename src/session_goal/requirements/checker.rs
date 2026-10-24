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
        tokens(text).any(|token| project_relative(loaded, &token).as_deref() == Some(wanted))
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
        let Some(relative) = project_relative(loaded, &token) else {
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
        let path = Path::new(&token);
        if !path.is_absolute()
            || project_relative(loaded, &token).is_some()
            || scripts.iter().any(|(existing, _)| existing == &token)
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
            scripts.push((token, text));
        }
    }
    scripts
}

/// Shell-ish words: whitespace and shell punctuation separate words, quotes
/// group one word, and a leading `NAME=value` assignment is not a command
/// word. A `--flag=path` word also yields its value, so a project path after
/// `=` stays a candidate for the checker search.
pub(crate) fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
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
                        words.push(std::mem::take(&mut word));
                    }
                } else {
                    word.push(character);
                }
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let mut result = Vec::new();
    for word in words {
        if is_assignment(&word) {
            continue;
        }
        if let Some((_, value)) = word.split_once('=') {
            let value = value.trim_matches(|c| c == '\'' || c == '"');
            if !value.is_empty() {
                result.push(value.to_owned());
            }
        }
        result.push(word);
    }
    result.into_iter()
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
    use super::tokens;

    #[test]
    fn tokens_split_shell_punctuation() {
        let words: Vec<_> = tokens("exec ./a.sh --x=\"b/c.sh\"; (sh d)").collect();
        assert_eq!(words, ["exec", "./a.sh", "b/c.sh", "--x=b/c.sh", "sh", "d"]);
        let assignments: Vec<_> = tokens("X=1 \"/a b/check.sh\"").collect();
        assert_eq!(assignments, ["/a b/check.sh"]);
    }
}
