//! Reading Rust source as text for the route inventories: string literals
//! (comments, char literals, lifetimes and raw strings handled), production
//! files without their test modules, expression extents and `fn`
//! definitions.
use super::emitted_routes::sources;
use std::collections::BTreeMap;

/// A string literal: byte span and raw content.
pub(super) struct Literal {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) text: String,
}

/// String literals of Rust source, skipping comments, char literals and
/// lifetimes; raw strings are literals too.
pub(super) fn literals(text: &str) -> Vec<Literal> {
    let bytes = text.as_bytes();
    let ident =
        |at: usize| at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("//") {
            i += rest.find('\n').unwrap_or(rest.len());
        } else if rest.starts_with("/*") {
            i += rest.find("*/").map_or(rest.len(), |end| end + 2);
        } else if bytes[i] == b'r'
            && !ident(i)
            && (rest.starts_with("r\"") || rest.starts_with("r#"))
        {
            let hashes = rest[1..].bytes().take_while(|byte| *byte == b'#').count();
            if rest.as_bytes().get(1 + hashes) != Some(&b'"') {
                i += 1;
                continue;
            }
            let close = format!("\"{}", "#".repeat(hashes));
            let body = i + 2 + hashes;
            let end = text[body..].find(&close).map_or(text.len(), |at| body + at);
            out.push(Literal {
                start: i,
                end: end + close.len(),
                text: text[body..end].to_owned(),
            });
            i = end + close.len();
        } else if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            out.push(Literal {
                start: i,
                end: j + 1,
                text: text[i + 1..j.min(text.len())].to_owned(),
            });
            i = j + 1;
        } else if bytes[i] == b'\'' {
            let next = rest[1..].chars().next().map_or(0, char::len_utf8);
            i += if rest[1..].starts_with('\\') {
                rest[2..].find('\'').map_or(1, |at| at + 3)
            } else if rest.as_bytes().get(1 + next) == Some(&b'\'') {
                next + 2
            } else {
                1
            };
        } else {
            i += 1;
        }
    }
    out
}

/// Production source: no `*tests.rs` file, no trailing `#[cfg(test)]`
/// module, and not the builder itself.
pub(super) fn production() -> Vec<(String, String)> {
    sources()
        .into_iter()
        .filter(|(path, _)| !path.ends_with("tests.rs") && !path.starts_with("src/emitted"))
        .map(|(path, text)| (path, strip_test_module(&text)))
        .collect()
}

/// Source without its inline test module: in `src/` a `#[cfg(test)] mod
/// name {` block is always the file's last item.
pub(super) fn strip_test_module(text: &str) -> String {
    for (start, marker) in text.match_indices("\n#[cfg(test)]\n") {
        let item = text[start + marker.len()..]
            .lines()
            .find(|line| !line.starts_with("#["))
            .unwrap_or("");
        if item.starts_with("mod ") && item.ends_with('{') {
            return text[..start].to_owned();
        }
    }
    text.to_owned()
}

pub(super) fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The literal spanning byte `at`, if any (literals are sorted by start).
pub(super) fn containing(lits: &[Literal], at: usize) -> Option<&Literal> {
    let after = lits.partition_point(|lit| lit.start <= at);
    after
        .checked_sub(1)
        .map(|index| &lits[index])
        .filter(|lit| at < lit.end)
}

/// The unmatched `[` that opens the list holding byte `at`.
pub(super) fn opening_bracket(text: &str, lits: &[Literal], at: usize) -> usize {
    opening(text, lits, at, b'[', b']')
}

/// The unmatched `open` (with its `close`) that encloses byte `at`.
pub(super) fn opening(text: &str, lits: &[Literal], at: usize, open: u8, close: u8) -> usize {
    let mut depth = 0;
    let mut i = at;
    while i > 0 {
        i -= 1;
        if let Some(lit) = containing(lits, i) {
            i = lit.start;
            continue;
        }
        match text.as_bytes()[i] {
            byte if byte == close => depth += 1,
            byte if byte == open && depth == 0 => return i,
            byte if byte == open => depth -= 1,
            _ => {}
        }
    }
    0
}

/// The byte where the expression starting at `at` ends: the first of
/// `stops` or an unmatched closing bracket at depth 0, skipping literals.
pub(super) fn end_of(text: &str, lits: &[Literal], at: usize, stops: &[u8]) -> usize {
    let mut depth = 0i32;
    let mut i = at;
    while i < text.len() {
        if let Ok(found) = lits.binary_search_by_key(&i, |lit| lit.start) {
            i = lits[found].end;
            continue;
        }
        match text.as_bytes()[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => break,
            b')' | b']' | b'}' => depth -= 1,
            byte if depth == 0 && stops.contains(&byte) => break,
            _ => {}
        }
        i += 1;
    }
    i
}

/// The value written after `at`: up to `,` or `;` or the closing bracket.
pub(super) fn expression(text: &str, lits: &[Literal], at: usize) -> String {
    text[at..end_of(text, lits, at, b",;")].trim().to_owned()
}

/// `fn name` definitions with their full text, by name.
pub(super) fn functions(sources: &[(String, String)]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut index: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (path, text) in sources {
        let lits = literals(text);
        for (at, _) in text.match_indices("fn ") {
            if at > 0 && text.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            let name: String = text[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let Some(open) = text[at..].find('{').map(|open| at + open) else {
                continue;
            };
            let end = end_of(text, &lits, open + 1, b"") + 1;
            index
                .entry(name)
                .or_default()
                .push((path.clone(), text[at..end.min(text.len())].to_owned()));
        }
    }
    index
}
