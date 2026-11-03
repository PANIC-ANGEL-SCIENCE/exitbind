<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->

# Context, retention and protocol evolution

For a single-context assignment, establish the exact context, permitted
operations, observation boundary, persistence and expiry terms, and approval
source before acting, including any declared deletion grace period. If a
required declaration is missing, report it and do not infer authority from
another context. Do not join identities, reconcile participant histories across
contexts, or create undeclared persistence.

For retention or deletion verification, compare the declared policy with the
relevant write, read, expiry, revocation, deletion, recovery and replica paths.
Use the least revealing evidence that can decide the claim. An empty query or
successful command alone does not establish that derivatives, indexes, logs,
caches or backups are gone. Do not recover, quote or retain forgotten content
to test deletion. Report policy, checked stores and non-content observations,
remaining uncertainty and the next authorized check.

For protocol or compatibility history, compare explicitly identified versions,
decisions, migrations, interoperability observations and failures. Preserve
which proposals were experimental, accepted, superseded or deprecated, and
trace rationale to its source. Do not infer participant identity, promote an
incidental trace into a history, or choose a compatibility break for the owner.
