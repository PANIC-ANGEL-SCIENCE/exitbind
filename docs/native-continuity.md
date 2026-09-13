# Native conversation continuity

It verifies what you asked an agent to do and what came back, in the same
record.

Exitbind's host guidance preserves the existing root conversation. Loading a
role, rendering a brief, resuming a run, refreshing a skill, or looking up
governed memory adds bounded context. These operations do not authorize a new
root conversation, a reset, a fork, or unnecessary compaction. The host retains
control of its native conversation and any compaction it needs.

Recent user corrections and rejected approaches with their rationale remain
working context. Host/system instructions and frozen assignment boundaries
still apply. If a correction changes a frozen run, keep that correction visible
and refer the conflict to the existing lead. The existing
[supersession procedure](../REFERENCE.md#run-and-recovery) records an authorized
successor; it does not replace the host conversation or silently change the
predecessor.

## Conversation and durable memory

Native conversational recall comes from the host's conversation. Exitbind's
opt-in durable memory has separate role rights, review and lifecycle rules. A
ledger, profile or recalled memory item is bounded evidence, not a substitute
conversation. This guidance adds no transcript capture, reconstruction, chat
ingestion, embeddings, or shared memory.

Native subagents still receive the exact pending assignment under the
[attended handoff rule](codex-tmux-away.md). The root conversation stays active.
The away runner remains limited to an explicitly authorized operator-away
handoff; it is not a way to restore or replace an attended root conversation.

## Install the guidance

New portable and local projects receive the bundled skill through the existing
`exitbind init` path. After upgrading the binary, update an existing project's
managed copies explicitly:

```sh
exitbind init --refresh-skills --root CONTROL_ROOT
```

Replace `CONTROL_ROOT` with the directory containing the project's
`exitbind.json`: the project directory in portable mode, or its separate control
directory in local mode. Refresh preserves the existing ownership checks and
does not edit native host configuration or session files. Refreshing a skill
does not itself authorize replacing the active conversation; host discovery
and presentation remain host responsibilities.

## What the hook and tests establish

The optional `SessionStart` hook emits bounded project continuity context on
startup, resume, compact and clear events. When an active host integration
detects a newer public release, an update-only advisory context may also appear
even without a configured project; it asks the root agent to mention the
option, and model surfacing is advisory rather than proven. Project continuity
remains fail-open and silent for missing, malformed or unconfigured input. The
hook uses no transcript or session path from the payload, launches no host
process, and emits no lifecycle control. Ordinary turn events and SubagentStart
never receive update notices.

Executable regression tests check output shape and size, silent events,
preserved disposable host-file sentinels, host-command launch traps, and the
bytes installed or refreshed into portable/local skill copies. These checks
establish Exitbind's emitted guidance and the local behavior exercised. They
do not establish model compliance, long-session or post-compaction recall, a
historical root cause, or a repair for model-capacity warnings or host transport
failures.

## Optional host comparison

For an explicitly requested diagnostic, use a disposable workspace to compare
direct Codex with the same Codex installation through a terminal client, such
as Orca. Keep the CLI version, model, reasoning effort, configuration and work
sequence the same. Each condition owns a fresh native thread and keeps that
exact thread through a short sequence of code inspection, edits, checks and
discussion before checking recall of an earlier fact and rejected approach.

Record native thread/session IDs from the host, instruction-source identities,
context sizes, compaction events and token usage when available. A terminal tab
title alone does not establish native thread identity. Keep recent corrections
in the same conversation; do not reset an existing working thread or capture
its transcript for the comparison. Missing identity or isolation evidence
leaves the comparison inconclusive.

A selected-model capacity warning concerns availability and is separate from
the original continuity symptom. Keep capacity failures separate from recall
outcomes and wait for the selected model rather than changing it mid-comparison.
A client-specific difference warrants investigation; it does not establish
that Orca or another terminal client caused a historical failure. This is an
optional manual procedure, not an Exitbind diagnostic collector or a claim that
the comparison has been performed.
