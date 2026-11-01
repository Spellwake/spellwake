---
name: quzzar-system-design
description: Design systems around data structures and clear inputs and outputs, using quzzar-flow to visualize data pipelines. Use when designing a new system or auditing or changing the architecture of an existing system.
---

# quzzar-system-design

Think in data structures and data pipelines. Use a functional programming perspective:
clear inputs, clear outputs, and an explicit description of how each transformation
turns its inputs into its outputs.

## Entry points

Identify which kind of work the user is requesting:

- Design a new system from scratch.
- Audit an existing system or make changes to it.

For a new system, read [references/new-system.md](references/new-system.md) before starting.
For an existing system, follow the phases below. Each entry point includes its own full
planning, implementation, and verification sequence. Implement only when requested; an
audit-only request ends with the audited map.

## Working directory and visualization

Create `.quzzar-system-design/` in the project being designed. Add
`/.quzzar-system-design/` to that project's `.gitignore`. Keep the diagram app, maps,
iterations, review reports, and checklist plan inside this ignored directory.

Use the published `quzzar-flow` package to render and show the full layout. Consume the
package; this work is not development of quzzar-flow itself. Let the user inspect nodes,
connections, and the data flowing through the system.

Keep these artifacts separate:

- `requirements.md` and `scope.md`: raw user input and the agreed scope for a new system.
- `system.md`: the full specification of the approved system.
- `current/`: the verified map of how the system works before changes.
- `proposed/`: a copy of the current map, iterated with the user.
- `plan.md`: the exhaustive checklist for reaching the approved proposed system.
- `reviews/`: independent audit, plan, and implementation review reports.
- `implemented/`: a fresh map of what the code actually does after implementation.

Preserve the verified current map as the baseline. Correct factual audit errors explicitly;
never overwrite it with proposed behavior. Keep the approved proposal available for the
final comparison.

Do not invent fallbacks or contingencies. Include them only when the user has explicitly
specified the behavior. Be very hesitant to introduce new application UI surfaces. If
considering one, explicitly ask the user whether it should be added before including it
in the design or building it. The quzzar-flow diagram is the visualization requested by
this workflow.

## Phase 1: full, honest audit

Do the deep investigation yourself so the user can understand the system at a higher
level without reading all the implementation details manually. Trace the actual code
and construct the full current-state map before redesigning it.

Map the full system under review:

- Database structures and their relationships.
- How the backend consumes those structures and transforms their data.
- How the frontend consumes the backend's data.
- Connections between systems, including the data passed through each connection.

Use groups to categorize meaningful sections, such as database, backend, and frontend,
while keeping their relationships visible. For each processing step, show what data
enters, what happens to it, and what comes out. Make unusual inputs, unexpected data
shapes, and convoluted transformations visible.

Represent what the implementation actually does. Record the code locations supporting
the map. Mark uncertainty instead of inventing connections or transformations.

If the system is convoluted, the diagram should reveal that. Do not hide awkward paths
or present an idealized architecture to make it look better. Exposing flaws is the point.
A full audit accounts for all known parts of the system under review; unexplored areas
must remain explicit gaps, not disappear behind a simplified diagram.

Render and inspect the map, run the independent review below against the actual code,
and correct omissions or inaccuracies. Show the user the full layout and explain the
structure, data flows, flaws, and unresolved questions.

## Phase 1.5: independent structure and efficiency review

After verifying the audit's accuracy, give a separate agent with a fresh context the
rendered quzzar-flow diagram, the graph definitions that produced it, the audit results,
and access to the actual source code. Have it inspect the diagram and cross-reference
the depicted structure and flows with the implementation. This is an additional review,
separate from the two accuracy checks below.

Ask whether the system is unnecessarily complex, convoluted, or inefficient, and where
the structure or data flow creates that complexity. Require concrete findings supported
by the graph and code. Distinguish structural inefficiency from measured runtime
performance; a messy diagram alone does not prove a performance problem.

Distinguish messiness in the actual system from poor diagram layout or presentation.
Improve presentation when it obscures the truth, while preserving every actual
relationship and awkward flow. Keep system flaws visible in the current-state map;
proposed structural improvements belong in phase 2.

Save this agent's findings in `reviews/` and present them alongside the full audit so
the user can use them during iteration. Report what the reviewer inspected and any
uncertainty rather than treating its judgment as proof.

## Phase 2: iterate with the user

Copy the verified current map into `proposed/`. Work with the user on this copy to change
the flow and structure. Render each iteration so the user can inspect what is changing
and compare it with the current system.

Keep existing facts and proposed behavior clearly distinguishable. Preserve the honest
baseline even when the proposed system is much cleaner.

Continue iterating until the user explicitly says the proposed design looks good.
Only then move to phase 3. Silence is not design approval.

## Phase 3: full specification and exhaustive checklist

Write `system.md` describing the approved system in full. Compare the actual code and
current map with the approved proposal.

Explicitly ask the user unless they have already answered for this change:

- Does the changed system need backward compatibility?
- Does existing work or data need to be migrated? If so, what migration flow do we want?

Add backward compatibility only when the user explicitly requires it. A migration
requirement does not imply permission to add compatibility layers or fallback behavior.
Resolve these questions before finalizing the relevant plan items.

Write `plan.md` as a full, exhaustive checklist of everything required to build the
approved system or turn the existing system into it. Cover changes to database structures and relationships,
backend processing, frontend consumers, contracts, connections, required migrations,
superseded code, and verification wherever those are affected.

Make each item concrete: what changes, where it changes, what it depends on, and how to
confirm it is complete. Account for every requirement in the approved specification and, for an existing system,
every difference between the actual implementation and the approved proposal. The plan must be sufficient to implement the whole change,
not just its most visible pieces.

## Phase 3.5: independently check the plan

Run the independent review below using the raw requirements, full specification, approved
proposal, and checklist, plus actual code and the current map for an existing system. Ask whether implementing every item would actually produce the approved
system, and what is missing, contradictory, or incorrectly assumed.

Revise the checklist to address verified findings and have the corrections checked.
Do not move to implementation with material gaps concealed or marked complete.
If a finding requires changing the approved design, return to the user in the applicable design-iteration step.

## Phase 4: implement

Implement the full reviewed checklist. Track completed work in `plan.md`, with the
verification supporting completion. Keep the implementation aligned with the approved
structure and the user's compatibility and migration decisions.

If implementation exposes a design change, return to the applicable design-iteration step for that decision, then
update and recheck the affected plan. Do not silently substitute a different system.

## Phase 5: full, honest audit of the result

Investigate the resulting code again and build `implemented/` from what it actually does.
Do not produce the final map merely by relabeling the proposal or ticking the checklist.

Render the implemented map and compare it with the approved proposal and every checklist
item. Run the independent review below against the resulting implementation.
Confirm that the data structures, relationships, inputs, outputs, transformations, and
required migration behavior actually match what was agreed.

Fix verified discrepancies and recheck the affected work. Show the user the final layout,
the comparison, and any remaining gaps. Report completion only when the implementation
and evidence support it; an unresolved finding is not a successful audit.

## Independent checks

At existing-system phases 1, 3.5, and 5, use two isolated subagents with fresh contexts. Give them the
relevant task, source code access, maps, and approval or checklist artifacts, without
feeding them your conclusions or each other's findings.

- One checks coverage against raw requirements and specification, and against code when
  it exists: omitted systems, structures, consumers, connections, and unsupported claims.
- One follows the data end to end: inputs, outputs, transformations, relationships, and
  whether the map, plan, or implementation fulfills the intended flow at that phase.

Require each reviewer to report evidence, gaps, and the limits of what they inspected.
Synthesize their findings only after both finish. Correct verified issues and have those
corrections checked before declaring the phase complete. Store the reports in `reviews/`.

If independent subagents cannot run, report that limitation explicitly. Do not present
self-review as an independent check or silently replace the required review process.
