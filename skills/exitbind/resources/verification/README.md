<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->
# Checkout-bound Cargo target

The optional selector requires Python 3.11 or newer.

`bind-target.py --checkout PATH --cache-root DIRECTORY` selects a private target
for the resolved checkout and prints its paths as JSON. It creates an ownership
marker only in a new or empty directory and refuses an existing unmarked,
foreign, malformed, or symlinked target. Configure Cargo with the returned
target. This selector does not prove that a test read the current source, bind
a result to an input fingerprint, acquire a build lock, or run any checks;
those remain responsibilities of the repository's verification procedure.

When a checker reads source files, establish the source it actually read before
reusing its result. Compare a meaningful witness from that executing checker
(for example, its compiled source root) or reproducible compile provenance with
the expected resolved checkout. The invocation directory, equal commit IDs,
cache ownership and Cargo's freshness metadata alone do not establish this.
If no witness is available, build the checker in an isolated target and retain
the compile provenance. Preserve mismatched-result logs as wrong-subject
evidence, then rerun only the affected named check. A repository may provide a
stronger existing binding; use it rather than adding a duplicate gate.
