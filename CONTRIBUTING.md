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
drift-warning demo, native tmux handoff, release references, release build,
onboarding smoke, packaging, and installer smoke. On macOS it runs the same
native test/proof/build/package/installer sequence as the macOS CI jobs.
Each stage announces its name and log path before it starts. Output stays
quiet on success. Failure prints the last 8 KiB; complete stdout/stderr remain
in the private per-run log directory printed at startup. Inspect that stage's
log before retrying; the final console line is not necessarily the failed test.
`state` shows the current stage, `stages` records exit codes and durations,
`STAGE.log.exit` and `run.exit` retain command and script outcomes. A missing exit
file means completion has not been recorded, not success. `context` records
checkout/target paths, HEAD, dirty paths, tool versions, and comparison base;
it is diagnostic context, not an immutable acceptance receipt.

Logs live under the external target's private `ci-local-runs` directory and are retained until
you remove the selected completed run. They may contain private command output;
inspect them before sharing. Do not delete a running run's logs or cache.

Cargo is resolved from `CARGO`, then `PATH`, then the usual Cargo home. The
script adds that executable's directory to its own PATH; it does not change
shell settings. Install the repository Rust toolchain and, on Linux, tmux
before running. It never installs system packages. `CARGO_TARGET_DIR` defaults
to a checkout-specific directory under the temporary root; relative overrides
are relative to the checkout and must resolve outside it. Target, temporary
fixtures and `CI_LOG_ROOT` are validated before a build or log is created.
Run `scripts/ci-local.sh --preflight` to inspect exact Cargo/toolchain,
selected paths and live disk/memory observations without starting a check.
The script selects native builds; leave `CARGO_BUILD_TARGET` unset.

Each target directory is associated with its canonical checkout path on first
use. Later `ci-local.sh` runs from a different checkout must select a separate
`CARGO_TARGET_DIR`. Avoid sharing targets with temporary worktrees: compiled
tests can embed a fixture path from a checkout that is later removed. The
association does not validate a cache previously populated by other commands.
For an already mixed cache, select a fresh target once and reuse it for that
checkout; do not delete another session's cache or create a fresh one per retry.
A target lock prevents concurrent runs of this script. Direct Cargo commands
and older script processes do not honor it. On an interrupted run, inspect the
lock's `owner` record and actual process state before removing a stale lock;
the script never removes another run's lock automatically.

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

For a changed Work consumer, check the ordinary readable route and native
delivery together, including compact fallback, historical/held results,
cross-host corrections, and current goal status. Keep help, command reference,
generated profiles, and the packaged skill aligned with the canonical source.
Run the affected fixtures before the full sequence; a provisional impact map
does not justify omitting a consumer discovered during implementation.

For vulnerabilities or sensitive reports, follow [SECURITY.md](SECURITY.md)
instead of opening a public issue. For ordinary bugs and proposals, include the
affected workflow, expected result, observed result, and the narrowest useful
reproduction.

## Selected maintenance assignment

For the explicitly assigned v0.25.1 reviewer-handoff and R14 closeout, use
[the maintenance entry](docs/maintenance/v0.25.1/README.md). It is contributor
work, not an installed-user workflow or an automatic instruction for every task.
