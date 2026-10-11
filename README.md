# Exitbind

**Help your coding agent start the right task, continue when work is interrupted, and show what was checked.**

Exitbind works with the Codex or Claude conversation you already use. For
important changes, it keeps the goal, approved boundaries, work and evidence
together so you can inspect the result and decide when it is ready. Your host
still runs the agent and controls its tools and permissions.
Local records and checks need no model, daemon or cloud service. Your host
provides model access when an agent needs it.

[![Exitbind / Exit](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/PANIC-ANGEL-SCIENCE/exitbind)](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Give your lead one link

Paste this into the Codex or Claude conversation you already use, then describe
the work in your own words:

```text
https://github.com/PANIC-ANGEL-SCIENCE/exitbind

Fix the interrupted-import bug, preserve existing imports, and show me the
checked result. Continue the same task if the session is interrupted.
```

On a URL-only first contact, your lead inspects the project and asks before
installation, project writes or permission changes. Once setup is approved,
you can keep working in the same conversation. You do not need to carry work
IDs or record files between messages. A link gives your lead instructions; it
does not prove that a host followed them. See the [onboarding guide](docs/onboarding.md).

For a change that matters, Exitbind helps your lead keep the important pieces
together:

1. what you want done and which files or commands are in scope;
2. the current project guidance and relevant evidence;
3. the agent's result and any supported way to resume the same work;
4. the check result, review decision and your lead's acceptance decision.

Your lead remains responsible for the task's meaning and affected users. Exitbind
can show when required information or evidence is missing, but it cannot fill
in missing requirements or make the first implementation correct by itself.
Use it when a change needs a clear handoff, may span sessions, or benefits from
separate implementation and review. Small reversible edits can stay in your
usual agent and Git workflow.

## Install and try a first result

The published v0.31.0 release supports Linux x86_64 and macOS on Apple Silicon
or Intel. The versioned installer below uses that release. The installer checks
the archive checksum;
release archives also carry GitHub build attestations. Review the command and
destination before approving installation.

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

This model-free demonstration creates and removes a disposable Git project and
shows a checked result reaching acceptance. It runs no agent and does not touch
your project. To keep its records for inspection, pass `--output NEW_DIRECTORY`.
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

When the task boundaries, check and review choice are already approved, your
lead can preview them with `setup` and apply the same facts once. Setup returns
the exact `work begin --detail` command; its response delivers the current actor,
full role and rules, evidence and executable next action. Follow that action
rather than assembling another command. See [approved setup facts](docs/onboarding.md#assemble-approved-facts-without-a-configuration-join).

The [onboarding guide](docs/onboarding.md) covers local storage, setup previews,
first-task boundaries and unsupported hosts. The [first checked run](docs/first-checked-run.md)
walks through an inspectable example.

## Continue interrupted work

For a goal with several requirements, ask your lead to retain the agreed source
and meanings, assign useful work, and resume the remaining items. `goal status`
shows each requirement's revision, mapped work, current evidence and next action.
Missing or stale support keeps the goal open. Final closure needs a current
checked integration covering every agreed item, with the selected review and
Lead acceptance. See [Complete a goal with several requirements](docs/goal-requirements.md)
for the supported commands and a reproducible three-item example. This capability
is included in the current release. Keep each older Work's compatible reader and
configuration when upgrading.

Ask your lead to resume the existing task in the same conversation. Exitbind
can provide the current task details and available next steps, including a
saved result that has not yet been returned. The lead still decides how to
handle uncertain work; uncertainty does not authorize running an operation
again. A resumed task uses its current approved scope. It does not inherit an
older task's permission or check result.
When a reply is lost, the lead can inspect the saved work before deciding
whether to continue, return an existing result or ask you what should happen.

For direct operation, the command reference documents `exitbind work resume`
and its recovery options. See [continuing native work](docs/continuing-native-work.md)
for supported host handoffs.

## Make completion trustworthy

**A reported “done” is not an accepted result.** A worker's completion, a
passing check, reviewer approval and your lead's acceptance are separate
events. In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.
When the result or covered files change, earlier check evidence remains historical.

Your lead recommends whether a change needs independent review. You decide
whether review is required, and an explicit omission stays an omission. A
successful checked task can produce a receipt that records the result and its
evidence. The receipt helps you inspect what was accepted; it does not prove
that the code is correct. See [receipt limits](REFERENCE.md#exit-path-receipt-and-verification).

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
