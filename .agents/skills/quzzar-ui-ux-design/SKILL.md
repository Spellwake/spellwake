---
name: quzzar-ui-ux-design
description: Design UI section by section through concept choices, early user checkpoints, detailed section walkthroughs, independent coverage review, faithful implementation, and user-reviewed consolidation. Use for new or redesigned screens, components, and flows.
---

# Quzzar UI/UX Design

Use this flow whenever designing or building new UI, including a new component or state in an
existing app. Read `quzzar-workplace` and `quzzar-frontend` for implementation conventions.
Existing and absent component libraries follow the same design process. Existing components
inform references and reuse; they do not justify replacing the approved design with something
easier to build. Final appearance-changing consolidation requires the user's decision.

This skill is self-contained. It does not require Impeccable, Matt Pocock's skills, a particular
coding agent, or a proprietary launcher. Discover the active host's image, browser, question,
and independent-agent capabilities. Use its image-generation tool for the required mockups;
do not silently substitute coded sketches or skip images when generation is unavailable.
Report the limitation and keep dependent work pending. Use available question tools for user
clarification; ask in chat if the host has none. Agent reviews never substitute for user approval.

## Work in sections with two user checkpoints

Split a substantial journey into user-recognizable sections, such as a library, lobby, or
import flow. Keep a short section plan with the current section, its next decision, and what
is deferred. A small component can be one section. Reuse settled decisions and approved work.

For each section, follow this loop:

1. **Concept:** show a few meaningful layout/interaction options at representative content.
   Explain the decision they test. Ask the user to choose or revise the concept, then wait.
2. **Detail:** flesh out only the chosen section: its controls, relevant states, reusable UI
   pieces, images and button destinations. Check this section independently, show its complete
   walkthrough, ask whether it is right, then wait before expanding the next section.

Direction approval does not approve the detailed section. Section approval does not approve
unseen sections. Record the exact scope and revisions of each decision. If the user has already
approved a section concept, proceed to its details without repeating that checkpoint.

**Show the concept before generating its exhaustive state set.** User feedback is part of
design development, not a final ceremony after the whole application is drawn. Do not delay
early user review until a whole-package verifier passes. Delegate only work within the current
approved phase; parallel generation must not run ahead of the user checkpoints.

When feedback changes the journey, return the affected section to its concept or detail
checkpoint. Update the brief and defer dependent imagery. Keep unchanged approvals; preserve
superseded evidence outside the active walkthrough. Exhaustive coverage still matters after
the concepts are settled, within each agreed section and finally across their connections.

## 1. Establish the brief and one target viewport

Ask about the audience, task, visual feel, references, content, constraints, and what must stay.
Ask for reference images when useful. Inspect the existing product, tokens, assets, and component
gallery if present. Ask whether that library should be the visual reference and which aspects
may change. Do not assume an existing component's style is the user's chosen direction.

Choose one initial medium with the user: mobile for a mobile-first app, normally desktop for
a website, or tablet when appropriate. Record actual viewport dimensions. Keep this round at
that viewport; additional widths and device-specific paths are a later, explicit design pass.
Do not claim responsive or native-platform completion from one viewport's approval.

Record settled answers and unanswered questions. Preserve the user's scope and terse copy.
Ground the direction in the real product and content, with intentional hierarchy, typography,
spacing, contrast, and imagery. Do not add decorative controls or explanatory prose to fill space.

## 2. Develop the current section concept

When a visual direction has not yet been selected, generate **three distinct sample images**
of the current section at the same content and viewport. With an approved visual direction,
keep that visual language
and show a few alternatives where composition or interaction is still undecided. Use one
targeted revision when the user has already specified the concept; do not manufacture choices
or redraw approved sections to satisfy a quota.
Vary meaningful composition and visual treatment rather than just recoloring one layout. Where
an existing visual system should carry over, use captures of its real components as image
references and say which attributes should carry over. Do not copy unrelated reference content.

Inspect all images. Present them together in the [review workspace](references/review-workspace.md)
so the user can compare them. Ask which direction or combination to pursue and what to change.
Wait for their answer. If they select a concept and ask to flesh it out, record that approval
and proceed to section detail. If they request changes, regenerate only the affected concept
and iterate until they approve it. Stop at that checkpoint before generating
its detailed state set or concepts for subsequent sections. Preserve approved revisions and the actual approval evidence;
do not overwrite the reference while claiming its old approval still applies.

Choosing a direction approves its visual direction, not every invented action, screen, or state
an image model might have drawn. Identify such elements during UX discovery instead of adopting
them as requirements. Do not begin product UI implementation during this image iteration.

## 3. Resolve and inventory the chosen section

After this section's concept approval, ask detailed, adaptive questions about navigation, menus, dialogs,
forms, selectors, buttons, dismissal/back actions, and the effect of each interaction. Clarify
loading, success, empty, validation, permission, failure, and boundary states where relevant.
Ask as many substantive questions as needed in manageable batches. Do not ask settled questions
again, impose a question quota, or invent failure behavior to avoid asking.

Give a separate agent the actual selected image, raw user answers, product constraints, and
current section boundary. Have it inventory **every required visibly distinct view/state in
this section**, including menus, overlays, and intermediate states, with evidence and questions.
Record cross-section entry/exit contracts and defer the destination section's detailed design.
Use [reviewer tasks](references/reviewer-tasks.md). Resolve gaps with the user, then keep stable
view IDs and an action-to-destination/state map. A close/back action can point to an already
pictured state. A required view cannot be dismissed because it is not a full page.

## 4. Flesh out, verify and review this section

Generate images for the current section's inventoried views/states, anchored on its approved concept and
shared references. Include relevant open-menu, focus/hover/selection, loading, and other agreed
states. Reuse unchanged evidence and equivalent data states; do not generate Cartesian name,
count or setting permutations. Inspect each image yourself, then give **each image** an independent verification pass
against the user answers, its incoming actions, and this section's inventory. Reviewers must receive
the actual images, not descriptions of images. Reviews can be assigned in bounded batches, but
every image needs its own findings and every page later needs its own implementation verdict.

The verifier enumerates every visible interactive element and every way it changes the UI.
Every in-section action needs a specified outcome and image evidence for the resulting visible state.
No dangling links, invented features, unpictured popovers, or decorative buttons that do nothing.
For a nonvisual action or external destination, resolve the expected visible feedback/handoff
with the user and record it; do not invent an internal screen or pretend the action is navigation.
For a destination in a deferred section, record the entry/exit contract and its pending status;
destination imagery is due during that section's detail phase. A section verdict covers that
boundary contract, not the unseen destination. The final integrated review must verify every
required destination or unchanged external-module handoff before declaring full-flow coverage.

Present the section's complete walkthrough, UI pieces and action destinations in the review
workspace. Clearly mark deferred sections and genuine unresolved questions. Ask whether this
section is right and wait for the user's answer before expanding another section. When a
coverage finding changes a concept or adds a material interaction, show that decision to the
user before generating its dependent images. Checkpoints expose uncertainty early rather than
holding back all work until it is exhaustive.

Product UI may implement only the approved detailed scope with settled entry/exit contracts.
An unreviewed section remains pending. If the task is design-only, stop at its requested design
checkpoint. Review-app code is design tooling, not product implementation.

### Close coverage within sections, then across the agreed flow

Use the coverage loop below for each detailed section. Once the agreed sections are designed,
give a verifier the **entire current package**: all images, the view inventory, action map,
raw UX answers, section approvals and scope. Its job is to discover omissions across the
whole flow, not only confirm that the supplied list of screens looks plausible. Ask what
screens, tabs, controls, intermediate states, or destinations are still missing, including
required actions that are absent from the images and visible controls with no defined result.

For each finding, either generate the missing view/state or propose removing/changing an
unnecessary control. Resolve product decisions with the user; do not silently remove an
approved requirement merely to close coverage. For accidental, unapproved image-model controls,
remove them from the affected image and map rather than inventing a feature to justify them.
Record each finding, chosen resolution, and the resulting image/map revision.

Hand the **updated whole package back to the verifier**. Repeat finding, correction, generation,
and resubmission until it explicitly reports that all required views/actions are covered, with
no unresolved coverage findings or uninspected material. Fixing a list yourself is not a passed
re-review. The verifier's verdict must name the current package revision; an earlier pass does
not certify later edits. Present integration findings and changed paths to the user for approval;
unchanged section approvals carry forward. A whole-flow pass does not retroactively approve
unseen designs or replace either section checkpoint. User revisions that affect coverage return
through affected-section review and the integration check before claiming the flow complete.

Keep the integrated walkthrough available for the user to inspect, critique and revise. Show
the current section first, with a small concept comparison or detailed section walkthrough as
appropriate. Keep broader coverage artifacts inspectable without making an exhaustive app-wide
image list the user's first feedback opportunity. Report completeness only for reviewed scope.

## 5. Inventory and build the faithful component library

Have a separate reviewer inspect **all approved images in the current implementation scope** and identify repeated UI pieces,
component families, tokens, variants, states, and assets. Map every image region to a component
or deliberate screen-level composition. Read [component libraries](references/component-libraries.md)
and [asset production](references/asset-production.md) before building.

With a library, reuse components that faithfully fit and extend it without breaking existing
consumers. Without one, build a scoped library for this approved set, following repo conventions.
Build the full set of components and gallery states needed by the approved screens before
assembling screens. Reuse logic and accessible primitives throughout. Extend variants or compose
components when necessary to reproduce approved differences; do not force a stylistic compromise
or create one component per screenshot fragment. These extensions need no extra approval when
they preserve the approved outcome and existing authorization.
For an incremental build, this means the approved section's components; later approved sections
extend the same library without requiring speculative components for deferred designs.

## 6. Assemble and independently verify each screen

Implement one approved screen/state at a time using those components. Wire approved actions to
the typed client functions defined by System Design where applicable. Capture the actual
rendered implementation at the reference viewport and matching content/state. Give a reviewer
the approved image and current capture, plus matching region crops when useful. Retain full-screen
context so local comparisons cannot conceal whole-page drift.

Require a distinct verdict for each page/state: what matches, what differs, the precise regions,
and the remaining fixes. Iterate with fresh captures until it faithfully matches the approved
reference and the interactions work. Measure geometry and inspect typography, spacing, colors,
assets, content, states, and accessibility. Do not claim perfect fidelity from memory, a source
review, a favorable numeric score, or a reviewer who did not see the images.

If a generated reference is contradictory or cannot be reproduced coherently, surface the exact
conflict and proposed correction to the user. Expand the component library where that solves it.
Otherwise obtain approval for the changed reference; do not silently reinterpret it. Disclose
remaining rendering differences or agreed tolerances instead of calling an approximate match
perfect. Check shared-component edits against every affected approved screen and existing use.

## 7. Propose consolidation and let the user choose

Only after the faithful implementation is verified, give a fresh agent the approved images,
verified renders, component inventory, and actual implementation. Ask for consolidation that
makes the library maintainable without collapsing different behaviors or changing UX semantics.
Routine deduplication that preserves the approved result remains normal engineering work.
Appearance-changing standardization is a **proposal**, not automatic permission to replace it.

Build and render an isolated, reviewable consolidated candidate. Preserve the faithful build
for comparison without committing divergent permanent production libraries. Verify that the
candidate keeps the approved action map and behavior. In the review workspace show, per screen:

1. The approved reference image.
2. The verified faithful implementation capture (with any disclosed differences).
3. The proposed consolidated implementation capture and specific visual changes.

Also show two component-gallery views: the library needed for faithful reproduction and the
recommended consolidated library, including changed variants/states and affected screens.
For an existing library, explain effects on existing components and consumers. For a new one,
show the proposed baseline. Explain concrete reuse/maintenance gains and visual tradeoffs.

Ask which version or specific changes the user approves. Wait; silence, an agent's preference,
or a default selection is not approval. Apply only accepted appearance changes, verify affected
screens again, and remove rejected temporary implementation code. Keep reference images,
captures, decisions, and useful review evidence. Ship one chosen implementation and library;
report the faithful, proposed, accepted, and still-pending work distinctly.

## 8. Review completion and restart for each additional device target

At final presentation, ask whether the complete current-target package looks right. Show which
targets are complete and explicitly name any still outstanding: for example, desktop complete;
mobile and tablet not yet designed. Do not call a primary-viewport result the complete
cross-device design or silently omit a requested target.

For each next target, **restart this skill from Step 1** with that device/viewport as the new
primary medium. Reuse settled product requirements, approved visual identity, assets, and
component work as context; ask new target-specific questions instead of repeating settled ones.
Discuss navigation, information density, touch/pointer use, menus, and flows for that target.
Develop target-specific directions, then repeat the section concept/detail checkpoints,
inventory and verify each chosen section, and run the integrated coverage loop before completion.
Build/verify faithful screens and present consolidation for that target. Do not treat CSS
shrinking, a device-frame mockup, or an automatic breakpoint as a substitute for this process.

Keep the device passes in the same review workspace with target-labeled images, action maps,
approvals, and implementation evidence. After shared-library changes, recheck already-approved
targets for regressions. Preserve one shared production application and component system; a
new device pass does not authorize a permanent platform fork. Continue targets already within
the user's requested scope; ask which target to add next only when that scope is not settled.
