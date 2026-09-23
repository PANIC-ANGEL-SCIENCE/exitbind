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

## Public identity

Use `envy404` as the human-facing author, owner, and developer name.
Use `envy404` only as the GitHub account or repository URL slug. Direct
commits should use `envy404 <legacy-operator@users.noreply.github.com>`.
GitHub-generated squash metadata may use the account slug; `.mailmap` maps that
identity back to the canonical display name. Do not rewrite published history
to change older metadata.

## Development setup

Clone the repository and work from its root. Rustup automatically selects and,
when needed, installs the repository toolchain declared in
[`rust-toolchain.toml`](rust-toolchain.toml). This keeps contributor and CI
formatting and lint behavior aligned. The separate `rust-version` field in
`Cargo.toml` declares the minimum Rust version supported by the package.

The project has no required service, database, daemon, or model runtime.

Run the shared native CI sequence from any working directory:

```sh
./scripts/ci-local.sh
```

Use the script's absolute path when outside the checkout. On Linux it runs
`cargo fmt --check`, path and product-surface gates,
`cargo clippy --locked --all-targets -- -D warnings`, tests, value proof,
refusal demo, native tmux handoff, release references, release build,
onboarding smoke, packaging, and installer smoke. On macOS it runs the same
native test/proof/build/package/installer sequence as the macOS CI jobs.
Each stage stops on failure and prints its complete failure output.

Cargo is resolved from `CARGO`, then `PATH`, then the usual Cargo home. The
script adds that executable's directory to its own PATH; it does not change
shell settings. Install the repository Rust toolchain and, on Linux, tmux
before running. It never installs system packages. `CARGO_TARGET_DIR` defaults
to the checkout's `target`; relative overrides are relative to the checkout.
The script selects native builds; leave `CARGO_BUILD_TARGET` unset.

Set `VALUE_PROOF_BASE` to the intended comparison commit. CI supplies the PR
base or pre-push SHA; an empty/all-zero value uses `HEAD^`. The script requires
full published ancestry and fetches an explicitly supplied missing base from
`origin`. It does not silently substitute another comparison. `CANDIDATE_SHA`
can additionally select the contributing head for the ancestry check.

`--checks-only` stops before build/package steps; the release workflow uses it
before its tag, build, attestation, and publication gates. Local completion
covers only the selected native host and current working tree. GitHub's exact
checkout checks, the other OS jobs, dependency audit, WSL, and release
provenance checks remain separate; a local pass is not an exact-SHA CI pass.

Add the smallest focused test that proves changed behavior. Documentation links
and public wording contracts belong in the existing documentation tests. Do not
add a dependency when the standard library or an existing dependency is enough.

For vulnerabilities or sensitive reports, follow [SECURITY.md](SECURITY.md)
instead of opening a public issue. For ordinary bugs and proposals, include the
affected workflow, expected result, observed result, and the narrowest useful
reproduction.
