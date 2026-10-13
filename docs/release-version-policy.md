# Release series and compatibility

Published stable tag: v0.31.12
Source candidate tag: v0.31.12

The published stable release is 0.31.12.
The current source version is 0.31.12.
README install links name the available published assets. A source version or
passing CI alone does not establish an official release. The release-reference
check validates published and candidate declarations separately; source metadata,
schemas and installation defaults must match the current source version.

Future stable releases in this series use 0.31.10 through 0.31.99, increasing
only when an immutable release is delivered. Versions 0.31.1 through 0.31.9 are
intentionally skipped. Core numbers have no leading zeroes: 0.31.01 is invalid.
The updater compares numbers, so 0.31.10 follows both 0.31.0 and 0.31.9. Passing
99 or needing a new compatibility epoch requires a deliberate series decision,
such as 0.32.10.

This is a pre-1.0 product convention. Compatible new features share a series
with fixes and are described as features. Breaking CLI, configuration, privacy
or persisted-format changes require explicit migration and version review.
Package versions never replace ledger format identities.

Capabilities are opt-in. Configurations without `project.capabilities` retain
the previous path. Strict older readers refuse that new configuration field;
retain the pre-registration configuration for rollback. Existing Work remains
bound to its original configuration and producer. New optional child-intent
metadata does not grant an older host permission. Compatible readers prepare
and validate current approved content.

Publication requires exact source review, native checks, hosted checks on the
selected publication ref, actual WSL validation, tag checks, release attestations
and installer proof. Official release and operator installation remain separate decisions.
Published tags and assets stay immutable.

## Automatic documentation references

`Published stable tag` above is the checked-in stable checkpoint; `Cargo.toml`
is the candidate authority. Run `python3 tooling/releases/sync_docs.py --write`
(Python 3.11+) to render references, or omit flags to check them. A candidate
bump never promotes stable installation links. `--write --from-github` first
verifies the canonical latest stable release, its published/non-draft status,
and all nine native assets. Both the checker and regression tests run in
`scripts/ci-local.sh`.

`tooling/releases/doc-versions.json` inventories exact version-bearing lines.
A `template` uses `{stable}` or `{candidate}` without the leading `v`; its
`count` catches missing or duplicated references. A `pinned` line and reason
preserve historical evidence, compatibility examples and host versions. The
changelog is the only whole-file archive exception; its source heading remains
checked by the existing release-reference gate. All Markdown, MDX,
reStructuredText and AsciiDoc files are scanned, including language variants
and guides outside `docs`. New version-bearing lines must join the inventory,
even when their values equal today's stable release. Add the exact template
or justified pinned line, render, then check. New formats need scanner tests.

After asset publication, the release workflow checks out trusted main and
prepares a documentation patch. Its separate publication job uses the existing
GitHub token with contents and pull-request write permissions, limited to that
job. It only pushes `automation/stable-docs` and opens or reuses a draft PR to
main. It never merges a PR, pushes main, force-pushes, deletes branches, changes
rules or uses the badge branch. Existing PR discussion and metadata are retained.
The repository must permit this exact branch through its branch lock and allow
GitHub Actions to create PRs. An installation requiring an additional permission
or policy change must obtain that authorization separately; no new credential
is required or created by this workflow.

GitHub-token pushes do not trigger push workflows. PR opened, synchronized or
reopened events can create approval-required workflow runs; a reviewer with
write access can approve them. See GitHub's
[token event behavior](https://docs.github.com/en/actions/concepts/security/github_token#when-github_token-triggers-workflow-runs).
This job runs the documentation and release-reference gates before publication;
it does not dispatch duplicate CI or create another credential to trigger it.
Run any additional required review checks through the authorized CI route.

Reruns with the same rendered tree create no commit or duplicate PR. Pending
bot version updates may be extended, preserving main's current tree and the
previous branch head as parents. Unexpected branch edits, a PR with another
base, a changed main, or a rejected non-force push stop publication. No branch
code is executed. Squash-merged implementation changes are reusable only when
Git's computed merge tree is exactly current main; otherwise review is required.
Review the saved patch and job error on failure; published
installer checks remain independent. A PR creation error is followed only by a
read to determine whether it succeeded; it is not blindly retried.

The initial automation implementation is reviewed on top of `fix/fr45-residuals`
so its product fixes, tests and changelog are preserved without merging them as
a setup step. Once the implementation reaches main through review, recurring
PRs target main. A still-open initial PR with the stacked base blocks automated
branch reuse until that review is resolved.
