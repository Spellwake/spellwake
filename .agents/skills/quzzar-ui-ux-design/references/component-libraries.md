# Component library design and consolidation

The workflow is the same whether the project has a library or not. Existing libraries supply
real reference images, tokens, APIs, and reuse opportunities. Preserve the section concept/detail
checkpoints, scoped state coverage and faithful implementation review; reuse approved visual
direction rather than repeating three options for settled choices. Do not fork an existing
system silently. Keep changes scoped to the approved work and check affected existing consumers.

## Model components by anatomy and behavior

Start from the reviewer inventory across the approved image set in the current implementation
scope. For an incremental build, cover the approved section and extend the same library as
later sections are approved; deferred concepts do not require speculative components. Use families such as
buttons, icon buttons, links, inputs, selectors, menus, tabs, dialogs, and feedback only where
they are needed. Separate shared primitives from screen-level arrangements. Do not create a
universal component with arbitrary style escape hatches, nor an exhaustive kit nobody uses.

For each component document:

- Its purpose, anatomy, typed props/events, content slots, and occurrences in approved views.
- Variants needed by those views: for example primary, secondary, outlined, or ghost buttons.
- Relevant sizes and interaction states: normal, hover, pressed, keyboard focus, disabled,
  selected, loading, invalid, expanded/open, and empty as applicable.
- Behavior independent of appearance: selection persists; hover is transient; focus identifies
  keyboard position; loading and disabled have specified interaction consequences.
- Tokens for typography, color, spacing, radii, elevation, and motion, with semantic roles.
- Accessible names, keyboard behavior, focus management, contrast, and reduced motion.

Selectors need their open popup and option states, not only their closed trigger. Dialogs need
their trigger, content, dismissal rules, and focus return. Loading, invalid, and disabled states
are not interchangeable. Use accessible native elements or the project's primitives; do not
reproduce a button or label as pixels in a screenshot. Keep data, routing, and business logic
outside the shared presentation library, per `quzzar-frontend`.

## Build and verify the faithful set

Reuse existing components and tokens when they match. Add a justified variant or extension
when the approved image requires it. Follow repo component/export conventions, strict types,
and JSDoc for public props and non-obvious behavior. Build the components needed by the approved
set before composing screens. Keep artwork as independently reusable assets where appropriate.

Use the existing component gallery, or build one scoped to this set if none exists. Render
variants and states in an inspectable matrix, including realistic and long content where needed.
Gallery-only state controls may make hover/focus examples reproducible, but also test real
pointer/keyboard interaction; forced preview styling does not establish accessibility.

A component needed once can still belong to this scoped library when the approved composition
requires it. Do not wait for a second screen by duplicating its behavior, and do not promote
unrelated one-off layout details into a global design-system API.

## Consolidate after faithful verification

Share exact-output logic/styles routinely. For visual standardization, compare the faithful
and proposed sets: which variants merge, which tokens normalize, which APIs simplify, and
which screens change. Preserve behavior and accessibility. Demonstrate the candidate through
both its gallery and every affected screen, not a written promise.

The final review includes the reference-faithful gallery and consolidated gallery. Approval
can accept individual changes or keep the faithful version. If nothing worthwhile changes
appearance, report that outcome and show the unchanged candidate rather than manufacture a
visible difference. Do not merge speculative variants or remove an approved distinction just
to reduce component count. After the user's decision, retain one maintained production library
and remove rejected preview implementations; historical images remain evidence.
