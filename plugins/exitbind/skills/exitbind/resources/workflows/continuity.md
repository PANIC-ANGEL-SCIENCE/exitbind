<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->
# Same-thread continuation

When the owner explicitly requests continuation after disconnect and unfinished
work remains, keep the existing objective, scope, approvals and stop conditions.
Do not start while an owner decision is needed or a writer with overlapping
scope is active. Finish the current atomic action and preserve a compact,
private handoff and exact thread identifier. A new agent or new thread does not
prove continuity of the existing conversation.

Use the host's verified same-thread resume interface and a task-specific
supervisor that can outlive the terminal. For a Codex host whose local help
supports `codex exec resume SESSION_ID -`, send the handoff through standard
input in the selected working directory. Start only after the current turn has
finished and can no longer write, and coordinate the session lock to prevent
concurrent resumes. Use private permissions for handoff and output; do not pass
credentials in the prompt or print unrelated conversation contents. Record the
supervisor identity, process status and output location without treating them as
product lifecycle or acceptance evidence. If thread identity, termination, lock
ownership, or the supported resume interface cannot be established, stop.

On return, observe the process and its result, then inspect the actual affected
state and evidence. Process exit by itself does not prove the task completed.
