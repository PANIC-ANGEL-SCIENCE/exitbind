#!/bin/sh
set -eu

previous="${1:?usage: review-inventory.sh <previous-release-ref>}"
git rev-parse --verify "${previous}^{commit}" >/dev/null

count_ref() {
  git grep -E -o "$2" "$1" -- 'src/*.rs' 2>/dev/null |
    wc -l |
    tr -d ' '
}

count_worktree() {
  git grep -E -o "$1" -- 'src/*.rs' 2>/dev/null |
    wc -l |
    tr -d ' '
}

echo "== 1. Pattern deltas (previous -> current) =="
while IFS=: read -r label pattern
do
  before=$(count_ref "$previous" "$pattern")
  after=$(count_worktree "$pattern")
  printf '%-26s %s -> %s\n' "$label" "$before" "$after"
done <<'PATTERNS'
lossy_or_defaulted_path:to_string_lossy|to_str\(\)\.unwrap_or|unwrap_or\("\."\)|PathBuf::from\("\."\)
panic_sites:unwrap\(\)|expect\(|panic!|unreachable!
unsafe_blocks:unsafe[[:space:]]
dynamic_json:\bValue\b
exit_code_handling:code\(\)\.unwrap_or
todo_markers:TODO|FIXME|XXX
PATTERNS

echo
echo "== 2. Prior fixes that may have regressed =="
echo "Read each diff, extract the removed pattern, and search current siblings."
git log --oneline --regexp-ignore-case --grep='^fix' --grep='^security' -- 'src/*.rs' |
  head -20

echo
echo "== 3. Normative claims to check against code =="
grep -rhoE '[A-Z][^.]{15,140}\b(never|only|must|cannot|always|fail[- ]closed|canonical|refuses|is not)\b[^.]{0,100}\.' README.md SECURITY.md AGENTS.md docs/*.md 2>/dev/null |
  sort -u ||
  true

echo
echo "== 4. New and heavily changed modules =="
git diff --stat "$previous" -- 'src/*.rs'

echo
echo "== 5. Authority and side-effect surface =="
git grep -n -E -e 'Command::new|std::process|unsafe[[:space:]]|env::var|fs::remove|fs::rename' -- 'src/*.rs' ||
  true
