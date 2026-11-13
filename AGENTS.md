# Reading source

Before reading a Rust source file under `src/`, read its first eight lines.
Its opening header identifies what it owns, what it refuses or limits, and
where to look next. Keep detailed documentation after that short header.

The repository source inventory covers every Rust file under `src/`, including
test-gated files. Use a real, named action and record the files actually opened
when measuring a code-change reading path. Do not split a file merely for size.
