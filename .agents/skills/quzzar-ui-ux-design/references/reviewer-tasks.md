# Independent review tasks

Use separate agents with fresh contexts for these roles. Supply raw requirements and current
artifacts, not your preferred verdict. Reviewers report evidence, gaps, and what they could
not inspect. They do not approve design decisions or invent product behavior. If independent
agents cannot run, report the limitation; self-review is not an independent review. Continue
work that does not depend on the missing review and keep that checkpoint pending.

## View inventory

Inputs: selected direction image, raw user messages/answers, agreed scope, and existing product
constraints, plus the current section and phase. Open the actual image. At concept review,
check the proposed decision and obvious contradictions; defer the exhaustive state inventory.
After the user selects this section's concept, produce the complete list of its required screens,
menus, dialogs, panels, feedback states, and other visibly distinct views at the chosen viewport.
For every item give its trigger, purpose, state/content differences, and supporting requirement.
Flag unresolved behavior, image-model inventions, and contradictions rather than filling them
with assumptions. Enumerate visible controls and map each to a destination or specified action.
Keep deferred sections as entry/exit contracts. Do not expand their detailed designs before
their own user concept checkpoint.

## Image and interaction coverage

Inputs: a particular generated image, the current section's inventory and available images, action map,
raw UX answers, and approved direction. For that image enumerate every visible button, link,
menu item, clickable icon/region, input, selector, and state-changing affordance. Include opening,
closing, canceling, submitting, changing selection, validation, loading, and outcomes as applicable.

Return rows with source view, control/trigger, intended action, destination/result state,
image evidence, requirement evidence, and any missing decision. A destination already in the
set is valid evidence; a text promise cannot stand in for required in-section imagery. An explicitly
deferred section uses an entry/exit contract until its detail phase; mark its destination pending
and outside this section's verdict, then require evidence during integrated review. Inspect intermediate
states too. Flag implied click targets without behavior and behavior without a pictured outcome.
Separate genuinely distinct views from equivalent states; do not demand every combinatorial
permutation of identical controls or invent pages simply to fill the map. Ensure long/empty/error
content the user agreed to can be represented coherently. Recheck after affected image edits.

## Whole-package coverage loop

Inputs: the entire current section image set, inventory, action map, raw answers, scope, package
revision, phase, section approvals and previous finding/resolution log. Once the agreed sections
are designed, repeat across their integrated flow. Discover views and actions independently; do not assume
the supplied inventory is exhaustive. Trace every path across screens, tabs, menus, overlays,
and intermediate states. Also check whether the stated requirements need a control or entry
point the images do not provide. Return new omissions and verify every previously corrected
finding against the actual revised images and map.

Require an explicit verdict for the current revision: coverage complete, findings remain, or
inspection incomplete. Name the missing controls/views/images and incoming/outgoing actions.
A recommendation to remove a control is a proposed resolution, not permission to discard user
requirements. The author resolves findings, regenerates images or removes unnecessary controls
as authorized, updates the package, and resubmits it. Repeat until the current entire package
has been inspected and no coverage findings remain. User changes that alter coverage trigger
another affected-section check and integration pass. Early concept and section reviews happen
before this final integration gate. A coverage verdict never substitutes for the user's approval
of the detailed scope being implemented.

## Component inventory

Inputs: the entire approved image set, approved UX map, existing component APIs/gallery and
constraints if any. Identify component families, repeated anatomy, variant/state differences,
layout compositions, tokens, and independent artwork layers. Map occurrences back to view IDs
and image regions. Mark reuse, extension, or new component choices. Preserve the approved
appearance; do not apply final visual standardization during this inventory. Flag contradictory
images and genuine implementation questions with their locations.

## Per-screen implementation fidelity

Inputs: one approved view image, an actual current rendered capture of that state at the same
viewport/content, component mapping, relevant requirements, and access to the running preview
where available. Open both images. Compare the whole screen and corresponding crops; identify
geometry, type, spacing, colors, assets, control states, missing content, and interaction issues.
Give specific findings and a verdict for this view, with any uninspected areas. Check the action
map through actual interaction when tools permit. Static images cannot prove keyboard behavior,
focus management, animation, network effects, or native integration. Do not accept a screenshot
of the reference displayed in the app as evidence of a working semantic implementation.

## Consolidation proposal

Inputs: all approved references, faithful rendered screens and gallery, verified action map,
actual component/source inventory, and current public component contracts. Find redundant
implementations, near-identical variants, repeated tokens, and opportunities for clearer APIs
or composition. Distinguish exact-output refactors from proposals that change appearance.
For each proposal name affected components/screens, the before/after behavior and appearance,
what becomes shared, the maintenance benefit, and regression checks. Do not merge semantically
different states or hide visual changes under a refactor label. The user chooses the visible
tradeoffs after seeing rendered comparisons and both galleries.
