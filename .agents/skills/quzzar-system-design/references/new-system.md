# New systems

Use the shared `.quzzar-system-design/` workspace and functional, data-oriented approach
from SKILL.md. Keep the scope, raw user requirements, design iterations, decisions,
reviews, and implementation plan in that ignored folder.

## Step 1: listen, document scope, and clarify

Let the user explain the idea. Do not jump into implementation, diagram construction,
or mockup building while they are still describing what they want.

Capture their original messages and answers faithfully in `requirements.md`, separate
from your interpretation in `scope.md`. Preserve corrections and later decisions so a
reviewer can compare the design with what the user actually said.

Write the project scope: what the system should do, how it should work, its boundaries
and limitations, and the intended structure. Think in data structures, clear inputs,
outputs, and transformations. Identify what is known, assumed, or still unanswered.

Once the user has given a clear summary, say: "Before I build anything, here's what I
understand." Lay out your understanding, then ask five substantive questions about the
remaining gaps. Use the question tool when available, splitting the five questions
across calls if the tool limits how many fit. Let the user answer; do not send later
batches before answers to the earlier batch can inform them.

Five is the initial clarification round, not a ceiling. Continue asking further questions
when needed until the user confirms you understand the whole idea and you can lay out
all its points accurately. If fewer than five genuine gaps remain, use the remaining
questions to confirm concrete boundaries or consequences; do not invent uncertainty or
add features to fill a quota.

Update the scope and requirements with the answers. Do not leave this step or start
building the design until that shared understanding is explicit.

## Step 2: design the structure

Build the proposed system in `proposed/` using the published quzzar-flow package. Show
relationships, groups, data structures, inputs, outputs, transformations, and data flowing
between components. Keep scope requirements traceable to the relevant parts of the map.

For UI/UX, include low-fidelity screens or states inside nodes and label connections
with the actions or conditions that lead between them. Use image blocks for wireframes
or screenshots, alongside schema, text, JSON, or table blocks as useful. Read
[ui-journeys.md](ui-journeys.md) for supported capabilities and a runnable consumer example.
These are design artifacts, not permission to build the product UI.

## Step 2.5: independent design review

Give a fresh reviewer the user's raw requirements and answers, your scope interpretation,
the graph definitions, and the rendered diagram. Do not supply your conclusions about
whether the design is good.

Have the reviewer assess whether the structure is appropriate for what the user actually
requested, whether the inputs and outputs make sense, and whether requirements have
been omitted, distorted, or invented. Require specific evidence and open questions.

Iterate with the reviewer on grounded corrections. Ask the user about unresolved intent
rather than letting the reviewer decide it. Then present the reviewed quzzar-flow design
to the user.

## Step 3: iterate with the user

Spend the time needed here. Ask whether the structure and flow match what the user means,
make changes to the quzzar-flow design, and show the updated layout. Explicitly ask about
anything potentially missing instead of quietly expanding the system.

Record new answers and decisions in the requirements and scope. Keep assumptions visible
until the user resolves them.

## Step 3.5: independent completeness and edge-case review

Give a fresh reviewer the complete raw requirements and answers, updated scope,
decisions, graph definitions, and rendered design. Ask what is missing and where the
design has shortcomings, including user journeys, failure paths, and data limits.

Specifically inspect applicable cases such as:

- Zero states: no records, no results, or no profile picture.
- Large states: too much data, large lists, or excessive content.
- Long text, including an unusually long user name.
- A profile picture being present versus absent.
- Upload boundaries, such as a picture exceeding a discussed 10 MB cap. That number is
  an example, not a default limit to impose on every system.
- Failed login and failures in the systems needed to complete a user journey.

The reviewer identifies unanswered behavior; it does not authorize answers. Do not
invent fallbacks, contingencies, retry behavior, substitute data, or extra screens.
Surface each relevant issue to the user and get their explicit answer about what should
happen. Map the agreed behavior and document limits, assumptions, and any explicitly
requested fallback or contingency. Silence and reviewer suggestions are not approval.

Return to step 3 to resolve findings with the user, revise the design, and run step 3.5
again. Continue until the review is exhaustive for the agreed scope, material questions
are resolved, and the user explicitly confirms the reviewed design. Design approval
predating new review findings does not resolve those findings.

## Step 4: full specification and exhaustive implementation checklist

After the user explicitly confirms the reviewed design, write `system.md` describing the
approved system in full: structures, relationships, contracts, transformations, data flows,
UI journeys where relevant, limits, and the failure behavior the user actually agreed to.

There is no current-state map to fabricate for a new system. If existing work or data
must be accommodated, explicitly ask the user unless already answered:

- Does the system need backward compatibility?
- Does existing work or data need to be migrated? If so, what migration flow do we want?

Add backward compatibility only when explicitly required. A migration requirement does
not authorize compatibility layers or fallback behavior. Resolve applicable questions
before finalizing those plan items.

Write `plan.md` as a full, exhaustive checklist of everything needed to build the approved
system. Cover database structures and relationships, backend processing, frontend
consumers, contracts, connections, user-agreed failure behavior, any required migrations,
and verification wherever applicable.

Make each item concrete: what is built, where it belongs, what it depends on, and how to
confirm it is complete. Account for every requirement in the approved specification.
The checklist must be sufficient to build the whole system, not just its visible pieces.

## Step 4.5: independently check the plan

Run the two independent checks below using the raw user requirements and answers,
confirmed scope, full specification, approved graph, and exhaustive checklist. Include
existing code only where the design actually depends on it.

Ask whether implementing every item would produce the approved system, and what is
missing, contradictory, or incorrectly assumed. Revise the checklist to address verified
findings and have those corrections checked. Do not begin implementation with material
gaps concealed or marked complete.

If a finding changes the approved design or requires a user decision, return to step 3,
then repeat the affected design review and planning checks.

## Step 5: implement

When implementation is requested, build the whole reviewed checklist. Track completion
in `plan.md` with the verification supporting each completed item. Keep the code aligned
with the approved structure, contracts, user journeys, limits, and decided failure
behavior, including any explicit compatibility or migration requirements.

If implementation exposes a design change, return to step 3 for that decision, update
the specification and checklist, and recheck the affected plan. Do not silently substitute
a different system or invent a fallback.

## Step 6: full, honest audit of the result

Investigate the resulting code and build `implemented/` from what it actually does.
Do not produce the final map merely by relabeling the proposal or ticking the checklist.

Render the actual system in quzzar-flow and compare it with the approved proposal,
full specification, raw requirements, and every checklist item. Run the two independent
checks below against the resulting code and diagram.

Confirm that the structures, relationships, inputs, outputs, transformations, user
journeys, and decided failure behavior match what was agreed. Verify any required
migration behavior rather than assuming a completed checklist proves it.

Fix verified discrepancies and have the affected work checked again. Show the user the
final layout, the comparison, and any remaining gaps. Report completion only when the
implementation and evidence support it. Unresolved findings are not a successful audit.

## Independent plan and implementation checks

At steps 4.5 and 6, use two isolated subagents with fresh contexts. Give them the relevant
raw requirements, scope, specification, approved graph, checklist, and source access where
code exists. Do not feed them your conclusions or each other's findings.

- One checks coverage against requirements and specification, and code when it exists:
  omitted systems, structures, consumers, connections, and unsupported claims.
- One follows the data end to end: inputs, outputs, transformations, relationships, user
  journeys, and whether the plan or implementation fulfills the agreed system.

Require each reviewer to report evidence, gaps, and the limits of what they inspected.
Synthesize findings after both finish. Correct verified issues and have corrections
checked before declaring the step complete. Keep the reports in `reviews/`.

If independent subagents cannot run, report the limitation explicitly. Do not present
self-review as an independent check or silently replace the required review process.
