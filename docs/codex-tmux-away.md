# Codex + tmux away runner

Exitbind's optional reference adapter keeps one already-authorized pending
Codex assignment alive when the operator explicitly disconnects. Exitbind
still owns assignment and artifact evidence; tmux owns only process lifetime.

## Attended sessions use native spawn

Do not select this adapter during an attended active session. Every
implementation worker and reviewer instead uses the host's native subagent
spawn with the assignment's exact `nativeTaskName`. If native spawn is
unavailable, stop and return the pending assignment to the operator; do not
fall back to shell `codex exec` or `exitbind away`. The explicit
operator-away/disconnect handoff below remains available.

[openai/codex#31894](https://github.com/openai/codex/issues/31894) is a strong
external symptom match for affected `codex exec` no-result runs, not a proven
root cause. Until a later repository change retires this temporary rule after
the upstream resolution is independently verified on a supported CLI, exclude
affected `codex exec` no-result samples from provider-native completion and
token-efficiency baselines. Historical evidence remains in place; quarantine
does not delete or rewrite it.

## Contract

- The adapter starts only an exact assignment returned by `exitbind run next`.
- A harness-bound run carries a content-free receipt reference in its v2 start
  event. Before native launch, the adapter asks Exitbind to revalidate the
  current receipt/configuration/profile/manifest evidence, then hash-reads the
  exact receipt beneath StateRoot and its recorded canonical
  `exitbind/harness/harness-manifest.json` or legacy root manifest beneath
  ControlRoot. `--require-harness-receipt` refuses an unbound assignment.
- `runtime.host` must be `codex`; a requested model and reasoning effort are
  passed explicitly to Codex. No fallback is selected.
- The selected profile and project-local memory sources are re-read and hashed
  immediately before native launch. Config, profile, memory, boundary, and
  upstream-artifact drift therefore retain existing fail-closed behavior.
- With `--sandbox-mode`, Exitbind passes the selected `read-only`,
  `workspace-write`, or `danger-full-access` posture to Codex and records it.
  Without the option, Codex inherits its resolved configuration and Exitbind
  records `unknown`. Approvals are forced to `never`; the recorded posture is
  evidence, not enforcement.
- One task-specific tmux socket is derived from run/stage/attempt/agent identity.
  A second live launch of that assignment is rejected before another Codex
  process starts. This is same-host process exclusion, not a distributed lease.
- Process exit is insufficient. The runner succeeds only after observing the
  exact stage/attempt/agent event produced by normal `run submit`; absence from
  the pending set is not treated as submission.
- The adapter stores no prompt, packet, transcript, environment dump, or JSONL.
  Its mode-0700 StateRoot directory contains only bounded identity/status files,
  recorded sandbox posture, native exit kind/code or termination signal, and
  Exitbind-generated bounded errors. Native stdout and stderr are discarded so
  an executable cannot echo supplied context into
  recovery state. These files are local recovery aids, not canonical task
  evidence.
- The fresh Codex process follows normal native project/global discovery. For a
  bound assignment, the adapter additionally presents the exact raw manifest
  claims after the receipt and manifest hashes pass; this is presentation only,
  not proof that a skill, perspective, Ponytail injection, or hook activated.
  Record those separately with the existing harness receipt evidence levels;
  `configured` or `presented` is not `hook_observed`.
- The in-memory prompt orders the authoritative contract and assignment first,
  then reviewed profile guidance, context-only memory, and evidence-only
  harness claims. Upstream artifact references prove bytes only. Instruction-
  like text in non-authoritative content cannot grant scope or approval; these
  labels are a reviewable contract, not a model-enforcement guarantee.

The native runner requires Codex CLI and tmux on the host. It is part of the
Exitbind binary, needs no Python or Node.js runtime, and is never started
automatically.

## Start and recover

Start an already-authorized pending assignment:

```sh
exitbind away start implementation_worker .exitbind/runs/run.jsonl \
  --config exitbind.json --name bounded-change \
  --sandbox-mode workspace-write \
  --require-harness-receipt
```

The strict flag is optional. Use it when the assignment must carry a valid
receipt-v2 binding; without it, unbound runs retain the existing adapter path.
The sandbox option is also optional; omit it only when an explicit `unknown`
posture in `away show` is acceptable.

ProductRoot and StateRoot come from the validated configuration; they are not
duplicated as runner options. The command prints the isolated socket, session,
and private state path before returning.

```sh
exitbind away list --config CONFIG
exitbind away show RUN_ID --config CONFIG
exitbind run inspect LEDGER --json --config CONFIG
```

If the assignment remains pending, inspect the bounded error/status evidence
and retry only after the isolated tmux session has ended. The submitted artifact
and canonical ledger are the result; away mode keeps no separate model output.

## Harness receipt evidence

An opt-in receipt-v2 manifest may record the adapter as a normal skill
activation such as `exitbind/away`. Use `configured`, `presented`,
or `hook_observed` only for what the host actually established. The receipt is
not process survival or model-compliance proof, and no new evidence level or
receipt field is introduced.

### Run-event format

An unbound run continues to emit run-event version 1 with identical fields.
Passing `--harness-receipt` emits run-event version 2: the start event adds only
`harnessReceipt` with `path`, `sha256`, and receipt `version: 2`; later events
carry event version 2 without repeating the reference. Mixing event versions is
invalid. Exitbind 0.7 inspects both formats, including the committed frozen
fixtures. A 0.6 binary does not understand a new version-2 run; upgrade the
binary to inspect it. Receipt and configuration formats did not change.

`run supersede` may bind a newly selected receipt to the successor. It never
copies the predecessor binding implicitly or changes predecessor bytes.

## 0.6 migration

The 0.6 project-copied Python script was replaced by `exitbind away` in 0.7.
Existing copied scripts are not executed or refreshed by 0.7 and may be removed
after no live 0.6 runner depends on them. Run and receipt ledgers remain
backward-inspectable.
