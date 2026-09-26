# Exitbind

**A reported “done” is not an accepted result.**

The code can be current. The tests can have passed. The review can have
approved. All three can still belong to different results.

Exitbind is a local acceptance boundary for coding-agent work. Its `work check`
command runs the check fixed at the start of the task and records the outcome
against the exact result. Checks, required independent review, and Lead
acceptance must all belong to that result. Evidence taken on an earlier result
stays historical; it never opens the current door.

**No result exits unbound.**

```text
Without Exitbind
  Agent: "Done. Tests pass."
  You merge.
  Later: the tests ran on the agent's previous result.

With Exitbind
  Agent: "Done. Tests pass."
  Exitbind: EXIT BLOCKED (check_missing): no passing check belongs to the current result.
  Only a check, required review, and lead acceptance bound to the current result reach EXIT READY.
```

Current prerelease: `v0.25.0-rc.4`. One local binary; it calls no model and runs
no daemon or cloud service. This page can describe behavior that is newer than
that prerelease; the [changelog](CHANGELOG.md#unreleased) lists it.

[![Exitbind / Exit](https://github.com/envy404/exitbind/actions/workflows/ci.yml/badge.svg)](https://github.com/envy404/exitbind/actions/workflows/ci.yml)
[![Stable release](https://img.shields.io/github/v/release/envy404/exitbind)](https://github.com/envy404/exitbind/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## What it adds to “tests passed”

| An agent says | Exitbind records instead |
| --- | --- |
| “Tests pass.” | The frozen check command, run by Exitbind itself on the current result, with its real exit code or signal, duration, and hashed output logs. A check a host reports is labelled `reported`; one Exitbind ran is labelled `observed`. |
| “It's the latest code.” | A fingerprint of tracked and untracked, non-ignored project files, excluding Git metadata and Exitbind state, so uncommitted work is covered before anything is pushed. |
| “Review approved it.” | Worker completion, check, review, and Lead acceptance as separate, attributed events. An omitted review is recorded as an omission, never as approval. |
| “The reviewer asked for X, so I added it.” | In marked work, the finding as evidence and the Lead's recorded decision: `repair` inside a Lead-set boundary, `supersede` the basis, `defer`, or `reject`. Only the first two start another attempt; a deferred or rejected finding is never counted as approval. |
| “Nothing changed since.” | An append-only, hash-chained ledger: recorded artifact bytes equal disk bytes, or no new run event is written. |
| “Done.” | `EXIT READY`, `EXIT REFUSED`, or `EXIT BLOCKED` with a reason code, plus a verifiable receipt for accepted checked runs. |

Exitbind adds a local acceptance step to the agent loop, including uncommitted
results before a push. CI and branch protection keep their existing roles.

### When to use it

| Job | Practical choice | What it establishes |
| --- | --- | --- |
| A small reversible edit, or a task whose native handoff and CI are sufficient | Keep the existing agent, Git and CI workflow. | The checks that workflow actually ran; no Exitbind setup is needed. |
| A one-off check with no work to hand off or resume | Run that check in the existing host or CI. | The command's own exit status; Exitbind adds no record. |
| Material work needing a recoverable same-work handoff and acceptance tied to the current result | Use Exitbind in the existing agent host. | Recorded work, applicable check evidence, the owner's review decision and Lead acceptance under [Exitbind's authority boundary](REFERENCE.md#authority-boundary). |
| A series of material tasks in one project | Opt in to project memory and begin each task as new governed work. | The managed session hook gives each new session the Lead's role and currently accepted project rules; each work item still earns its own check, review, and acceptance. |

Task quality and operating cost compared with other workflows are unmeasured.

**As a default layer.** Exitbind can stay installed beside Codex or Claude
Code. The host keeps its models, sessions, subagents, tools, and permissions;
small reversible work stays direct; material work gains acceptance and
continuity that outlive one conversation. The footprint is a short bootstrap
skill, a session hook, and local files.

## Built for your coding agent

Small reversible work stays direct; the protocol surfaces only for material or
promotion-required work, resuming governed work, or explicit cross-host
continuation.

The coding agent you already use operates the CLI inside your existing
conversation. During a governed task it follows the recorded next action:

```text
exitbind work next WORK --full -> read the current assignment and constraints
(do the work)
exitbind work return WORK …    -> submit the assigned result
exitbind work check WORK       -> run the frozen check when it is the next action
```

- **Check evidence is captured.** Exitbind executes the frozen command and
  records its outcome and logs. Workers still report their results, and
  reviewers still supply their judgments.
- **Resume, don't reconstruct.** `work resume` rebuilds the next step from
  recorded state after a restart or context loss and returns the work last
  begun or focused; older running work stays history (`work resume --history`).
  Evidence that still belongs to the same result is kept.
- **Continue on another host.** `work continuation WORK` gives Codex or Claude
  Code in the same local workspace the original requirements, corrections,
  result references, and the bind and child commands; stale context is refused.
  See [cross-host continuation](docs/cross-host-continuation.md).
- **Capture subagent results from the host.** After `work child prepare`, the
  managed Claude Code subagent hooks record the child's host-reported ID and
  exact final message, so the parent never retypes it. Other hosts record the
  return with `work child`.
- **Small replies, safe to inspect.** `work check` and `work return` reply in
  at most 8 KiB of JSON with a read-only follow-up command as an argv array.
  When either records an event, the reply names it even if later cleanup fails,
  so the lookup reads it without re-running the check or resubmitting.
- **Hold a result while replanning.** When the iteration governor requires a
  replan or new evidence, a completed worker result is retained by content hash
  for later resubmission. Other refusals do not promise that retention.

See [work mutation results](docs/work-mutation-results.md) for the exact reply
contract. A successful recording operation does not mean the check passed;
the reply reports the check's exit code or signal separately.

## See a wrong door refused

This page describes the opt-in release candidate `v0.25.0-rc.4` for Linux x86_64
and macOS on Apple Silicon or Intel. The pinned installer places the executable
under `$HOME/.local/bin` and verifies the archive checksum; release archives
also carry GitHub build attestations. Review the command and destination before
approving installation.

```sh
curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/v0.25.0-rc.4/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"
```

That command installs the binary and, for each coding host it detects on this
machine, a small bootstrap skill and session hook so the lead you already use can
find Exitbind. It reports what it installed and what it could not, and never
overwrites a file it does not manage. Check it with `exitbind host status`.
Discovery is not activation: only a work handle, recorded check, review, or
acceptance shows that governed work actually started.

Then run the local demonstration:

```sh
exitbind benchmark
```

It creates a disposable Git project, shows a failed exact check refused, keeps
that failed attempt on record, and lets only a fresh checked result reach
acceptance. No model runs and your project is untouched. It is a synthetic
demonstration of the mechanism, not a claim about every agent, project, or
quality outcome; use `exitbind benchmark --output NEW_DIRECTORY` and the
[proof methodology](docs/value-proof-methodology.md) for inspectable records.

## Give your lead one link

You do not have to operate the protocol. Paste this into the Codex or Claude
conversation you already use, then describe the work in ordinary language:

```text
https://github.com/envy404/exitbind
```

On this URL-only path your lead inspects the project, explains the effect, and
asks before installation, project writes, or permission changes. After you
approve, it uses the local CLI plus project-local guidance:

- **Classify the work first.** Follow the [entry rule](#built-for-your-coding-agent);
  the lead classifies actual effects, and a filename alone does not require
  governance.
- **Material or promotion-required work** binds its evidence to the exact result
  before it can exit.
- **Resumed work** keeps evidence that still belongs to the same result, so a new
  session does not redo a valid check or review. Before relying on a saved
  packet, `work validate WORK --packet FILE` checks it against current state.

A pasted URL is guidance, not proof that a host followed it.
[See the complete onboarding path.](docs/onboarding.md)

## How material work exits

```text
one link -> classify -> exact-result check -> review decision
          -> applicable review -> lead acceptance -> verified receipt
```

These doors stay separate: worker completion is not a passing check, a passing
check is not reviewer approval, reviewer approval is not lead acceptance, and
a reviewer finding is not a requirement until the Lead decides it.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact. It rejects older evidence when a new worker result replaces it or covered project files change.

For important work, the Lead recommends independent review and the owner
decides. Record that choice at a new governed entry with
`--review-policy required` or `--review-policy omitted`; the owner can revise it
while work continues. A run started without `--review-policy` is the unmarked
historical path and keeps required-review semantics.

Configuration or profile changes after a run starts are reported as warnings
and the recorded plan continues; changed or substituted evidence bytes are
refused. For an accepted checked run, `exitbind receipt` emits a receipt and
`exitbind verify` checks it. A receipt covers recorded artifacts, not every
project file, and does not prove the work is correct; see
[receipts](REFERENCE.md#exit-path-receipt-and-verification).

## The Lead sets the Frame

For work where structure matters, the Lead fixes the shared decisions workers
must not silently reinterpret—ownership, interfaces, preserved behavior,
decisive cases, non-goals—and workers choose details inside it. A contradiction
goes back to the Lead. The independent reviewer can challenge the Frame itself.

Optional progress narration is computed by Exitbind:

```text
[Neuro] Exitbind progress: N%.
```

It describes the current run and does not change the host agent's identity or
authority. An accepted terminal run supplies the literal display value
`EXIT READY`, which hosts copy verbatim.

## Set up a project when needed

After approving project writes, initialize a portable project from its root:

```sh
exitbind init --mode portable --root .
```

This creates `exitbind.json`, private ignored state under `.exitbind/`,
reviewable role profiles, and project-local guidance for Codex and Claude. Add
`--skip-skills` if your host already manages project skills. Setup installs no
hook, launches no agent, and grants no host or OS permission. Codex and Claude
are exercised setup paths; OpenCode's compatible path is experimental. An
optional host plugin carries the skill only—see [setup options](docs/onboarding.md).
Review the generated boundaries and native worker/reviewer mapping before
project work; empty starter boundaries are valid configuration, not permission.

## Trust, data, and compatibility

Your host owns models, subagents, tools, processes, and permissions; your
repository host owns merges. Exitbind owns its local lifecycle and exit rules.
The [authority boundary](REFERENCE.md#authority-boundary) defines this once.

Exitbind does not authenticate a model's self-report or withstand an attacker who
can rewrite every local file: its ledger is tamper-evident, not tamper-proof.
Outside Git, the input fingerprint covers project files except Git metadata and
Exitbind state. Neither mode covers files outside the project, the environment,
or remote or time-dependent conditions; Git mode also excludes ignored files.
Raw ledgers and artifacts can contain
goals, commands, paths, and task results; keep them private and read
[SECURITY.md](SECURITY.md) before real work.

Historical run records are read under their original producer and schema
meaning, and old evidence is never relabelled as new. Existing projects retain
their historical paths. See the
[public format map](CHANGELOG.md#public-tags-and-format-readers) before a
rollback, and [legacy compatibility](docs/legacy-compatibility.md) for an old
project.

## Update, recover, or leave

Ask the same lead to update Exitbind, resume interrupted work, or remove it.
Direct operators can use:

```sh
exitbind update
exitbind work resume
exitbind version --json
```

`version --json` reports the running executable's SHA-256 and, for release
builds, the embedded build commit, for comparison with release evidence.

After updating from an earlier release, run `exitbind host install` once so the
host guidance matches the new binary. From this release on, `exitbind update`
uses the new binary to refresh that guidance. Before removing the binary,
remove optional hooks and review which project records to retain.
`rm "$HOME/.local/bin/exitbind"` does not delete project ledgers, receipts,
configuration, or projected skills; see [update and removal](REFERENCE.md#removal)
and [run recovery](docs/repair-a-run.md).

## Read next

- [Ask Codex or Claude to set it up](docs/onboarding.md)
- [First checked run](docs/first-checked-run.md)
- [Work mutation results](docs/work-mutation-results.md)
- [Repair or resume a run](docs/repair-a-run.md)
- [Continue one work item across hosts](docs/cross-host-continuation.md)
- [Command reference](REFERENCE.md)
- [Terminology](docs/glossary.md)
- [Optional memory, hooks, and receipts](docs/optional-surfaces.md)
- [Legacy compatibility](docs/legacy-compatibility.md)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [License](LICENSE)
