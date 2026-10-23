//! The one Rust source scanner every inventory test reads `src/` with
//! (R55 F11). String literals, char literals and comments are found once
//! (lifetimes, escapes and raw strings handled); everything else works on
//! code with those blanked, so a brace, `=>` or keyword inside a literal or a
//! comment never counts. Test-gated items are cut by their own extent
//! wherever they sit, not "from the test module to the end of the file";
//! files that only test code declares are found through `mod` items,
//! `#[path]` and nesting; dispatcher arms and declared lists are read from
//! the CLI's own tables.
#![allow(dead_code)]
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// A string literal: byte span (quotes included) and raw content.
pub struct Literal {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// String literals, char literals and comments, each sorted by start.
struct Scanned {
    literals: Vec<Literal>,
    chars: Vec<(usize, usize)>,
    comments: Vec<(usize, usize)>,
}

fn scan(text: &str) -> Scanned {
    let bytes = text.as_bytes();
    let ident =
        |at: usize| at > 0 && (bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
    let mut out = Scanned {
        literals: Vec::new(),
        chars: Vec::new(),
        comments: Vec::new(),
    };
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("//") {
            let end = i + rest.find('\n').unwrap_or(rest.len());
            out.comments.push((i, end));
            i = end;
        } else if rest.starts_with("/*") {
            let end = i + rest.find("*/").map_or(rest.len(), |end| end + 2);
            out.comments.push((i, end));
            i = end;
        } else if bytes[i] == b'r'
            && (!ident(i) || (bytes[i - 1] == b'b' && !ident(i - 1)))
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
            let after = (end + close.len()).min(text.len());
            out.literals.push(Literal {
                start: i,
                end: after,
                text: text[body..end].to_owned(),
            });
            i = after;
        } else if bytes[i] == b'"' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(text.len());
            out.literals.push(Literal {
                start: i,
                end,
                text: text[i + 1..j.min(text.len())].to_owned(),
            });
            i = end;
        } else if bytes[i] == b'\'' {
            let next = rest[1..].chars().next().map_or(0, char::len_utf8);
            let end = if rest[1..].starts_with('\\') {
                // Skip the escaped character itself: '\'' and '\\' close after it.
                let body = 2 + rest[2..].chars().next().map_or(0, char::len_utf8);
                rest[body..].find('\'').map(|at| i + body + at + 1)
            } else if next > 0 && rest.as_bytes().get(1 + next) == Some(&b'\'') {
                Some(i + next + 2)
            } else {
                None // a lifetime
            };
            match end {
                Some(end) => {
                    out.chars.push((i, end));
                    i = end;
                }
                None => i += 1,
            }
        } else {
            i += 1;
        }
    }
    out
}

/// String literals of Rust source, skipping comments, char literals and
/// lifetimes; raw strings are literals too.
pub fn literals(text: &str) -> Vec<Literal> {
    scan(text).literals
}

/// `text` with every comment and the inside of every string and char literal
/// replaced by spaces (newlines kept): same length, same byte offsets.
pub fn blank(text: &str) -> String {
    let scanned = scan(text);
    let mut bytes = text.as_bytes().to_vec();
    let mut clear = |start: usize, end: usize| {
        for byte in &mut bytes[start..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    };
    for lit in &scanned.literals {
        clear(lit.start + 1, lit.end.saturating_sub(1).max(lit.start + 1));
    }
    for (start, end) in &scanned.chars {
        clear(start + 1, end.saturating_sub(1).max(start + 1));
    }
    for (start, end) in &scanned.comments {
        clear(*start, *end);
    }
    // Only ASCII bytes were replaced, but a multi-byte character may have
    // been partly cleared; replace any broken sequence the same way.
    String::from_utf8(bytes).unwrap_or_else(|error| {
        String::from_utf8_lossy(error.as_bytes())
            .chars()
            .map(|c| if c == '\u{FFFD}' { ' ' } else { c })
            .collect()
    })
}

/// How a `#[cfg(..)]` attribute gates its item on `test`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Gate {
    /// Compiled in test builds and elsewhere: `cfg(any(test, ..))`.
    TestReachable,
    /// Compiled only in test builds: `cfg(test)`, `cfg(all(test, ..))`.
    TestOnly,
}

/// The gate a `#[cfg(..)]` attribute puts on `test`, if any.
pub fn gate(attribute: &str) -> Option<Gate> {
    let compact: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
    let inner = compact.strip_prefix("#[cfg(")?.strip_suffix(")]")?;
    if inner == "test" {
        return Some(Gate::TestOnly);
    }
    let (only, terms) = if let Some(rest) = inner.strip_prefix("all(") {
        (true, rest.strip_suffix(')')?)
    } else if let Some(rest) = inner.strip_prefix("any(") {
        (false, rest.strip_suffix(')')?)
    } else {
        return None;
    };
    let mut depth = 0;
    let mut start = 0;
    let mut found = false;
    for (at, c) in terms.char_indices().chain([(terms.len(), ',')]) {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                found |= &terms[start..at] == "test";
                start = at + 1;
            }
            _ => {}
        }
    }
    found.then_some(if only {
        Gate::TestOnly
    } else {
        Gate::TestReachable
    })
}

/// An item gated on `test`: its extent from the gating attribute to its
/// closing brace or semicolon.
pub struct Gated {
    pub start: usize,
    pub end: usize,
    pub gate: Gate,
}

/// The end of the item starting at `at` in blanked code: after its first
/// depth-0 `;`, or after the brace that closes its first depth-0 `{`.
fn item_end(code: &str, at: usize) -> usize {
    let mut depth = 0i32;
    for (offset, byte) in code.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth -= 1,
            b'}' => {
                depth -= 1;
                if depth <= 0 {
                    return at + offset + 1;
                }
            }
            b';' if depth == 0 => return at + offset + 1,
            _ => {}
        }
    }
    code.len()
}

/// The `]` closing the attribute that opens at `at` in blanked code.
fn attribute_end(code: &str, at: usize) -> usize {
    let mut depth = 0;
    for (offset, byte) in code.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return at + offset + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// Every item a `#[cfg(..)]` attribute gates on `test`, outermost only, in
/// order: wherever it sits, whatever it is (`mod`, `fn`, `impl`, `use`).
pub fn gated_items(text: &str) -> Vec<Gated> {
    let code = blank(text);
    let mut found: Vec<Gated> = Vec::new();
    for (at, _) in code.match_indices("#[cfg(") {
        if found.last().is_some_and(|item| at < item.end) {
            continue;
        }
        let close = attribute_end(&code, at);
        let Some(gate) = gate(&code[at..close]) else {
            continue;
        };
        // Further attributes may follow before the item itself.
        let mut item = close;
        loop {
            let rest = &code[item..];
            item += rest.len() - rest.trim_start().len();
            if code[item..].starts_with("#[") {
                item = attribute_end(&code, item);
            } else {
                break;
            }
        }
        found.push(Gated {
            start: at,
            end: item_end(&code, item),
            gate,
        });
    }
    found
}

/// `text` without its test-only items (their newlines kept, so line numbers
/// stay those of the file).
pub fn strip_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for item in gated_items(text)
        .into_iter()
        .filter(|item| item.gate == Gate::TestOnly)
    {
        out.push_str(&text[cursor..item.start]);
        out.extend(text[item.start..item.end].matches('\n'));
        cursor = item.end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// One `mod` item: its name, whether it is out of line (`mod x;`), the gate
/// of its attributes, its `#[path]` and the line its attributes start on.
pub struct ModItem {
    pub name: String,
    pub out_of_line: bool,
    pub gate: Option<Gate>,
    pub path: Option<String>,
    pub line: usize,
}

/// Every `mod` item of `text`, read on blanked code (a `mod` in a literal or
/// comment is not one); `#[path]` values come from the original text.
pub fn mod_items(text: &str) -> Vec<ModItem> {
    let code = blank(text);
    let mut items = Vec::new();
    let mut attributes: Vec<(usize, String)> = Vec::new();
    let mut offset = 0;
    for (index, line) in code.split('\n').enumerate() {
        let start = offset;
        offset += line.len() + 1;
        let trimmed = line.trim();
        if trimmed.starts_with("#[") {
            let at = start + (line.len() - line.trim_start().len());
            let original = &text[at..attribute_end(&code, at).min(text.len())];
            attributes.push((index, original.replace(' ', "")));
            continue;
        }
        let item = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub(super) "))
            .or_else(|| trimmed.strip_prefix("pub "))
            .unwrap_or(trimmed);
        if let Some(rest) = item.strip_prefix("mod ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let after = rest[name.len()..].trim_start();
            if !name.is_empty() && (after.starts_with(';') || after.starts_with('{')) {
                items.push(ModItem {
                    name,
                    out_of_line: after.starts_with(';'),
                    gate: attributes
                        .iter()
                        .filter_map(|(_, attribute)| gate(attribute))
                        .max(),
                    path: attributes.iter().find_map(|(_, attribute)| {
                        let value = attribute.strip_prefix("#[path=\"")?;
                        Some(value[..value.find('"')?].to_owned())
                    }),
                    line: attributes.first().map_or(index, |(first, _)| *first),
                });
            }
        }
        if !trimmed.is_empty() {
            attributes.clear();
        }
    }
    items
}

/// The file an out-of-line `mod` item of `file` declares, if it exists.
pub fn declared_file(root: &Path, file: &str, item: &ModItem) -> Option<String> {
    let parent = file.rsplit_once('/').map_or("", |(parent, _)| parent);
    if let Some(path) = &item.path {
        return Some(format!("{parent}/{path}"));
    }
    let stem = file.trim_end_matches(".rs");
    let own_dir = match stem.rsplit_once('/') {
        Some((dir, "mod" | "main" | "lib")) => dir.to_owned(),
        _ => stem.to_owned(),
    };
    [
        format!("{own_dir}/{}.rs", item.name),
        format!("{own_dir}/{}/mod.rs", item.name),
    ]
    .into_iter()
    .find(|candidate| root.join(candidate).is_file())
}

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `root/dir` (relative, `/`-separated, sorted) with
/// its text; `tests/fixtures` is data, not code.
pub fn sources_in(root: &Path, dir: &str) -> Vec<(String, String)> {
    fn visit(root: &Path, dir: &str, out: &mut Vec<(String, String)>) {
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            return;
        };
        for entry in entries {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            let relative = format!("{dir}/{name}");
            if entry.file_type().unwrap().is_dir() {
                if relative != "tests/fixtures" {
                    visit(root, &relative, out);
                }
            } else if name.ends_with(".rs") {
                let text = fs::read_to_string(root.join(&relative)).unwrap();
                out.push((relative, text));
            }
        }
    }
    let mut out = Vec::new();
    visit(root, dir, &mut out);
    out.sort();
    out
}

/// Every file under `src/` as written, test code included.
pub fn sources() -> Vec<(String, String)> {
    sources_in(manifest(), "src")
}

/// Test-code files of `root` with their gate: every file under `tests/`
/// (fixtures excluded), every `src/` file a `cfg(test)`-gated `mod` item
/// declares, and every file those declare in turn.
pub fn test_files(root: &Path) -> BTreeMap<String, Gate> {
    let files: Vec<(String, String)> = sources_in(root, "src")
        .into_iter()
        .chain(sources_in(root, "tests"))
        .collect();
    let known: BTreeSet<&str> = files.iter().map(|(file, _)| file.as_str()).collect();
    let items: Vec<(&str, Vec<ModItem>)> = files
        .iter()
        .map(|(file, text)| (file.as_str(), mod_items(text)))
        .collect();
    let mut test: BTreeMap<String, Gate> = files
        .iter()
        .filter(|(file, _)| file.starts_with("tests/"))
        .map(|(file, _)| (file.clone(), Gate::TestOnly))
        .collect();
    loop {
        let mut added = false;
        for (file, declared) in &items {
            for item in declared.iter().filter(|item| item.out_of_line) {
                let gate = match (test.get(*file), item.gate) {
                    (Some(parent), Some(own)) => Some((*parent).max(own)),
                    (Some(parent), None) => Some(*parent),
                    (None, own) => own,
                };
                let (Some(gate), Some(child)) = (gate, declared_file(root, file, item)) else {
                    continue;
                };
                if known.contains(child.as_str()) && test.get(&child).map_or(true, |g| *g < gate) {
                    test.insert(child, gate);
                    added = true;
                }
            }
        }
        if !added {
            return test;
        }
    }
}

/// Production source: every `src/` file except those only test code
/// declares (and `*tests.rs`), each without its test-only items.
pub fn production() -> Vec<(String, String)> {
    production_in(manifest())
}

pub fn production_in(root: &Path) -> Vec<(String, String)> {
    let test = test_files(root);
    sources_in(root, "src")
        .into_iter()
        .filter(|(path, _)| {
            test.get(path) != Some(&Gate::TestOnly)
                && !path.ends_with("/tests.rs")
                && !path.ends_with("_tests.rs")
        })
        .map(|(path, text)| {
            let stripped = strip_test_modules(&text);
            (path, stripped)
        })
        .collect()
}

pub fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The literal spanning byte `at`, if any (literals are sorted by start).
pub fn containing(lits: &[Literal], at: usize) -> Option<&Literal> {
    let after = lits.partition_point(|lit| lit.start <= at);
    after
        .checked_sub(1)
        .map(|index| &lits[index])
        .filter(|lit| at < lit.end)
}

/// The unmatched `[` that opens the list holding byte `at`.
pub fn opening_bracket(text: &str, lits: &[Literal], at: usize) -> usize {
    opening(text, lits, at, b'[', b']')
}

/// The unmatched `open` (with its `close`) that encloses byte `at`.
pub fn opening(text: &str, lits: &[Literal], at: usize, open: u8, close: u8) -> usize {
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
pub fn end_of(text: &str, lits: &[Literal], at: usize, stops: &[u8]) -> usize {
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
pub fn expression(text: &str, lits: &[Literal], at: usize) -> String {
    text[at..end_of(text, lits, at, b",;")].trim().to_owned()
}

/// The `{` opening the body of the `fn` at `at`, unless a `;` outside
/// parentheses and brackets ends it first.
fn body_open(code: &str, at: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, byte) in code.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'{' if depth == 0 => return Some(at + offset),
            b';' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// `fn name` definitions with their full text, by name.
pub fn functions(sources: &[(String, String)]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut index: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (path, text) in sources {
        let code = blank(text);
        for (at, _) in code.match_indices("fn ") {
            if at > 0 && {
                let before = code.as_bytes()[at - 1];
                before.is_ascii_alphanumeric() || before == b'_'
            } {
                continue;
            }
            let name: String = code[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let Some(open) = body_open(&code, at) else {
                continue; // a signature without a body
            };
            let end = item_end(&code, open);
            index
                .entry(name)
                .or_default()
                .push((path.clone(), text[at..end.min(text.len())].to_owned()));
        }
    }
    index
}

/// The body of the first `fn name` in `text`, if any.
pub fn function<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let code = blank(text);
    let at = code
        .match_indices(&format!("fn {name}("))
        .find_map(|(at, _)| {
            (at == 0 || !code.as_bytes()[at - 1].is_ascii_alphanumeric()).then_some(at)
        })?;
    let open = body_open(&code, at)?;
    Some(&text[at..item_end(&code, open)])
}

/// One arm of a `match`: its pattern text, its string patterns (empty for
/// `_`, a binding or any non-string pattern), whether a guard follows, and
/// its body.
pub struct Arm {
    pub pattern: String,
    pub strings: Vec<String>,
    pub guarded: bool,
    pub body: String,
}

/// The end of a non-block arm body starting at `at`: the `,` at depth 0, or
/// the brace that closes the enclosing block.
fn arm_body_end(code: &str, at: usize) -> usize {
    let mut depth = 0i32;
    for (offset, byte) in code.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => return at + offset,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => return at + offset,
            _ => {}
        }
    }
    code.len()
}

/// Splits `code[start..end]` at depth-0 occurrences of `separator`.
fn split_depth0(code: &str, start: usize, end: usize, separator: &str) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut from = start;
    let mut i = start;
    while i < end {
        match code.as_bytes()[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 0 && code[i..end].starts_with(separator) => {
                parts.push((from, i));
                i += separator.len();
                from = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push((from, end));
    parts
}

/// The top-level arms of the `match` block whose `{` is at `open`.
pub fn arms_at(text: &str, open: usize) -> Vec<Arm> {
    let code = blank(text);
    let lits = literals(text);
    let bytes = code.as_bytes();
    let close = item_end(&code, open).saturating_sub(1);
    let mut arms = Vec::new();
    let mut i = open + 1;
    loop {
        while i < close && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= close {
            return arms;
        }
        let Some(arrow) = split_depth0(&code, i, close, "=>")
            .first()
            .map(|(_, end)| *end)
            .filter(|end| *end < close)
        else {
            return arms;
        };
        let mut body = arrow + 2;
        while body < close && bytes[body].is_ascii_whitespace() {
            body += 1;
        }
        let end = if bytes.get(body) == Some(&b'{') {
            item_end(&code, body)
        } else {
            arm_body_end(&code, body)
        };
        let guard = split_depth0(&code, i, arrow, " if ");
        let head = guard[0];
        let mut strings = Vec::new();
        for (from, to) in split_depth0(&code, head.0, head.1, "|") {
            let part = &code[from..to];
            let at = from + (part.len() - part.trim_start().len());
            match lits.binary_search_by_key(&at, |lit| lit.start) {
                Ok(index) if lits[index].end == from + part.trim_end().len() => {
                    strings.push(lits[index].text.clone());
                }
                _ => {
                    strings.clear();
                    break;
                }
            }
        }
        arms.push(Arm {
            pattern: text[i..arrow].trim().to_owned(),
            strings,
            guarded: guard.len() > 1,
            body: text[body..end.min(text.len())].to_owned(),
        });
        i = end;
    }
}

/// Every `match` in `text`: its scrutinee and its top-level arms.
pub fn matches(text: &str) -> Vec<(String, Vec<Arm>)> {
    let code = blank(text);
    let mut found = Vec::new();
    for at in code_words(text, "match") {
        let start = at + "match".len();
        let mut depth = 0i32;
        let open = code.as_bytes()[start..]
            .iter()
            .enumerate()
            .find_map(|(offset, byte)| {
                match byte {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth -= 1,
                    b'{' if depth == 0 => return Some(start + offset),
                    b';' if depth == 0 => return Some(usize::MAX),
                    _ => {}
                }
                None
            });
        if let Some(open) = open.filter(|open| *open != usize::MAX) {
            found.push((text[start..open].trim().to_owned(), arms_at(text, open)));
        }
    }
    found
}

/// The string arms of every `match` in `text` (`"a" | "b" =>`, `"a" if guard
/// =>`), read on code: an arm-like line in a literal or comment is not one.
pub fn match_arms(text: &str) -> Vec<String> {
    matches(text)
        .into_iter()
        .flat_map(|(_, arms)| arms.into_iter().flat_map(|arm| arm.strings))
        .collect()
}

/// The bracket that closes the `(`, `[` or `{` at `open` in blanked code.
fn matching(code: &str, open: usize) -> usize {
    let mut depth = 0i32;
    for (offset, byte) in code.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// The text of the bracketed block whose `(`, `[` or `{` is at `open`,
/// brackets included.
pub fn block(text: &str, open: usize) -> &str {
    let close = matching(&blank(text), open);
    &text[open..(close + 1).min(text.len())]
}

/// The top-level arguments of the call whose `(` is at `open`, as text.
pub fn call_arguments(text: &str, open: usize) -> Vec<String> {
    let code = blank(text);
    let close = matching(&code, open);
    split_depth0(&code, open + 1, close, ",")
        .into_iter()
        .map(|(from, to)| text[from..to].trim().to_owned())
        .filter(|argument| !argument.is_empty())
        .collect()
}

/// The names a `const NAME: &[&str] = &[...]` declaration lists, read with
/// the literal scanner (a `]` inside an item does not end the list).
pub fn string_list(text: &str, name: &str) -> Vec<String> {
    let code = blank(text);
    let Some(start) = code.find(&format!("const {name}: &[&str] = &[")) else {
        return Vec::new();
    };
    let open = start + code[start..].find("= &[").unwrap() + 3;
    let close = attribute_end(&code, open);
    literals(&text[open..close])
        .into_iter()
        .map(|lit| lit.text)
        .collect()
}

/// Whole-word occurrences of `word` in code (not in literals or comments),
/// whatever whitespace or path qualification surrounds it.
pub fn code_words(text: &str, word: &str) -> Vec<usize> {
    let code = blank(text);
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(word)
        .map(|(at, _)| at)
        .filter(|at| !code[..*at].ends_with(ident) && !code[at + word.len()..].starts_with(ident))
        .collect()
}
