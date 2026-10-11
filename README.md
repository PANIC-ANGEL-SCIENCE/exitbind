# Exitbind

**Keep important coding work moving. Make “done” checkable.**

A session ends, a reviewer takes over, or the agent says the fix is finished.
What still needs doing, and which result was actually checked?

Exitbind keeps the recorded goal, approved scope, current result and evidence
together. Your lead can pick up the remaining work, hand it to the right role,
and show what supports acceptance. It works alongside the Codex or Claude
conversation you already use; the host still runs the agent and controls its
tools and permissions. Exitbind's local records and acceptance logic need no
model, daemon or cloud service. Your host provides model access; configured
checks may use other tools or services.

[![CI workflow](docs/assets/badges/exitbind-black-glass-ci.svg)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/actions/workflows/ci.yml)
[![Latest release](docs/assets/badges/exitbind-black-glass-release.svg)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest)
[![License: MIT](docs/assets/badges/exitbind-black-glass-license.svg)](LICENSE)

[![Exitbind / Exit](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/PANIC-ANGEL-SCIENCE/exitbind)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest)

## Give your lead one link

Paste this into the Codex or Claude conversation you already use, then describe
the work in your own words:

```text
https://github.com/PANIC-ANGEL-SCIENCE/exitbind

Fix the interrupted-import bug, preserve existing imports, and show me the
checked result. Continue the same task if the session is interrupted.
```

On a URL-only first contact, your lead inspects the project and asks before
installation, project writes or permission changes. After approved setup,
continue in the same conversation; the lead retrieves recorded work and
available actions. The link supplies instructions, not proof of host compliance.
See the [onboarding guide](docs/onboarding.md).

For a change that matters, the supported path is:

1. **Agree on the task.** Your lead records the goal, boundaries and checks.
   The lead recommends review; you decide whether it is required or omitted.
2. **Do the work.** The worker gets the current assignment and project guidance.
   If the session is interrupted, inspect the saved state and resume what remains.
3. **Check and review the result.** Run the configured checks on the submitted
   result, then obtain independent review when required.
4. **Accept with evidence.** Your lead records its acceptance decision.
   Verify the receipt linking the accepted result to its evidence.

Your lead remains responsible for the requirements, useful checks, review
and affected users.
Use it when work spans sessions, needs a clear handoff or separate review.
Small reversible edits can stay in your usual agent and Git workflow.

## Install and try a first result

The current [v0.31.0 release](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/tag/v0.31.0)
provides Linux x86_64 and macOS Apple Silicon or Intel binaries. The installer
checks the archive checksum. Review the command, destination and
[release provenance](docs/public-history.md) before approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/v0.31.0/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

The installer puts the command in `$HOME/.local/bin`. Use
`exitbind host status` to inspect managed host setup.

Try the model-free example:

```sh
exitbind benchmark
```

The demonstration shows a checked result reaching acceptance in a disposable
Git project. It runs no agent and leaves your project untouched. Pass
`--output NEW_DIRECTORY` to retain its records.
See the [proof method](docs/value-proof-methodology.md).

From your project root, initialize Exitbind after approving project writes:

```sh
exitbind init --mode portable --root .
```

Setup creates a project configuration, private ignored state, reviewable role
profiles and project guidance. It does not start agents or grant host
permissions. Review the generated boundaries and worker/reviewer mapping. Empty
starter boundaries are valid; they do not grant access. If your host already
manages project guidance, use `--skip-skills`. Codex and Claude are exercised
setup paths; OpenCode compatibility is experimental.

The [onboarding guide](docs/onboarding.md) covers local storage, setup previews,
first-task boundaries and unsupported hosts. The [first checked run](docs/first-checked-run.md)
walks through an inspectable example.

## Continue interrupted work

Ask your lead to resume the existing task. Exitbind can return the current
assignment, applicable evidence and next steps from the configured project,
including a saved result that has not yet been returned. Supported native
handoffs deliver the current role, project rules and required evidence before
the next agent starts.

If a reply is lost, inspect the saved work before deciding whether to continue
or return an existing result. Uncertainty does not authorize repeating an
operation. A resumed task uses its current approved scope; an older task's
permission or check result does not carry over.

For goals with several requirements, `goal status` shows each requirement,
mapped work, current evidence and next action. Missing or stale support keeps
the goal open. Closure needs a current checked integration covering every
agreed item, with the selected review and Lead acceptance.
See [Complete a goal with several requirements](docs/goal-requirements.md) for
a reproducible three-item example.

For direct operation, the command reference documents `exitbind work resume`
and its recovery options. See [continuing native work](docs/continuing-native-work.md)
for supported host handoffs. Keep each older Work's compatible reader and
configuration when upgrading.

## Make completion trustworthy

**A reported “done” is not an accepted result.** A worker's completion, a
passing check, reviewer approval and your lead's acceptance are separate
events. In checked runs, Exitbind refuses acceptance when the configured
check result is missing or reports failure for the current worker artifact.
When the recorded result or configured tested inputs change, earlier evidence
remains historical and cannot stand in for current support.

The selected review route must be resolved before Lead acceptance.
You may explicitly omit review; an omission stays visible and is never
presented as approval. A successful checked task can produce a verifiable
receipt tying the accepted result to its checks, review decision and acceptance.

This gate controls Exitbind's own acceptance and `EXIT READY` state. It does
not control every action or completion message in your host. A receipt shows
what was accepted and why; it does not prove that the code meets every real-world
requirement. See [receipt limits](REFERENCE.md#exit-path-receipt-and-verification).

Exitbind's records are local and tamper-evident, not tamper-proof against
someone who can rewrite every local file. The host owns native tools and
permissions; Exitbind is not an operating-system sandbox. Goals, paths,
commands and returned files can contain private information. Keep records
private and read [Security](SECURITY.md) and the
[authority boundary](REFERENCE.md#authority-boundary).

## Intentional boundaries

**Approval.** Exitbind adds no second approval channel or human-only secret. The configured lead records scope and acceptance; the host controls which process may do so. One session may implement and submit the lead's reason. That limits independence; it does not replace required review. Exitbind does not invent requirements or mark missing items complete.

**Executable identity.** The installer checks the release archive. Archive attestations do not authenticate every later executable. `exitbind version --json` reports a digest, not a signature. Compare `executableSha256` with trusted evidence for those executable bytes. Startup does not require a match to the latest release. A scoped Work may retain an older or candidate executable.

**Other checkouts.** Resume uses saved work in the configured project. It does not scan other checkouts or detect overlapping uncommitted files. No scheduler or shared dirty-file database is added. Coordinate separate working trees outside Exitbind. Resuming does not copy an older task's permission or check result.

## Update or roll back

`exitbind update` refreshes the executable and managed host guidance. It does
not replace your custom profiles or settings. Before switching the working
version, keep each active Work's pinned executable at a stable path outside the
installer's temporary aside copies. Existing work keeps its original reader,
configuration and records. Rollback changes the executable selector only: an
older reader may refuse newer marked work. Keep a compatible newer reader for
those records, or keep their state separate. Never edit a record to make an
older reader accept it.

See [upgrade and rollback](docs/upgrade-and-rollback.md) for the supported
return path, and [legacy compatibility](docs/legacy-compatibility.md) for older
project and record formats.

## Read next

- [Onboarding and setup options](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Upgrade and rollback](docs/upgrade-and-rollback.md)
- [Command reference](REFERENCE.md) and [terminology](docs/glossary.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)

For command output, bounded context and recovery, see [command consumption](docs/command-consumption.md).
