# Task-scoped operator and continuation detail

Read this only for an initialized same-Work cross-host handoff, explicit review-policy revision, or setup whose current response needs these details. Ordinary host-managed workers/reviewers consume `work detail WORK --json` and its permit/return forms; they do not initialize continuation just to deliver a result.

## Cross-host continuation

Use `work continuation WORK` for an initialized same-Work cross-host handoff and consume its bound `receive` actions. Follow any section/item routes marked `requiresExpansion`, with the same executable and configuration. If another goal owns the continuation sidecar, preserve it and use ordinary current Work detail. Never search raw state to reconstruct the handoff or infer retry safety from a missing process.

When handing off a host-managed native child result, first bind the receiving host
and native session. Pass the `mutationContext.token` from `work next` or `work
continuation` to the supported action:

```sh
exitbind work bind WORK --context TOKEN --host claude --session NATIVE_SESSION --host-version VERSION
```

Use only host-reported identity. In Claude Code, Exitbind's SessionStart hook
exports the host's session ID as `EXITBIND_NATIVE_SESSION_ID`; other launchers
may set the same variable. If unavailable, report that limit instead of
inventing one. Run the returned read-only `nextAction.command` for a fresh
context token, then prepare the child before launching it:

```sh
exitbind work child prepare WORK SHORT_ASSIGNMENT --context FRESH_TOKEN
```

Then launch the native child normally. In Claude Code, Exitbind's managed
subagent hooks capture the next compatible subagent this session starts: they
record its host-reported ID and its final message exactly as the host delivers
it, so do not copy either yourself. Add `--agent-type TYPE` when other
subagents may start first. A subagent started with no prepared intent is never
recorded. Read `exitbind work continuation WORK` afterwards: `preparedChildren`
shows the capture state and `children` the recorded result. If a finished
child stays `prepared` or `claimed` (a host without these hooks), record its
exact final message with `exitbind work child WORK SHORT_ASSIGNMENT --context
FRESH_TOKEN --native-child CHILD_ID < EXACT_RESULT_FILE`, within 8 KiB and
never truncated; if it is `failed` with a retained result, run the `recover`
command it names. A host-reported child is not a provider-authenticated
identity or independent review.


## Setup and review-policy revision

Preview owner-approved facts with `exitbind setup --mode portable --root . --scope worker --observe PATHS --write PATHS --commands FACTUAL_COMMAND --check-command CHECK_COMMAND --goal GOAL --review-policy required|omitted`. Review affected paths and host mapping before `--apply`. The applied response supplies exact next argv. Per-role observe/write/commands flags express asymmetric facts; `none` supplies an empty list. Do not mix shared scope/facts with per-role facts.

On a marked running run, the owner may revise review policy using `exitbind run review-policy lead LEDGER --decision omitted --reason "OWNER_REASON" --config CONFIG` (or `--decision required`). Use the current supported run reference; never infer an omission or approval. An unmarked historical run retains required-review semantics and cannot use this revision route.

For an already reviewed Architecture Contract selection, preview `exitbind project architecture select SOURCE --decision reviewed --reason TEXT --json --config CONFIG`, then consume the returned bound `apply.command`. Drift requires a fresh preview. A preview never approves a proposal or executes a checker. Explicit `project architecture check` evaluates bounded literal file checks; include it only in the user-authorized check when appropriate.

Optional direct Codex recording uses `exitbind activity codex < PROMPT_FILE` only when explicitly requested; its private result is unjudged. Normal small direct work needs no recording.
