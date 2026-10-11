# Your own project agents

Use an approved project-local `your.soulmate/` directory as the source of your
profiles and reusable skills or lenses. This is an opt-in convention using
existing configuration, not a new runtime or a migration of existing agents.

```text
YOUR-PROJECT/
├── exitbind.json
├── your.soulmate/
│   ├── agents/my-agent/profile.md
│   └── lenses/availability.md
├── exitbind/agents/          # existing profiles stay here
├── .exitbind/               # private records stay here
├── .codex/                  # approved host projections stay here
└── .claude/
```

After approving the source and configuration change, ask your lead to point
the chosen agent's existing `profile` field at
`your.soulmate/agents/my-agent/profile.md`. Keep its role, workflow mapping,
boundaries and native name explicit. A display name changes no authority.
The profile is read directly; there is no second editable canonical copy.
Do this before beginning new Work. Existing Work keeps its frozen configuration
and reader; retain that configuration and use supported supersession if needed.

Profile paths are relative to **ControlRoot**. In ordinary portable setup it
is the project root. With separate source/control roots, this convention belongs
in the approved ControlRoot; a SourceRoot file cannot escape that confinement.
Skill and lens descriptor/content paths are relative to **SourceRoot**. See
[project placement](project-placement.md) before selecting split-root paths.

For an approved lens, put its content under `your.soulmate/lenses/` and use the
existing [capability preview/select path](project-capabilities.md) with that
content path, the current project identity, configured owner, relevant roles,
task terms and expiry. A named native launch receives the selected profile and
relevant approved lens. Inspection is not acknowledgement or observed use.
An already-running agent receives no invisible refresh. Changed content,
revocation, wrong project or recipient and unsafe paths retain their refusals.

Keep this directory private by default: add `/your.soulmate/` to the project's
ignore rules before writing private material. Exitbind does not automatically
stage or publish it. The owner may separately choose to track reviewable source
files. Preserve existing custom profiles and host files; never overwrite a
collision or use a symlink to bypass root confinement. Moving existing agents
or historical records needs a separate preview and owner decision.

This route uses existing schema fields and approved resource selectors. Older
readers retain their original configuration and may refuse capability fields;
no current selection is silently copied into an older or active Work.
