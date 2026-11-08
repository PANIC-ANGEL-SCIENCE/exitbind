# From brand strategy to a usable design system

Use when creating or substantially changing an identity or reusable system. This connects existing methods; it does not require a brand workshop for a local edit. Keep a decision chain in the [template](../templates/brand-system.md): **audience/context → promise and evidence → identity principle → expression rule → reusable contract → application/state → test and owner**. Strategy does not mechanically determine a color or font; the chain makes creative judgments explicit and revisable.

## What this adds

| Existing coverage | Missing connection supplied here |
| --- | --- |
| [Briefing](briefing.md), [lifecycle](lifecycle.md), brand stages in [communication design](communication-design.md) | Supported positioning/promise that constrains identity and product behavior |
| [Aesthetic construction](aesthetic-construction.md), [typography](typography.md), [color](color.md), [Mockup Image](../../mockup-image/SKILL.md) | Identity grammar with invariants and permitted variation across those disciplines |
| [Interface](interface.md) and [interface craft](interface-craft.md) | Primitive → semantic → component ownership, variant/state contracts and promotion criteria |
| Existing handoff, validation and maintenance | Change classification, exceptions, migration and evidence tied back to the promise |

## 1. Build a position that guides choices

Write who the offering serves, in which situation, what they need to accomplish or feel, what credible difference it offers, and why they should believe it. Separate documented facts, audience research, aspirations and hypotheses. “Friendly, premium, innovative” cannot distinguish a position without context. Do not invent a segment, advantage or proof to complete the sentence.

Inspect alternatives along meaningful dimensions: service behavior, tone, imagery, information priority and recognizable assets—not only color. Identify category cues worth preserving and an opportunity for distinction. A small reference set informs hypotheses, not market uniqueness or user preference. Record recognition to preserve in a redesign: names, marks, familiar interactions and approved assets.

Choose two or three principles with consequences and counterexamples. For “clear before commitment,” show essential conditions before action, use specific labels and expose uncertainty; avoid certainty language when data is pending. For “curious and expressive,” allow surprising image/type relationships in discovery while keeping recovery calm. Mood, delight and cultural expression are valid objectives without fabricated conversion benefits. State what would contradict each principle.

For a family of offers, decide what shares a name/mark/system and what needs differentiation. Master identity, endorsed offer and independent sub-brand imply different naming, navigation and asset rules. Do not multiply brands or token modes merely because there are several screens. Use existing delegated authority and name the decision owner; do not add approval gates for routine authorized work.

## 2. Translate strategy into an identity grammar

Compare territories on the same content and touchpoints. Each needs an organizing idea, recognizable signature, type/color/image/voice relationships, tradeoff and disconfirming example. Do not compare a polished photographic proposal with an empty wireframe. In explicit first-pass work, alternatives remain planning text before one selected build.

Choose **invariants** that preserve recognition/behavior and **variables** that respond to context. For example, maintain title/metadata contrast and direct voice while campaign imagery and section color vary. Consistency means recognizable relationships, not identical surfaces everywhere.

| Layer | Decision to specify | Stress case |
| --- | --- | --- |
| Name, wordmark, symbol/lockup when in scope | Asset by application; small-use/monochrome variants, clear space and endorsed-offer relationship | Tiny icon, long/non-Latin name, one-color reproduction; recognition is not legal clearance |
| Type | Display/read/control roles, script pairing, optical differences, real assets and fallback | Mixed-script title, error paragraph, dense numbers; use the typography reference |
| Color | Expression versus status/action; area/lightness/chroma relationships and approved pairs | Image overlay, focus on selection, error beside accent, print substrate |
| Form/layout | Repeated alignment, silhouette, density and edge roles; deliberate exceptions | Nested controls, narrow layout, large text and unboxed information |
| Imagery/material | Subject evidence, camera/light/crop/texture grammar and authenticity | Missing image, factual detail, alternate crop; preserve explicitly requested medium |
| Voice | Stable character plus tone by situation; approved facts/terms | Welcome, instruction, delay, failure and recovery need different levels of playfulness |
| Motion/sound if relevant | Expressive events versus repeated feedback; stable/reduced alternatives | Repeated use, interruption, silence and reduced motion |

Document why a rule belongs and one misuse. Test realistic applications without relying solely on a large logo or borrowed photography. Apparent distinctiveness is not trademark clearance; rights and specialist decisions require their own evidence.

## 3. Turn expression into reusable contracts

Inventory applications/components first. Group by shared meaning and behavior, not resemblance alone. A clickable choice and decorative card can look similar but have different semantics. Reuse an existing system when its contract fits.

**Promote deliberately:** make a token when a named design decision must stay coordinated across real consumers, modes or platforms; record those consumers and meaning. Keep a one-off optical offset or composition local with its reason. Promote a component when repeated anatomy, semantics and behavior need a shared owner and test surface; repeated boxes alone are insufficient. If reuse requires many contradictory flags, split by responsibility or leave the composition local. Do not invent future consumers just to justify abstraction.

Use three levels where complexity warrants them:

| Level | Owns | Original notation, not a library API |
| --- | --- | --- |
| Primitive/reference | Available values: palette, spacing, type assets, radii | `palette.ink`, `space.compact`, `type.reading` |
| Semantic | Intended role and permitted pair/context; mode resolution | `text.primary → palette.ink`, `surface.action`, `focus.ring` |
| Component | Role within an object and supported variant/state | `choice.label → text.primary`, `choice.selected.background → surface.selected` |

Record token meaning, value/alias, units/type, applicable mode, owner and evidence status. Resolve aliases without cycles or missing targets; check intended pairs in every supported mode. A border role is not a caption role because both are gray. Accent is not automatically success, error or selection. Themes remap roles deliberately; do not invert colors or shrink text blindly. Keep justified optical exceptions local rather than distorting global scales.

A component contract states purpose, anatomy/slots, accepted data, semantic role, variants, reachable states, input behavior, content limits, responsive behavior, accessibility and evidence. Separate **variant** (purpose/emphasis), **state** (loading, selected, error), **mode** (theme/density/language) and **instance data**. For an action, specify trigger → pending → success/failure → recovery and focus/information movement. A spinner does not prove completion. Review the actual component through its real data seam; static fixtures do not prove backend behavior. Content and imagery contracts—long names, empty/error copy, alt text, crop anchors and missing-image behavior—belong to the system too.

## 4. Resolve adaptation with evidence

Change layouts around content and task priorities: reflow, coherent reading-order changes, secondary disclosure or different image placement. A breakpoint follows a layout constraint, not a universal device label. Preserve essential information and input access under zoom, large text, translation, touch and keyboard. Brand expression must survive without color-only status or essential text inside a photograph.

Use [validation](validation.md) and [interface craft](interface-craft.md) for actual states. Record identity fit, accessibility, behavior and production fidelity separately. A token-pair check does not prove every overlay; a component test does not prove a journey. An internally coherent brand guide may still promise unsupported service behavior—report that gap rather than decorate it away.

For print, signage, packaging, motion and physical work, retain principle and hierarchy but translate units/materials/processes. A screen token is not a print color specification; a UI radius is not a manufacturing tolerance. Use the discipline's proof, safety and production authority. Editable text can express these contracts without a required library or tool.

## 5. Govern proportionately and hand off

Name owners for strategy/identity, rules, implementation, content and rights; one person may fill several roles. Keep one current source of truth and record its implementation relationship. Installed files or declared tokens do not prove adoption.

Classify change: correction within a contract, additive supported variant, or breaking change to meaning/behavior. For shared changes, identify consumers, reason, evidence, migration and rollback before replacing the contract. Deprecation needs replacement, affected uses, owner and retirement condition. Do not silently change a semantic token's meaning under the same name. Exceptions need scope, reason, owner and review trigger; repeated valid exceptions may expose a missing rule. A small project can use a short table, not a committee.

Deliver editable identity assets/specs, role/alias map, component/state/content contracts, responsive/accessibility decisions, examples/misuses, rights, tested/untested matrix and maintenance notes. Distinguish concept, documented system, implemented library and adopted production system. Inspect actual consumers before claiming rollout. Ordinary iterations refine the chain; [first-pass.md](first-pass.md) freezes the first output before read-only checks and prohibits silent repair.

## Original worked chain: a fictional rehearsal-room service

Teaching brief only: a fictional community service helps groups choose and reserve rehearsal rooms. Assumed supplied data includes dimensions, equipment, access notes, availability and approved photographs. These are scenario inputs, not research or real service claims. Purpose: understand the room before committing. No names, motifs or values here are defaults or source measurements.

**Strategy:** serve groups comparing rooms under time constraints; promise clear expectations before reservation, conditional on verified data. Hypothesis: a precise yet welcoming voice can reduce uncertainty without making culture feel administrative. Proof needed: accurate listings, truthful availability and working recovery; no measured improvement claimed.

**Alternatives on the same listing:** a proof-led directory supports comparison with aligned labels and consistent room views; a communal poster language uses energetic titles and modular graphic fields for discovery; a photographic backstage story emphasizes atmosphere through contextual images and deliberately related interface treatment. Select the first for this brief's comparison priority; the others may fit a festival campaign or editorial tour. None prescribes an industry font or color.

**Selected identity:** “open interval”—space around the main room view, repeated open-ended rules linking titles to details, title and information roles differentiated by the actual reading task. This is a proposed relationship, not a finished trademark. Invariants: evidence stays visible, metadata aligns, actions say what happens. Variables: image subject and campaign color; contrast and recovery remain stable. Select supported title and multilingual reading voices with real specimens. Color distinguishes the information field, action emphasis and independent statuses. Images show actual rooms with coherent viewpoint/light where possible. Voice examples: “Choose a room”; “Checking availability”; “We could not confirm this time. Your selection is saved.” The last sentence is allowed only if selection really persists.

| Promise/identity consequence | Original token relationship | Component/state consequence | Rejecting evidence |
| --- | --- | --- | --- |
| Read facts before committing | `text.primary → palette.ink`; `text.secondary` has its own tested pair, not `border.subtle` | Room row separates dimensions/equipment from action; long values wrap | Facts clipped or secondary text unreadable |
| Action visible without erasing evidence | `action.background → palette.emphasis`; tested `action.foreground`; separate status roles | Reserve follows room/time summary; pending retains selection and truthful label | Image overwhelms action; pending implies confirmed booking |
| Recognition through rhythm, not boxes everywhere | `space.group` separates identity/details; component inset has its own alias | Rows share baselines; expanded detail may have a different form | Equal weight everywhere or crowded nested edges |
| Selection survives mode changes | `surface.selected`, `text.onSelected`, `focus.ring` resolve together per mode | Selection has text/icon cue plus surface; focus visible on it | Theme loses selection or keyboard focus |

Choose actual values/assets, test pairs and record them before implementation: aliases alone are not a ready-to-ship theme. A room row's anatomy includes approved image, title, metadata, availability and explicit actions. Compact comparison and expanded detail are variants; image failure, availability pending/unavailable and reservation failure are states. Keyboard/touch actions remain semantic; avoid ambiguous nested click targets when opening details and reserving are separate actions. A missing photo keeps labels/layout stable and marks the gap; it never invents a room or substitutes illustration for a requested photo.

At narrow width, move image above facts when side-by-side layout loses readable measure; preserve identity and selection. At large text, allow metadata/actions to wrap and grow vertically. For a physical wayfinding sign, carry title/label rhythm and direct naming with verified room numbers and production-specific size/contrast. Do not carry hover behavior or assume hex-to-ink equivalence.

**Handoff/change:** owner receives room-row anatomy, accepted data, variant/state matrix, alias map, image/copy rules and actual evidence with untested conditions. If tests show `text.secondary` disappearing in a supported theme, correct that pair and recheck consumers, including selected rows. If the row starts making reservations itself, treat that as a behavior change: update its contract, focus/recovery checks and consumers rather than disguising it as a radius adjustment. Record version, migration, rollback and remaining evidence. This walkthrough demonstrates traceability, not rendered validation, participant success or production deployment.

## Scale down without losing the chain

For a single component fix, keep the current brand decision and trace only the affected role, state, consumer and check. For a new small identity, use a one-page chain and a few representative applications. For a multi-product system, add ownership, mode/version/consumer mapping and migration where actually needed. Do not demand a logo redesign, full token library or research study for every task.

A requested local silhouette or naming language becomes a project contract only at its stated scope. Do not promote it to a global token without real shared consumers and authority. In imagery, styling tokens alone cannot establish a person's participation or the brand's cultural meaning; review the lived action/context relationship without inferring membership from appearance.
