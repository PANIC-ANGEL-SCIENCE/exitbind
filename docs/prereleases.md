# Paired Linux prereleases and stable readiness

Routine branch CI runs on Linux. When two selected task branches have successful
CI for their current commits, the repository can combine them on a dedicated
`integration/pair-<version>` branch, check that exact result on Linux, and request
one Linux-only prerelease. Main and stable publication remain separate decisions.

The maintainer selects exactly two existing task branches and one candidate
version in the repository variable `EXITBIND_PRERELEASE_PAIR`:

```json
{"branches":["R49/example-task","dot03/example-task"],"version":"0.31.17-rc.1"}
```

These are examples, not automatically selected assignments. An unset variable
leaves the workflow idle. The two source branches must have their own required
task review and acceptance before their normal publication. The repository
observes GitHub CI, not private agent-work acceptance records. It does not infer
approval from a branch name. Pair selection and branch publication retain their
existing owner and review obligations.

The selected number is fixed for the pair. An existing candidate tag stops new
publication; the workflow never increments versions after each commit or
overwrites a released version. It does not delete task branches. Missing or
failed source CI waits; merge conflicts, changed release-control code, changed
inputs and uncertain effects hold automatic publication for diagnosis. Check the
paired workflow's retained planning/completion evidence. A failed or interrupted
combined-check admission is not silently retried. Repair the task branch and
select a fresh candidate or resolve the retained admission explicitly.

Privileged stages run the workflow's trusted main code. Candidate checks use
read-only permissions and an explicit source commit. The two source branches
cannot automatically change release workflows or their control scripts; deliver
such changes through the separately reviewed main path. Repository protection
must permit creation of the dedicated integration refs and new prerelease tags
while retaining main protection and published-tag update/deletion protection.
The workflow needs no personal token, external service or persistent daemon.

The release workflow classifies the checked-out package version. Prereleases
check, package, attest, publish and exercise the installed Linux artifact only.
Their assets are the Linux executable, archive and checksum, plus the
candidate's own `install.sh` and its checksum. Candidate preparation rewrites
every `current` version mention to the candidate version through the
[version inventory](release-version-policy.md#version-mentions). They are
marked prerelease and cannot replace the latest stable release. A failed release
or unknown dispatch stays a distinct result requiring inspection.

When stable promotion is selected, first prepare its final coherent version on
the integration branch: change the `Cargo.toml` version, run
`python3 tooling/repository/release/sync-doc-versions.py --apply` and add the
CHANGELOG entry. Run the release workflow manually on that branch with
mode `stable-readiness`. This checks Linux, native Apple Silicon and Intel Macs,
and actual WSL; it creates no release and changes no main or tag. Its successful
`stable-readiness-<commit>` artifact and exact workflow run identify the checked
candidate. A change to that commit requires fresh readiness evidence. Only after
that result is verified should the separately authorized main promotion and
stable tag publication proceed. Stable publication retains the complete native,
WSL, archive-provenance and installed-platform gates.
