# Agent control-plane boundary

Status: accepted architecture direction. This document defines responsibility
and dependency boundaries; it does not add a CLI command, configuration field,
or runtime guarantee.

## Product identity

Exitbind is a local agent control plane for bounded coding handoffs. Here,
"control plane" means that Exitbind defines and records agent semantics:
identity, purpose, authority, memory rights, relationships, run transitions,
acceptance, and evidence.

It does not mean that Exitbind controls the operating system, provider, model,
or host process. Declared boundaries and recorded evidence do not prove host or
model compliance.

## Three planes

| Plane | Owner | Owns |
|---|---|---|
| Control | Exitbind | Agent identity, purpose, declared authority, memory lifecycle, workflow relationships, run state, acceptance, and evidence |
| Distribution | dotagents or another installer | Resource sources, versions, trust policy, dependency resolution, lockfiles, installation, and host-specific projection |
| Execution | Codex, Claude, or another native host | Provider authentication, model invocation, tools, subprocesses, permissions, and actual enforcement |

The planes cooperate without inheriting one another's authority. Exitbind may
record or inspect bounded evidence about distribution and execution, but that
evidence does not transfer ownership to Exitbind.

## Sources of truth

`exitbind.json` is the semantic source of truth for Exitbind. It defines named
agents, their purposes and declared boundaries, memory eligibility, requested
runtime bindings, workflow membership, and lead authority. Profiles, run and
memory ledgers, artifacts, and receipts provide the corresponding selected or
recorded evidence.

When dotagents is used, `agents.toml` is the distribution source of truth. It
defines where resources come from, which version is selected, and which hosts
receive them. It does not define Exitbind authority, memory rights, workflow
transitions, or acceptance.

Host-native agent, skill, plugin, MCP, and hook files are projections or
external configuration. Their existence is not proof of activation, and they
must not silently become canonical Exitbind state.

The repository-level ownership layout is defined separately in
[the Exitbind-centered directory contract](exitbind-centered-layout.md):
`exitbind/` contains public canonical control material, `.exitbind/` contains
private operational evidence, `.agents/` distributes resources, and host
directories remain execution projections.

## Agent graph and authority

Exitbind owns the meaning of relationships among agents:

- the lead scopes work and alone records final `accepted`;
- workers implement bounded assignments and return artifact evidence;
- reviewers return role-scoped evidence such as `approved`, not acceptance;
- workflow stages define delegation order and artifact dependencies;
- memory lifecycle rights remain explicit per agent and scope.

Coordination never grants the lead or another agent implicit access to every
memory or artifact. Control-plane metadata needed to validate a handoff is
distinct from global content authority.

## Existing adapters

Exitbind may maintain narrow adapters that present or execute its own contract:

- `exitbind init` projects Exitbind's bundled skill into supported project-local
  host directories;
- `exitbind hooks` explicitly manages bounded Codex and Claude hook entries;
- `exitbind away` launches one already-authorized pending Codex assignment.

These exceptions do not make Exitbind a package manager, generic projection
framework, or model runtime. Generated host files remain disposable, unrelated
host configuration is preserved, and canonical run submission remains in the
Exitbind ledger.

## Architectural invariant

Exitbind owns semantics, not logistics.

In practical terms:

- Exitbind decides who, why, what may be requested, what may be remembered,
  how work is handed off, who may accept, and what evidence is recorded.
- A distribution system decides where resources come from, which version is
  installed, where it is projected, and how dependencies are locked.
- A native host decides what actually runs and how permissions are enforced.

Exitbind must remain conceptually complete if dotagents is replaced by another
distribution system or if Codex is replaced by another execution host.

## Bounded content visibility

Control-plane validation may require bounded global metadata such as agent
names, workflow edges, hashes, and lifecycle states. It must not imply shared
canonical memory, automatic transcript ingestion, universal artifact access,
or reconstruction of every agent's local context.

Content visibility and memory authority remain explicit, role-scoped, and
revocable even when control metadata is centrally inspectable.

## Feature decision rule

Before adding a feature, ask whether it helps Exitbind define, validate,
govern, or evidence an agent relationship.

If the feature primarily downloads, installs, synchronizes, converts,
distributes, resolves dependencies, manages a marketplace, or configures
generic host resources, it belongs in a distribution system unless repeated
dogfooding evidence proves that a narrow Exitbind-owned adapter is necessary.

Any such adapter must be deterministic, limited to Exitbind's own canonical
definition, disposable, and smaller than adopting general distribution
responsibility.

## Evolution order

1. Keep this responsibility boundary reviewable and consistent with the code.
2. Dogfood Exitbind with and without dotagents in real projects; classify
   friction as control-plane, distribution, host, or integration-boundary.
3. Add read-only observed-state checks only after repeated evidence shows that
   declarations cannot be usefully evaluated with existing checks.
4. Formalize capability requirements only if they improve bounded assignment
   validity without introducing installation or version resolution.
5. Add another native projection only for a repeated failure that existing
   distribution tools cannot solve cleanly.

The current `doctor` command checks the Exitbind contract, profiles, declared
boundaries, and optional command presence. It does not reconcile desired state,
inspect `agents.toml`, install capabilities, or repair a host. Those remain
future decisions, not implied commitments.
