<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->

# Optional publication checks

These resources check proposed GitHub push updates, tracked snapshots and text.
They grant no permission to publish and do not configure a hook. Invoke the
pre-push resource from a separately managed hook, or run the two named checks
directly. Keep all three executables together.

The caller supplies `PUBLICATION_PRIVATE_TERMS_FILE`, a UTF-8 file with one
case-insensitive fixed string per line, and
`PUBLICATION_BLOCKED_COAUTHORS_FILE`, a UTF-8 file with one blocked co-author
name per line. Blank and comment lines are ignored; an explicitly empty
co-author file means no names are blocked. Both files must be regular,
non-symlink files with mode 0400 or 0600. A terms file must contain at least
one entry. Missing or unsafe inputs refuse verification. Configure allowed
commit and tagger identities with Git's `publication.allowedIdentity` values
for each repository; an absent allowlist also refuses verification.

`check-tracked-snapshot REPOSITORY --cached` checks the staged content and
effective author and committer. With a commit or ref, it checks the reachable
history; `--commits-from FILE` checks exactly the listed commit objects. The
pre-push entry uses Git's update stream and checks the newly exposed commit
set and annotated tags for GitHub remotes. A non-GitHub remote is outside this
check. `check-public-text LABEL` reads the proposed text from standard input.
Diagnostics do not print matched values, content or identities.

These checks do not establish destination authority, repository visibility,
provider acceptance, CI status, or absence of all secrets. API clients do not
automatically invoke Git hooks. Published history, caches and copies require
their own authorized treatment.
