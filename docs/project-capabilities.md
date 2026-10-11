# Approved project skills and lenses

This path is included in the published 0.31.10 release. See
[release status](release-version-policy.md).

An approved resource reaches a relevant configured agent on its next native
launch. Registration grants no shell, memory, credential or host rights. The
agent handles the descriptor and executable command; the owner supplies purpose
and approval. Existing custom agent files stay in place.

1. Inspect `exitbind project context --json` and configured native mappings.
2. Put complete UTF-8 content in a confined project file (4096 bytes maximum).
   Create a descriptor using the current `project.memoryIdentity`.
3. Preview `exitbind project capabilities select lens.json --decision reviewed --reason "Owner approved the availability lens for parcel checks" --json`.
   Execute its exact apply command for that approval. The binding freezes
   configuration, descriptor and content. Preview writes nothing.
4. Inspect `exitbind project capabilities --task "Check parcel availability" --host codex --json`.
   Recipients match configured roles, explicit task terms and compatible hosts.
5. Begin a new Work with the changed configuration. Existing Work retains its
   frozen configuration; use supported supersession. Current native assignment
   context and a named `work child prepare` launch receive selected content.
   Consume the complete current assignment and rules as usual.

Replace project identity and expiry with approved current values:

```json
{
  "version": 1,
  "id": "availability",
  "revision": "r1",
  "kind": "lens",
  "owner": "lead",
  "projectIdentity": "<project.memoryIdentity>",
  "path": "lens.md",
  "roles": ["worker"],
  "taskTerms": ["parcel"],
  "hosts": ["codex"],
  "expiresAt": "<approved RFC3339 instant>",
  "revoked": false
}
```

Use `skill` or `lens`. The owner is the configured Lead. Declare one to four
roles and task terms. Host values are `codex` and `claude`. Limits are eight
registrations, three applicable resources per delivery and 6144 bytes for the
complete envelope. Narrow an oversized selection; instructions are never cut.
Path redaction has separate source and presented hashes.

Status distinguishes registered, expired and revoked resources, pending next
launch recipients and projected context. Projection does not prove launch,
acknowledgement or model use. A native result is separate evidence. Running
context has no supported live refresh and cannot be erased retroactively.

Review a new descriptor revision to update or revoke (set `revoked` to `true`),
then register through preview/apply. Changed bytes refuse stale delivery. Other
projects cannot use the descriptor. After a lost registration reply, inspect
and preview again; unchanged apply writes nothing. Repeated child context reads
retain the same delivery ID and never start another child. Inspection, preview and apply
require the Unix secure reader. Apply also requires POSIX file locking.
