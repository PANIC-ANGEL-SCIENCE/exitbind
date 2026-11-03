<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->
# Checkout-bound Cargo target

`bind-target.py --checkout PATH --cache-root DIRECTORY` selects a private target
for the resolved checkout and prints its paths as JSON. It creates an ownership
marker only in a new or empty directory and refuses an existing unmarked,
foreign, malformed, or symlinked target. Configure Cargo with the returned
target. This selector does not prove that a test read the current source, bind
a result to an input fingerprint, acquire a build lock, or run any checks;
those remain responsibilities of the repository's verification procedure.
