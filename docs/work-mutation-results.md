# Work mutation results (current source branch)

`work check WORK` and `work return WORK ASSIGNMENT --outcome OUTCOME` emit
one compact JSON result by default. `work return` also accepts `--json` for
scripts. The emitted JSON, including its newline, is at most 8 KiB.

`effect` says whether this invocation recorded an event or held a worker
result. `status` can additionally say `refused` when the recorded event is a
protection refusal. A process exit of zero means the recording operation was
reported as completed; it does **not** mean the checker passed. Read the
checker's `result.kind` and `result.code` or `result.signal` separately. An
invalid request or an error before recording exits nonzero.

`eventSha256` is the event from this invocation. `reference.headEventSha256`
may be a later head and must not replace `eventSha256`. The `next` object is a
hint with `requiresExpansion: true`; read `work next WORK --full` with the same
config before another mutation. If a compact lookup says
`constraintsOmitted: true`, its packet is absent until that full read.

`nextAction.command` is an argv array for a read-only
`run inspect LEDGER --event SHA --json --config CONFIG`. It selects the named
event even after the ledger advances and never reruns a checker or submits a
held result. The detail response reports `authorizesMutation: false`.
`evidence` reports referenced submission or check-log bytes as `verified`,
`missing`, or `changed`; unsafe or corrupt check-log references cause an
integrity error. A historical detail lookup does not validate the current
subject for reuse.

For an unusually long executable or config path, a mutation detail command
can be an argv suffix marked `nextAction.sameExecutableRequired: true` or end
at `--config` with `nextAction.sameConfigRequired: true`. Compact lookups use
the corresponding `fullCommandSameExecutableRequired` and
`fullCommandSameConfigRequired` markers. Append the exact config value used
for the current invocation. Do not substitute a default config. If
`fullCommandSameExecutableRequired: true`, prepend the
exact executable that produced the current reply to the returned argv suffix;
the product never substitutes a bare command name for an oversized executable
path. `diagnostic.phases` independently names `projection` and
`cleanup` when either failed after recording; the event identity and outcome
remain available.
