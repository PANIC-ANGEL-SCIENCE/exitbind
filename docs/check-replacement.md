# Continue after a defective frozen check

A check policy is part of an immutable run start. If the configured Lead determines that the check contract itself cannot assess the intended work, use the Work replacement route to create a linked successor. The predecessor remains blocked or running with its original policy and events.

Read current Work detail and use its current binding. Preview the explicit Lead decision:

```sh
exitbind work replace-check WORK \
  --check-command 'cargo test --test intended_case' \
  --classification defective-contract \
  --reason 'The frozen command names a test target absent from this project.' \
  --current-binding BINDING --json
```

Inspect the predecessor and successor policy identities, source event, reason and configured Lead owner. Preview returns a fully populated `applyCommand`; execute that exact command to create the successor. The current binding is rechecked before the write. A stale or foreign binding refuses.

The successor start and predecessor claim preserve both policy hashes and origins, the predecessor ledger and head, the source event, the configured Lead owner, and the explicit reason. It carries applicable same-Goal accounting, review requirements, preservation checks and unresolved-check guards through the existing supersession protocol. It does not copy a prior check pass, review or grant. The successor requires its own current check and applicable review.

Replacement keeps the predecessor's complete frozen plan, including each role's declared boundary, its boundary-manifest identity and architecture selection. The recorded boundary manifest, project configuration and selected architecture source must still be current before a replacement claim or successor is written; a current reader also rejects a successor whose rehashed plan differs from the predecessor.

Do not use this route for a normal failed product assertion, missing environment dependency, timeout, or uncertain live effect. A check failure does not by itself prove the contract defective or establish that no side effect occurred. Unknown effects, unresolved observations, accepted predecessors, stale session/project state, and live or prepared children remain blocking conditions. A committed successor should be followed through its returned read-only Work detail route; do not repeat the write after a lost projection.

When a worker submission leaves Work blocked with no pending Lead assignment,
the same route binds the explicit sibling Lead decision to the configured Lead
owner and current Work binding. The blocked Work action form exposes the route
with only the new check and reason as placeholders. It does not create or
append a Lead submission to the blocked predecessor.

The replacement marker is an opt-in persisted extension with its own protocol.
Older strict readers refuse marked successor starts and claims. Keep the old
Work on its pinned executable/configuration and read the successor with the
compatible executable that created it. Unmarked existing Works keep their
previous format and read path; rollback never removes or rewrites the marker.
