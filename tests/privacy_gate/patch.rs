//! Conservative mapping from a full-index Git patch to its two source blobs.

const MAX_HUNK_LINES: usize = 4096;
const MAX_HUNK_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy)]
struct Header<'a> {
    old_path: &'a str,
    new_path: &'a str,
    old_oid: Option<&'a str>,
    new_oid: Option<&'a str>,
    index_seen: bool,
    old_marker: bool,
    new_marker: bool,
    old_regular: bool,
    new_regular: bool,
}

pub(super) struct Line<'a> {
    pub(super) marker: u8,
    pub(super) text: &'a str,
    pub(super) patch_offset: usize,
    pub(super) old: Option<usize>,
    pub(super) new: Option<usize>,
    pub(super) no_newline: bool,
}

pub(super) struct Hunk<'a> {
    pub(super) old_oid: Option<&'a str>,
    pub(super) new_oid: Option<&'a str>,
    pub(super) old_python: bool,
    pub(super) new_python: bool,
    pub(super) lines: Vec<Line<'a>>,
}

struct Active<'a> {
    hunk: Hunk<'a>,
    old_next: usize,
    new_next: usize,
    old_left: usize,
    new_left: usize,
    bytes: usize,
    valid: bool,
}

fn plain_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        && !path.chars().any(|character| {
            character.is_whitespace() || character.is_control() || matches!(character, '\\' | '"')
        })
}

fn header(line: &str) -> Option<Header<'_>> {
    let mut parts = line.split(' ');
    if parts.next()? != "diff" || parts.next()? != "--git" {
        return None;
    }
    let old = parts.next()?.strip_prefix("a/")?;
    let new = parts.next()?.strip_prefix("b/")?;
    if parts.next().is_some() || old != new || !plain_path(old) || !plain_path(new) {
        return None;
    }
    Some(Header {
        old_path: old,
        new_path: new,
        old_oid: None,
        new_oid: None,
        index_seen: false,
        old_marker: false,
        new_marker: false,
        old_regular: false,
        new_regular: false,
    })
}

fn oid(value: &str) -> Option<Option<&str>> {
    if !matches!(value.len(), 40 | 64)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    Some(value.bytes().any(|byte| byte != b'0').then_some(value))
}

fn index<'a>(line: &'a str, header: &mut Header<'a>) -> Option<()> {
    let mut fields = line.split(' ');
    if fields.next()? != "index" {
        return None;
    }
    let (old, new) = fields.next()?.split_once("..")?;
    if old.len() != new.len() {
        return None;
    }
    header.old_oid = oid(old)?;
    header.new_oid = oid(new)?;
    if let Some(mode) = fields.next() {
        if !matches!(mode, "100644" | "100755") || fields.next().is_some() {
            return None;
        }
        header.old_regular = header.old_oid.is_some();
        header.new_regular = header.new_oid.is_some();
    }
    header.index_seen = true;
    Some(())
}

fn marker(line: &str, prefix: &str, path: &str, oid: Option<&str>) -> bool {
    match line.strip_prefix(prefix) {
        Some("/dev/null") => oid.is_none(),
        Some(value) => oid.is_some() && value == path,
        None => false,
    }
}

fn range(value: &str, prefix: char) -> Option<(usize, usize)> {
    let value = value.strip_prefix(prefix)?;
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    if start.is_empty()
        || count.is_empty()
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || !count.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let start: usize = start.parse().ok()?;
    let count: usize = count.parse().ok()?;
    if (count != 0 && start == 0) || start.checked_add(count).is_none() {
        return None;
    }
    Some((start, count))
}

fn active<'a>(line: &str, header: Header<'a>) -> Option<Active<'a>> {
    if !header.index_seen
        || !header.old_marker
        || !header.new_marker
        || (header.old_oid.is_some() && !header.old_regular)
        || (header.new_oid.is_some() && !header.new_regular)
    {
        return None;
    }
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let mut parts = ranges.split(' ');
    let (old_start, old_count) = range(parts.next()?, '-')?;
    let (new_start, new_count) = range(parts.next()?, '+')?;
    if parts.next().is_some() || (old_count == 0 && new_count == 0) {
        return None;
    }
    Some(Active {
        hunk: Hunk {
            old_oid: header.old_oid,
            new_oid: header.new_oid,
            old_python: header.old_path.ends_with(".py"),
            new_python: header.new_path.ends_with(".py"),
            lines: Vec::new(),
        },
        old_next: old_start,
        new_next: new_start,
        old_left: old_count,
        new_left: new_count,
        bytes: 0,
        valid: true,
    })
}

impl<'a> Active<'a> {
    fn push(&mut self, line: &'a str, patch_offset: usize) -> Option<bool> {
        let marker = *line.as_bytes().first()?;
        let (old, new) = match marker {
            b' ' => {
                if self.old_left == 0 || self.new_left == 0 {
                    return None;
                }
                self.old_left -= 1;
                self.new_left -= 1;
                let numbers = (Some(self.old_next), Some(self.new_next));
                self.old_next += 1;
                self.new_next += 1;
                numbers
            }
            b'-' => {
                if self.old_left == 0 {
                    return None;
                }
                self.old_left -= 1;
                let number = self.old_next;
                self.old_next += 1;
                (Some(number), None)
            }
            b'+' => {
                if self.new_left == 0 {
                    return None;
                }
                self.new_left -= 1;
                let number = self.new_next;
                self.new_next += 1;
                (None, Some(number))
            }
            _ => return None,
        };
        self.bytes = self.bytes.saturating_add(line.len());
        if self.bytes > MAX_HUNK_BYTES || self.hunk.lines.len() >= MAX_HUNK_LINES {
            self.valid = false;
            self.hunk.lines.clear();
        }
        if self.valid {
            self.hunk.lines.push(Line {
                marker,
                text: &line[1..],
                patch_offset,
                old,
                new,
                no_newline: false,
            });
        }
        Some(self.old_left == 0 && self.new_left == 0)
    }

    fn mark_no_newline(&mut self) {
        match self.hunk.lines.last_mut() {
            Some(line) if !line.no_newline => line.no_newline = true,
            _ => self.valid = false,
        }
    }
}

pub(super) fn visit_hunks<'a>(patch: &'a str, mut visit: impl FnMut(&Hunk<'a>)) {
    let mut current = None;
    let mut hunk: Option<Active<'a>> = None;
    let mut pending: Option<Active<'a>> = None;
    let mut pending_markers = 0_usize;
    let mut offset = 0;
    for part in patch.split_inclusive('\n') {
        let line = part.strip_suffix('\n').unwrap_or(part);
        if pending.is_some() {
            if line == "\\ No newline at end of file" && pending_markers < 2 {
                pending_markers += 1;
                pending.as_mut().unwrap().mark_no_newline();
                offset += part.len();
                continue;
            }
            let boundary =
                line.starts_with("diff --git ") || line.starts_with("@@ ") || line.is_empty();
            if boundary {
                let complete = pending.take().unwrap();
                if complete.valid {
                    visit(&complete.hunk);
                }
                if line.is_empty() {
                    current = None;
                }
            } else {
                // Extra or malformed hunk content must not grant an exception.
                pending = None;
                current = None;
                offset += part.len();
                continue;
            }
        }
        if line.starts_with("diff --git ") {
            current = header(line);
            hunk = None;
        } else if let Some(active) = hunk.as_mut() {
            if line == "\\ No newline at end of file" {
                active.mark_no_newline();
            } else {
                match active.push(line, offset) {
                    Some(true) => {
                        pending = hunk.take();
                        pending_markers = 0;
                    }
                    Some(false) => {}
                    None => {
                        current = None;
                        hunk = None;
                    }
                }
            }
        } else if let Some(item) = current.as_mut() {
            if line.starts_with("index ") {
                if item.index_seen
                    || item.old_marker
                    || item.new_marker
                    || index(line, item).is_none()
                {
                    current = None;
                }
            } else if line.starts_with("new file mode ") {
                item.new_regular = matches!(line, "new file mode 100644" | "new file mode 100755");
            } else if line.starts_with("deleted file mode ") {
                item.old_regular = matches!(
                    line,
                    "deleted file mode 100644" | "deleted file mode 100755"
                );
            } else if line.starts_with("old mode ") {
                item.old_regular = matches!(line, "old mode 100644" | "old mode 100755");
            } else if line.starts_with("new mode ") {
                item.new_regular = matches!(line, "new mode 100644" | "new mode 100755");
            } else if line.starts_with("--- ") {
                let expected = format!("a/{}", item.old_path);
                if !item.index_seen
                    || item.old_marker
                    || item.new_marker
                    || !marker(line, "--- ", &expected, item.old_oid)
                {
                    current = None;
                } else {
                    item.old_marker = true;
                }
            } else if line.starts_with("+++ ") {
                let expected = format!("b/{}", item.new_path);
                if !item.index_seen
                    || !item.old_marker
                    || item.new_marker
                    || !marker(line, "+++ ", &expected, item.new_oid)
                {
                    current = None;
                } else {
                    item.new_marker = true;
                }
            } else if line.starts_with("@@ ") {
                hunk = active(line, *item);
                if hunk.is_none() {
                    current = None;
                }
            } else if item.old_marker || item.new_marker {
                current = None;
            }
        }
        offset += part.len();
    }
    if patch.ends_with('\n') {
        if let Some(complete) = pending {
            if complete.valid {
                visit(&complete.hunk);
            }
        }
    }
}
