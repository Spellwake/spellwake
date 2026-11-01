# Mobbin research before design

Use Mobbin to understand how shipped products let people complete a task. A screenshot can
show a control; a flow shows when it appears, what the user knows then, and what follows.

## Choose the research scope

For a new or materially changed multi-step journey, define the actor, task, entry condition,
successful outcome, platform, and product constraints. Search for that job, not just a page
name: for example, inviting a collaborator through accepting the invitation and gaining access.
Compare several relevant products, usually three when coverage permits. Prefer comparable
roles, permissions, task frequency, and platform over famous brands. Research separate actors
as separate paths. A focused component or visual polish task needs only relevant screen/state
research; it does not require an unrelated end-to-end study.

Discover the available Mobbin tools and read their schemas and any supplied research guidance.
Use flow search for journeys, screen search for states, and section search for website
composition when exposed. Tool names and response fields vary; do not invent them. Prefer
the connected integration. If unavailable, inspect the signed-in website through supported
browser tools. Confirm actual access before claiming inspection. Do not install, connect,
or buy anything silently; disclose unavailable access and the resulting research gap.

## Inspect sequences and write findings incrementally

Open the returned images and follow each flow in order. Search results, titles, metadata,
and a flow's thumbnail are discovery, not evidence of its behavior. Retrieve the remaining
steps when a result is partial. Inspect interactive or recorded transitions when available
and relevant; static captures cannot establish timing or keyboard behavior.

For each product record:

- Entry point, ordered steps, user actions, decisions, and completion feedback.
- Information requested at each step, defaults, disclosure, and when commitment occurs.
- Observed alternate paths, validation, permission boundaries, cancellation, and recovery.
- Flow and screen source links, platform, and capture date/version when available.

Write observations after each small batch so findings survive beyond the images' context.
Label partial coverage and distinguish observed states from inferred or proposed ones. Do not
invent missing error paths or assume the entire journey was captured. Common patterns show
conventions; they do not establish conversion, usability, or business outcomes.

## Produce a reviewable visual report before implementation

Show annotated screen sequences with arrows or step numbers, grouped by product, and durable
links back to every referenced screen or flow. Use source images only through supported
rendering/export mechanisms. If image embedding is unavailable, provide linked sources and a
sequence diagram, and explicitly mark the report's visual evidence as incomplete. A URL list
or moodboard without sequence and analysis is not a flow report.

Synthesize shared patterns and meaningful differences, then map the proposed product journey:

| Proposed step or decision | Observed precedent and source | Adopt / adapt / omit | Product-specific reason |
| --- | --- | --- | --- |

Show the coherent proposed sequence, including actor handoffs and required recovery paths.
Explain deviations in terms of the product's users, data, permissions, and constraints. Keep
one consistent navigation, terminology, and save/commit model when combining useful patterns
from several products. Preserve the project's components and tokens rather than importing
other brands' styling. Do not add steps merely because a reference has them.

Put the report in the repo's ignored review-artifact location or the host's artifact directory,
and expose it to the user before implementation. This is evidence, not a new approval gate:
continue within existing authorization unless a consequential product decision needs an answer.
Retain durable findings in the existing feature record or surface brief and reference that
record from subsequent design work. Cite durable Mobbin page URLs returned by the service;
expiring image delivery URLs are not source citations.

## Handoff and verification

Carry the proposed sequence, adopted patterns, intentional deviations, and evidence gaps into
Impeccable's applicable workflow. Walk the implemented journey against that map, including
transitions and recovery, as well as inspecting individual screens. Report material divergence
and its reason; a polished screenshot alone cannot verify a journey.

Official examples: [Mobbin MCP](https://mobbin.com/mcp) and
[research use cases](https://mobbin.com/blog/how-to-use-mobbin-mcp).
