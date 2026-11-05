# Validation record

## 0.2.0 method update

Date: 2026-11-05. Scope: 34 allowlisted files; three consistent manifests; compact entrypoint and all routed references; local links; original 18 repository records plus seven public source records with observation and rights limits. No external dependency, demo or private project artifact added.

Run `python3 scripts/check_pack.py --self-test` from the package root. The update adds negative cases for a missing/duplicate public source, missing evidence limits, an unsupported rendered-evidence claim and an unrouted first-pass method, alongside the original five cases. These establish package integrity and provenance-field constraints, not semantic quality of prose or design outcomes. Actual execution is reported with the worker delivery; no new archive, provider installation or rendered design trial is claimed here.

The new teaching examples and first-pass workflow received the author walkthrough in [EVALUATION.md](EVALUATION.md). Independent review and the governing final check remain separate from that walkthrough. Browser access limitations for public references are recorded in [source notes](skills/design-pack/references/sources.md).

## Historical 0.1.0 record

Package: Design Pack 0.1.0. Date: 2026-11-04.

Scope: self-contained package structure, declared metadata, local links, source coverage and archive integrity. No upstream programs, renderers or provider plugins were installed. This candidate changes pack sources and their documentation; no host settings or runtime behavior are modified.

Executed `python3 scripts/check_pack.py --self-test`: PASS, 31 allowlisted files; manifest consistency, routed references, local links and 18 source records passed. Five negative cases were rejected: broken local link, manifest drift, missing sources, unexpected private file and symlink.

The packaging command revalidates and checks every ZIP member against the source bytes. Archive execution is additionally recorded in the delivery report outside the archive to avoid self-referential checksums. A successful structural check is not an aesthetic or accessibility result for a design produced with the pack.

Not tested: Claude/Codex installation/discovery, automatic triggering, native role/capability loading, Exitbind marketplace or release integration, physical design production, assistive-technology behavior of generated work, and comparative success/cost improvements.

Repository candidate checks: the existing Exitbind plugin distribution checker passed unchanged, and the Design Pack checker/negative tests/archive comparison passed in the repository location. The full `scripts/ci-local.sh --pre-return` attempt stopped because Cargo was unavailable. The repository product-surface gate likewise could not run its Cargo package inventory subcheck. Rust and full hosted CI success are not claimed. No marketplace registration, release or runtime behavior is changed by this source addition.
