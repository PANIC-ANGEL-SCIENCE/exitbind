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

| What happened | Next action |
| --- | --- |
| A check is missing | Have the host execute the exact frozen command and report its actual result for the current worker submission. |
| A check failed | Repair through explicit rework, then submit fresh work, check, and review. A review approval cannot clear the failed report. |
| Reviewer requested rework | Follow the returned worker assignment. Preserve the earlier documents. |
| Recorded artifact changed | Inspect its hash and restore the exact legitimate bytes. Do not edit the ledger to approve different bytes. |
| Configuration, profile, or selected context changed intentionally | The lead inspects the predecessor and explicitly creates a successor where supported. |
| Native agent cannot be started | Resolve the host mapping before continuing. A printed assignment does not mean an agent ran. |
| A worker, reviewer, or lead recorded `blocked` | Inspect the blocker. The lead can resolve scope and create an explicit successor where supported; do not record approval on the blocked run. |
| Run is accepted or rejected | It remains final. Start a separate task when more work is needed. |

The [first checked run](first-checked-run.md#3-record-the-review-and-lead-decision)
contains the complete rework sequence. Exact supersession and drift rules live
in [run and recovery](../REFERENCE.md#run-and-recovery).

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
