# Build a component system that stays usable

Use for reusable UI components, system customization or an agent handoff. For a local spacing edit, use the existing component and change only the relevant decision. This method is framework-neutral; it does not require a new library, CLI, registry or migration. Start with [interface](interface.md), [brand system](brand-system.md) and actual project sources, not a catalog of desirable components.

## Inspect before inventing

Find the installed version, exported API, real implementation, theme/provider boundary and existing examples. Distinguish shipped behavior, a draft design proposal, deprecated guidance and an unverified marketing claim. Resolve conflicts against the target version and tests; report a missing capability rather than inventing a prop. Read the smallest relevant contract first, then its implementation when behavior is uncertain. Humans and agents should use the same current source of truth.

Inventory the components the real flow needs: actions, inputs/choices, navigation, data display and feedback/overlays as applicable. Reuse supported behavior before creating an almost-identical wrapper. An existing primitive can provide semantics while a composition carries the product's own expression; a component library's default look is not the design brief.

## Define anatomy and behavior together

For each changed family, identify root, label, leading/trailing accessory, supporting text, status and interactive regions. Give slots a purpose and document who owns spacing, focus, events and accessible naming. Only make a slot independently interactive when the product needs that separate action. Avoid nested buttons and clickable wrappers around links.

Keep emphasis/variant, size/density, state, theme and instance data independent unless the actual contract couples them. Prefer a small composable API to contradictory flags. Define controlled/uncontrolled ownership, value change versus commit, defaults, supported overrides and unsupported combinations where implementation needs them. A new option must be represented in the API, examples and applicable tests together; generated documentation can still be wrong.

Choose concrete checks from the family, not a universal state cross-product:

| Family | Contract questions | Small useful trial |
| --- | --- | --- |
| Actions | Button or navigation link? Pending and repeat activation? Who reports failure? | Submit twice, fail once, retry without losing input; label and target stay stable |
| Inputs/choices | Label, help and error association? Selection ownership? Composition input and commit? | Long text, invalid value, keyboard selection, cancellation and supported input methods |
| Navigation | Destination or local selection? Current state? Back context? | Navigate away and back; focus and selected object remain intelligible |
| Data display | Reading order? Empty/partial/stale content? Sorting or selection semantics? | Similar rows, long content, missing media and narrow layout through the real data seam |
| Overlays/feedback | Trigger ownership, focus entry/return, dismissal and nested layers? | Open, interact, Escape/Close, reopen; no lost focus or trapped page after dismissal |

Native controls and platform conventions are the starting point for behavior. Verify keyboard and assistive-technology semantics appropriate to the actual pattern; a menu, listbox and combobox are not interchangeable because they share a popup. Do not claim conformance from a component name or an automated accessibility scan alone.

## Customize at the right layer

Choose the least invasive supported layer: instance content/props → documented slot or class → semantic theme tokens → composition of exported primitives → owned source only when necessary. This is a diagnosis order, not a ban on custom design. If the required behavior cannot fit the abstraction, own a focused implementation rather than accumulate hidden overrides.

Trace primitive → semantic → component tokens through every affected supported mode. Check alias cycles, missing values, units and actual foreground/background pairs. Use [color](color.md) for state contrast and [typography](typography.md) for real strings. Separate density from touch-target and text-size needs. Do not shrink everything to create a compact mode. Scope theme boundaries and test inherited versus explicit values; a local brand surface should not accidentally recolor unrelated content.

Inspect control rhythm as a relationship: label baseline, icon mass, padding, gap, border and focus contour. Equal declared heights can look unequal beside different typography or marks. Use actual-content rows of neighboring controls and isolated states, then the real screen. Keep justified optical corrections local. Preserve expressive silhouettes where requested instead of normalizing everything into one neutral library aesthetic.

An owned fork/ejected component carries an upstream version, license, maintenance owner and a future comparison path. Do not copy dependencies, private code or artwork just because an upstream tool can export them. No installation or publication permission follows from this method.

## Make customization testable and discoverable

Maintain one compact component contract near the implementation: purpose, imports, anatomy, supported props/slots, reachable states, keyboard/focus behavior, tokens, examples, limitations and current version. Reuse an existing documentation format rather than adding a parallel database. Provide minimal working examples with real content and a difficult case. Keep source, public exports, docs and tests synchronized when the contract changes.

Test actual production components with synthetic product-shaped data through [interface craft](interface-craft.md). Cover relevant size/mode/state intersections rather than multiplying every flag. Include reflow, zoom, long labels, directionality, focus visibility, reduced motion and unavailable data where supported. Verify composition as well as primitives: a passing button does not establish a usable form or safe overlay stack. Track behavior, visual craft, accessibility and owner preference independently.

For shared updates, record affected consumers, additive versus breaking changes, migration examples and deprecations. Rerun the changed contract and representative downstream composition. Keep failures and untested states visible. A complete inventory or a generated screenshot is not evidence of working runtime behavior.

Use [iconography](iconography.md) for the meaning, drawing and control contract of icons. The optional [component brief](../templates/component-brief.md) is a scratch aid, not required paperwork for each edit.
