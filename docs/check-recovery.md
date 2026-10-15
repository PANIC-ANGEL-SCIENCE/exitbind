# Continue after a known-ended check could not save its failure

When a check ends but storage is unavailable, Exitbind can retain its admission
without a durable result. Freeing space does not itself authorize another
execution. Inspect the existing Work with its compatible reader and configuration.
Do not start a replacement Work to reset accounting.

In 0.30.1, the configured Lead can recover a retained storage-failure response
whose process group and capture readers ended, whose cleanup is complete, and
whose partial capture bytes still exist. Preview first:

```sh
exitbind work recover-check WORK --json --config CONFIG
```

For an eligible known-ended storage failure, the read-only response supplies
`ownerDecision`. Preserve its exact
recovery request binding, admission snapshot, earlier observer identity and timeout.
This binding is specific to this preview, with the `check_recovery_request_v1`
domain, current Lead assignment and original admission. A general Work-detail
binding cannot substitute for it. Canonical readers reconstruct the request
binding from the pre-recovery state as well as checking the actual ledger prefix.
After confirming the retained failure belongs to that attempt, set `approved`
to true, supply the Lead's reason, put the complete saved response bytes in
`response`, and set `responseSha256` to their SHA-256. Pass that complete JSON
object through standard input:

```sh
exitbind work recover-check WORK --apply --current-binding BINDING --json --config CONFIG < APPROVED_DECISION.json
```

The decision is limited to 32 KiB. It binds the exact ledger prefix, configured
Lead, admission, original inputs, check policy and retained response. Each
partial capture must match its admitted operation, path, size and digest.
Changed inputs, missing captures, stale decisions, live or uncertain termination,
and incomplete cleanup refuse without launching a check. A lost apply reply can
reuse the identical decision only at its current recovered head; later repair
or another transition makes that replay unavailable.

Recovery records one **Lead-reported failed observation**. A saved process exit0
does not supply passing check evidence when its observation was not committed.
The decision is not an authenticated host observation: the configured Lead must
report the actual retained facts. Local records remain tamper-evident rather than
tamper-proof against their filesystem owner.

Follow fresh Work detail after recovery: the Lead decides repair, then the new
worker obtains its ordinary permit, returns its result, and completes fresh
checks, required review and acceptance. Recovery supplies no permit, check
credit, acceptance or accounting reset.

The distinct v8 recovery event requires a compatible 0.30.1 reader for the
affected Work's remaining lifecycle. Keep its original binary/configuration
and all unrelated Work pins. Older readers refuse this action without changing
saved bytes; switching a selector does not downgrade the record. If the complete
failure response or known-ended evidence is unavailable, leave the outcome
unresolved and report the remaining user task. Source changes, private launchers
and hand-edited ledgers are not consumer recovery steps.

For a durable failed observation with unknown termination whose Work is already
blocked, the same preview returns `historical_unknown_termination`. This is an
immutable diagnostic: it identifies the original admission, failure and blocked
head, reports the recorded process facts and separately describes current input
availability. It exposes no captures, recovery decision or apply command.
Changed source does not prevent this historical inspection. `--apply` refuses;
the original Work remains blocked with no check credit. A later process listing
cannot establish the missing original execution identity.

On Linux, new observations use a fresh exclusive observer process to own and
reap their checker descendants, including children that change session. The
caller never becomes a process-wide subreaper. Exact checker status and raw
streams remain separate from the private termination proof. The observer waits for normal background children to end within the observation
budget and sends no cleanup signal on that normal path. Timeout cleanup signals only currently owned children, remains bounded
and remains a failed observation even when all children are reaped. Missing or
incomplete proof keeps termination unknown. This does not retroactively resolve
an older unknown execution or provide a sandbox against hostile processes.
