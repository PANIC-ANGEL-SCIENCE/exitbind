# Design Pack 0.2.0

A self-contained, independently authored design workflow intended for possible future distribution alongside Exitbind. It consolidates useful methods from 18 reference repositories and seven public visual references into **one pack**, rather than connecting or installing those repositories.

Start at [the skill](skills/design-pack/SKILL.md). It routes work across client briefing, research, strategy, art direction, digital products/services, brand, graphic/print/editorial, diagrams, industrial products, spaces, fashion, motion, sound and 3D. The workflow covers discovery through maintenance, with compact on-demand references and editable templates.

## What this delivers

- A designer's method for turning vague requests into concrete intent, alternatives, decisions and acceptance checks.
- Actual-reference comparison and purposeful-copy checks before choosing or revising visual direction.
- Concrete composition contracts, optical alignment, contextual color and optional effect budgets with visual rejection tests.
- Explicit one-shot mode: planning alternatives only, freeze the first artifact, then inspect without redesign.
- Role-specific perspectives and medium-specific production handoffs.
- Accessible, context-sensitive design validation and honest evidence limits.
- Separation of reusable principles from private project colors, sizes, concepts and choices.
- Source lineage and licensing considerations for every supplied repository.

This is a skills-only package, not a renderer, CAD suite, physical testing lab or guarantee of expertise in every design specialty. Core briefing, planning, critique and specification need no external applications. Production uses tools already available and authorized for the task; the pack does not install them.

## Use now without installation

Open `skills/design-pack/SKILL.md` in your assistant's file context and ask it to apply Design Pack to your current task. Include existing project guidance and artifacts where relevant. Have it record which references it actually read and the specific decisions they informed.

Example: “Use Design Pack. Make the appointment flow feel welcoming and considered; preserve the existing booking behavior. Choose the direction and explain the important assumptions.”

For a small edit, it should proceed with a short interpretation. For a new or consequential direction, it should frame meaningful alternatives or choose within delegated authority. It should not turn every request into a workshop.

## Installation candidate and future Exitbind distribution

The archive includes root, Claude and Codex plugin manifests modeled on Exitbind's inspected 0.31.26 skills-only layout. It is a source-only candidate, **not released or installed**; provider discovery/installation has not been exercised.

To try a skill-only local installation, use the host's documented skill import mechanism to copy the complete `skills/design-pack/` directory into an approved skill location. Preserve its `references/` and `templates/` directories. Resolve any name collision before copying; do not overwrite an existing skill. Reload the host as required and explicitly invoke the skill, then confirm that it read the entrypoint and a relevant reference. This is a procedure, not a tested provider CLI command.

For a host supporting local plugin imports, select this complete package root using that host's current documentation. Confirm metadata parsing and skill discovery before making compatibility claims. Do not register it in an Exitbind marketplace without a separate reviewed integration change. Exitbind's existing distribution checker is specific to its own plugin and is not this pack's validator.

No hooks, MCP configurations, executables from upstream, provider settings or Exitbind runtime configuration are installed. The manifests do not grant host permissions. See [Exitbind boundaries](skills/design-pack/references/exitbind.md).

## Local checks and packaging

Python 3.10+ standard library only. Run from this package root:

```sh
python3 scripts/check_pack.py
python3 scripts/check_pack.py --self-test
python3 scripts/package.py --output ../design-pack-0.2.0.zip
```

The checker validates the exact file inventory, local links, metadata consistency, source coverage and absence of symlinks/unexpected private material. Its negative tests exercise broken links, manifest drift, missing original and public sources, missing observation limits, unrouted methods, unexpected files and symlinks. Packaging revalidates and uses only the declared files. A successful check proves package properties, not design quality or provider compatibility.

See [VALIDATION.md](VALIDATION.md) for the actual execution record and [EVALUATION.md](EVALUATION.md) for scenario coverage and its limits.

## Maintenance and licensing

Original pack content is MIT licensed; no upstream material is vendored. [Source notes](skills/design-pack/references/sources.md) describe independent synthesis and asset/license limits. Keep private profiles outside this directory. To update, change source notes and metadata as needed, review scenarios, update the file inventory deliberately, and rerun checks before packaging. Do not overwrite a user's project memory during an update.
