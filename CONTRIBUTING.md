# Contributing to Exitbind

This guide is for human contributors. If you use a coding agent to modify this
repository, also give it the repository-specific instructions in
[AGENTS.md](AGENTS.md).

Keep changes bounded to one explainable problem and preserve Exitbind's core
authority boundaries: the host owns models, execution, and permissions; the
human owns the intended outcome and approvals; and Exitbind records bounded
handoffs and evidence. Discuss a proposed change before implementing it when it
would alter a public invariant, persisted format, trust boundary, or release
contract.

## Development setup

Clone the repository and work from its root. Rustup automatically selects and,
when needed, installs the repository toolchain declared in
[`rust-toolchain.toml`](rust-toolchain.toml). This keeps contributor and CI
formatting and lint behavior aligned. The separate `rust-version` field in
`Cargo.toml` declares the minimum Rust version supported by the package.

The project has no required service, database, daemon, or model runtime.

Before submitting a change, run the checks used by primary CI:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Add the smallest focused test that proves changed behavior. Documentation links
and public wording contracts belong in the existing documentation tests. Do not
add a dependency when the standard library or an existing dependency is enough.

For vulnerabilities or sensitive reports, follow [SECURITY.md](SECURITY.md)
instead of opening a public issue. For ordinary bugs and proposals, include the
affected workflow, expected result, observed result, and the narrowest useful
reproduction.
