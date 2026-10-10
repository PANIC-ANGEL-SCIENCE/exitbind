# Resume work or repair its record

Give your existing host the ledger path and ask what remains. It can retrieve
the frozen task and earlier result references without making you reconstruct
event IDs. Start with these read-only views, using your actual configuration
and ledger paths:

```sh
exitbind run status LEDGER --config CONFIG
exitbind run next LEDGER --text --config CONFIG
exitbind run inspect LEDGER --config CONFIG
```

`status` checks current artifact/configuration conditions. `next` returns the
validated pending assignment. `inspect` checks recorded history; it does not
establish that current files still match. The readable `--text` form requires
the stable binary; older stable hosts can read the default JSON packet.
For a named `work check` or `work return` event, use the read-only selector and
response rules in [work mutation results](work-mutation-results.md).

| What happened | Next action |
| --- | --- |
| A check is missing | Have the host execute the exact frozen command and report its actual result for the current worker submission. |
| Check observation failed | Read `work detail WORK`. An ended attempt exposes a Lead repair disposition; follow its new worker assignment/permit, return fresh work, then check and review. Unresolved execution or live descendants remain blocked from retry or supersession. |
| Failure storage was unavailable | Keep the admitted Work unresolved; follow the returned inspection route and stop if termination cannot be established. The response is diagnostic evidence, not a durable check or permission. |
| A check failed | Repair through explicit rework, then submit fresh work, check, and review. A review approval cannot clear the failed report. |
| Reviewer requested rework | The Lead decides the current finding through disposition, then follows its authorized worker assignment. Preserve earlier documents. |
| Recorded artifact changed | Inspect its hash and restore the exact legitimate bytes. Do not edit the ledger to approve different bytes. |
| Configuration, profile, or selected context changed intentionally | The lead inspects the predecessor and explicitly creates a successor where supported. |
| Native agent cannot be started | Resolve the host mapping before continuing. A printed assignment does not mean an agent ran. |
| A worker, reviewer, or lead recorded `blocked` | Inspect the blocker. The lead can resolve scope and create an explicit successor where supported; do not record approval on the blocked run. |
| Run is accepted or rejected | It remains final. Start a separate task when more work is needed. |

Repeated check failures keep the same Work and its spent authority. Read
`work detail WORK --json`: `actionForms.recovery` supplies the current material
re-plan or evidence form. Fill only the Lead's actual intent; it grants no new
host permission. After recovery, a `recovery.heldResults` command returns the
selected exact retained bytes with no rewriting. Refresh detail and run the
fresh applicable check and required review before Lead acceptance. A blocked
governor or unresolved execution remains a truthful stop.

One narrow exception exists for an owner-attested successor after the exact
current worker's allowed permit is immediately followed by a terminal
evidence-required duplicate refusal. If fresh `work detail WORK --json`
contains `recovery.ownerRecoveryDraft`, inspect its exact ledger, assignment,
operation, nested governor events and product snapshot. The draft is
unapproved; it does not authorize a new action. The owner must confirm the
same invocation and supply a complete, bounded StateRoot inventory covering
product writes, process starts, network writes and other external effects.
Unknown effects, changed inputs/configuration, held results, an intervening
event or a consumed grant keep the old run blocked. These owner attestations
remain evidence of the owner's review, not a mechanical proof about external
services.

After running the emitted `run supersede ... --owner-recovery` command, the
same-goal successor preserves every governor counter and begins at
`evidence_required` with no inherited grant, check result, review or
acceptance. Record fresh exact evidence before requesting a permit. Keep the
blocked predecessor unchanged; its original binary can still read that exact
ledger even if it cannot read the new marked successor. Ordinary
`beforeEditing` forms use a stable request ID derived from their current
binding, so replaying the exact argv recovers the original response instead
of creating another spend.

The [first checked run](first-checked-run.md#3-record-the-review-and-lead-decision)
contains the complete rework sequence. Exact supersession and drift rules live
in [run and recovery](../REFERENCE.md#run-and-recovery).

For an approved same-goal successor, use the current `run supersede` response.
When it includes a Work handle and `currentDetail` command, consume that fresh
detail before assigning work; it carries the remaining bounded accounting and
starts with new authority. If the response only describes a legacy run ledger,
use `work resume --history` to discover a supported Work locator. A separately
admitted managed-file request remains with its existing effect owner and is
never resolved or retried by governor carry.

Before starting another run in the same host session, check that its configured
native task names are usable there. Some hosts retain used names. Reusing an
existing native agent and spawning a fresh context are different operations;
do not silently substitute one for the other. If an exact frozen mapping must
change, keep the original records and use the explicit successor procedure.

For an older ledger, choose a reader from the
[public tag and format map](../CHANGELOG.md#public-tags-and-format-readers).
Never make an old binary accept a ledger by editing its format number.

These operations recover recorded task evidence. The host owns conversation
continuity, test execution, permissions, and product rollback. Keep normal Git
checkpoints or backups for product edits. See the
[authority boundary](../REFERENCE.md#authority-boundary) and
[term translation](glossary.md).
