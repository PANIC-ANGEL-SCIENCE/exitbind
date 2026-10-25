# Contributing to Exitbind

This is the canonical guide for human contributors and coding agents modifying
the Exitbind source. Give a contributing agent this file explicitly before it
edits the repository. Removing root `AGENTS.md` means hosts no longer discover
these developer instructions through that filename; `CONTRIBUTING.md` is not
an automatic host fallback.

To use Exitbind for your own work, start with the [README](README.md) and
[onboarding](docs/onboarding.md). Product use does not require contribution,
cloning this source, Rust, or this developer guide.

Keep changes bounded to one explainable problem and preserve Exitbind's core
authority boundaries: the host owns models, execution, and permissions; the
human owns the intended outcome and approvals; and Exitbind records bounded
handoffs and evidence. Discuss a proposed change before implementing it when it
would alter a public invariant, persisted format, trust boundary, or release
contract.

## Public identity

The canonical repository is `PANIC-ANGEL-SCIENCE/exitbind`. The maintainer's
public Git identity is `envy404 <287204764+envy404@users.noreply.github.com>`.
Contributors should retain their own attribution and may use the GitHub noreply
address shown in their account settings. Do not rewrite published history to
change metadata or replace the [public history baseline](docs/public-history.md).

## Development setup

Clone the repository and work from its root. Rustup automatically selects and,
when needed, installs the repository toolchain declared in
[`rust-toolchain.toml`](rust-toolchain.toml). This keeps contributor and CI
formatting and lint behavior aligned. The separate `rust-version` field in
`Cargo.toml` declares the minimum Rust version supported by the package.

The project has no required service, database, daemon, or model runtime.

Bind verification to the executable you actually invoke: record its absolute
path, `version --json` producer commit, independently computed SHA-256, and
acquisition class (`development` or authenticated `release`). Equal semantic
versions can contain different development or release bytes. A null commit is
`producer unresolved`; a digest mismatch is unresolved until explained.

For a development candidate, commit the selected source, verify the checkout is
clean, and build with `EXITBIND_BUILD_COMMIT=$(git rev-parse HEAD) cargo build
--locked`. This embeds an identity supplied by the builder; it authenticates no
source by itself. Label dirty builds as development snapshots, never as unchanged
bytes from their base commit. Keep the exact candidate path for subsequent checks.

Compare caller-selected values with the standard-library helper:

```sh
python3 scripts/verify-binary.py /absolute/path/to/exitbind \
  --class development --commit EXPECTED_FULL_COMMIT --sha256 EXPECTED_SHA256
```

The helper refuses a null producer or mismatched bytes even if the version
matches. For releases, first authenticate the expected values using release
checksums/attestations; `--class release` records acquisition intent and does not
authenticate a release. See [binary identity in onboarding](docs/onboarding.md#binary-identity).

Portability fixtures assert their Git topology before invoking Exitbind. Tests
that need no Git require a marker-free temporary directory and ancestors. Set
`EXITBIND_TEST_NO_GIT_ROOT` to an existing permitted marker-free directory when
the default temporary root inherits Git. If no such namespace is available,
`EXITBIND_TEST_NO_GIT_UNAVAILABLE=1` explicitly emits `COVERAGE UNAVAILABLE`
and exercises no product behavior for those cases; report them as untested,
separately from the mandatory enclosing-Git, Gitfile and refusal cases.

Run the shared native CI sequence from any working directory:

```sh
./scripts/ci-local.sh
```

Workflow syntax/context validation runs before Rust builds. It uses checksum-
pinned actionlint 1.7.12 from a task-local archive cache, with ambient shell/Python
analyzers disabled. For first acquisition, explicitly set
`EXITBIND_FETCH_VALIDATOR=1`; hosted jobs opt in. Offline contributors provision
the matching verified archive in `EXITBIND_WORKFLOW_CACHE`; an absent or corrupt
archive refuses the gate. No global validator installation is required.

Use the script's absolute path when outside the checkout. Every run starts with
the no-build repository-rule guard
(`tooling/repository/ci/check-repository-rules.sh`): the size budget in
pre-return, path rendering, product surface and release references on Linux,
and public history and commit identity everywhere fail before any build
starts; full mode fetches the comparison base first. On Linux it then runs
`cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
tests, value proof, drift-warning demo, native tmux handoff, release build,
onboarding smoke, packaging, and installer smoke. On macOS it runs the same
native test/proof/build/package/installer sequence as the stable release jobs.
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
base or pre-push SHA; an empty/all-zero value uses `HEAD^`. Any other value
that is not a full commit SHA, such as `pub/main`, names a ref, which the script
resolves once to `git merge-base HEAD <ref>`. The script requires
full published ancestry and fetches an explicitly supplied missing base from
`origin`. It does not silently substitute another comparison. `CANDIDATE_SHA`
can additionally select the contributing head for the ancestry check.

`--checks-only` stops before build/package steps; the release workflow uses it
before its tag, build, attestation, and publication gates. Local completion
covers only the selected native host and current working tree. GitHub's exact
checkout checks and dependency audit remain separate. Routine CI uses Linux;
[stable readiness](docs/prereleases.md) checks both Macs and WSL before a
separately selected main promotion. Release provenance stays separate; a local
pass is not an exact-source hosted result.

Add the smallest focused test that proves changed behavior. Documentation links
and public wording contracts belong in the existing documentation tests. Do not
add a dependency when the standard library or an existing dependency is enough.

Run a selected integration target with the shared verification preparation:

```sh
./scripts/test-focused.sh --test project_user_surface setup_pre_return
./scripts/test-focused.sh --test work_admission_return pre_return::selected_test_name -- --exact
```

Run every target a change affects, each with the features it needs:

```sh
./tooling/repository/ci/test-affected.sh --base origin/main --list
./tooling/repository/ci/test-affected.sh --base origin/main
```

Changed files are the diff against the base plus untracked files. A changed
`src` file selects the targets whose `covers` prefixes in
`tooling/repository/checks/test-target-map.json` match it, a changed test file
selects its own target, a file the binary embeds (`include_str!`, for example
a schema or skill body) selects the targets covering its embedding module, and
`Cargo.toml`, `Cargo.lock` or `tests/support` select all targets. Each target runs with its declared `features` and without
host session variables, prints its exact command, and must run at least one
test. Pre-return runs it as the `affected-tests` stage against
`PRE_RETURN_BASE`, else `VALUE_PROOF_BASE`.

Focused selectors use the new target name followed by the module-qualified test path. Repository-only checks are included in focused and full developer CI with the `repository-checks` feature; distributed Cargo source tests omit that feature. Focused, pre-return and full checks share `scripts/verification-context.sh`.
It removes ambient native Goal and Git selectors only for these owned checks,
preserves the selected command arguments, uses a checkout-owned external cache,
and retains complete output and child status. Focused selection requires at
least one passing test; an empty or entirely ignored selection fails. Synthetic
fixtures may add their own isolated identity after preparation. Real human Work
and native delegation retain their conversation context. On interruption, the
capture forwards the signal to its owned process group and drains output before
releasing the cache lock; an unrecorded outcome remains unknown.

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

Read `README.md` before changing the project.

## Purpose

Exitbind is a local acceptance boundary for material coding-agent work. It binds
an exact result to check evidence, independent review, lead acceptance, and a
verifiable receipt. It is not a model runtime or a general autonomous-agent
framework.

## Public surface

When a user gives only `https://github.com/PANIC-ANGEL-SCIENCE/exitbind`, inspect this project and
the host capabilities, decide whether the governed path fits, and ask before
installation, project writes, permission changes, or another owner-controlled
action. After authorization, operate selectively inside the existing
conversation. Text guidance does not prove host or model compliance.

- Open public explanations with the supported work path: applicable goals and
  evidence, useful execution and continuation, then trustworthy completion.
  Explain exact-result acceptance as a trust mechanism within that path.
- Keep the default path legible as `one link -> classify -> exact-result check
  -> review decision -> applicable review -> lead acceptance -> verified
  receipt`; the Lead recommends review, the owner decides and may revise, and
  an omission is never approval. Group commands as operator details without
  hiding limitations.
- New governed entry must record the owner choice with
  `--review-policy required` or `--review-policy omitted`. An unmarked run is
  the historical path with required-review semantics; it cannot later use
  `run review-policy`.
- Every public demo must run in CI and prove only the check it exercises.
  Current releases do not compare all product changes with the declared write
  boundary, so never claim or demonstrate that they do.
- Keep one invariant aligned in README, CLI errors, run transitions, and CI:
  recorded artifact bytes equal disk bytes, or no new run event is written.
- Define each authority boundary once. Elsewhere link to that definition or let
  a focused test carry it; do not restate it for emphasis.
- Public artifacts show one accurate refusal instead of advertising a list of
  absent features or non-guarantees.

## Non-negotiable constraints

- A normal coding-agent session must work when Exitbind is absent or disabled.
- No daemon, cloud service, database, model call, telemetry, or background
  process may become required.
- Advisory use must not wrap or delay normal development commands.
- Strict enforcement must be explicit and limited to boundaries Exitbind
  actually controls. Never imply prompt text is an OS sandbox.
- Persistent memory is opt-in, role-scoped, reviewable, expiring, and revocable.
- Never create shared canonical memory or automatically ingest chat history.
- One lead owns the result. An orchestrator may coordinate only the active task
  and gains no implicit access to agents' role-scoped memories.
- Prefer a host's native subagent and review mechanisms. Do not rebuild model
  loops, provider clients, or process schedulers merely for uniformity.
- Delegation must name the profile, purpose, allowed scope, dependencies,
  verification, and return contract. Profile loading must be evidenced.
- Receipts record selected configuration/profile bytes and requested runtime
  metadata, not observed actions, hidden reasoning, secrets, or unnecessary
  command output.
- Reuse host capabilities and established formats before adding an adapter or
  dependency. dotagents integration must remain optional.

## Complexity ladder

Before adding a feature, stop at the first sufficient option:

1. Do users need it to satisfy a tested workflow?
2. Does Codex, Claude Code, dotagents, Git, or the OS already provide it?
3. Can a documented convention solve it?
4. Can the standard library solve it?
5. Only then add the smallest implementation.

Do not reduce validation at trust boundaries, deletion safety, privacy, or the
small verification needed to prove behavior.

## Change discipline

- Each file must have one explainable responsibility.
- Every file under `src/`, `tests/` and `tests/fixtures/` stays under 32 KiB.
  A file listed in `tests/fixtures/architecture-budget.txt` may not grow, and
  its ceiling falls with it. Extract the responsibility you are changing
  instead of adding policy to a large facade, reducer, router or test file.
- Keep the CLI entry point as wiring and dispatch only.
- `config::validate` owns configuration acceptance. New or modified code must
  use `Loaded::agent` and typed fields instead of indexing `Loaded.config`
  directly; retain the raw value and source bytes only for compatibility and
  evidence hashing.
- Prefer plain TOML/JSON/JSONL files over a storage service.
- Add no dependency without documenting why native and standard-library options
  are insufficient.
- Converting a path to a string for a decision, command, identifier, or recorded
  evidence must fail on non-UTF-8 input. Lossy conversion is permitted only in
  human-facing error or display text. Path resolution failures must return an
  error; a missing final leaf may be rendered only after its parent is
  canonicalized.
- Every user-visible command needs one focused test or fixture.
- For the full native CI sequence, use `scripts/ci-local.sh` rather than
  assembling the commands again. See [CONTRIBUTING.md](CONTRIBUTING.md) for
  prerequisites, proof-base selection, and checks that remain host-specific.
- For a historical failed GitHub Actions run, use `scripts/ci-failure.sh RUN_ID`
  to see failed steps and bounded error lines. Keep full remote logs out of
  agent context; record the run ID, head SHA, failing step, and diagnosis.
  Record a real gate failure, remote or local, with `scripts/ci-failure.sh
  --record --gate GATE --class CLASS --summary TEXT --resolution TEXT
  (--run RUN_ID | --source TEXT)`; it appends to
  `tooling/repository/ci/failure-ledger.jsonl`. Every `ci-local.sh` gate is
  listed in `tooling/repository/ci/gate-inventory.json` with its cost, the
  failure it prevents, and a ledger reference, provisional date or deterrent
  reason; `check_gate_inventory.py` enforces this in the `ci-stage-contract`
  stage on every platform, reports a provisional date that has passed, and
  `release_integrity` refuses a release while one has.
- For noisy verification in an agent session, use
  `scripts/quiet-command.sh LABEL COMMAND [ARG ...]`: ordinary success remains
  one labeled line, while recognized successful `cargo test` output is reduced
  to test counts and any warning or unclassified lines are retained verbatim.
  The child exit status is preserved. Captured output keeps combined, stdout,
  and stderr originals with private argv, working-directory, producer hash,
  Git source fingerprint (when available), status, and stream digest metadata;
  existing paths and symlinks are never overwritten. Read a retained
  stream without rerunning its command with
  `python3 scripts/quiet-command.py read LOG [--stream combined|stdout|stderr]
  [--offset N] [--limit N]` (each read is capped at 64 KiB). The wrapper changes
  presentation, never the command or its acceptance evidence.
- Measure startup and command latency before claiming the tool is lightweight.
- Update the plan when scope, authority, or a stop gate changes.
- Follow the [release series and compatibility policy](docs/release-version-policy.md).
  Compatible features and fixes share its pre-1.0 series. Review changed public
  invariants, newly rejected counterexamples and intentional compatibility breaks
  against that policy before selecting a version.
- Run `scripts/review-inventory.sh PREVIOUS_RELEASE` before a repository review.
  A second occurrence of a finding must add the mechanical gate that would have
  caught it; on a third occurrence, failure to adopt the gate is the finding.

## Historical maintenance evidence

The [documentation index](docs/index.md) separates current contributor guidance,
product reference, design notes, and public evidence. Archived maintenance
records are not current contributor instructions.
