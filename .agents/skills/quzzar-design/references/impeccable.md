# Impeccable in Quzzar visual work

Use the installed official `impeccable` skill. Its current playbooks own the command details;
this reference defines when Quzzar work uses them. Do not maintain a second copy of upstream
instructions or invoke every command on every edit.

## Start with context

Read the installed skill's setup and routing before choosing a playbook. Run its bundled
`impeccable context` launcher once per session for the affected app, from the project cwd
specified by that installation; use `impeccable.cmd` on Windows when `sh` is unavailable.
Resolve a named target before invoking it. In a monorepo, honor target-selection output and
load the affected workspace instead of treating a root-level report as app context. Ask only
when the task and repo evidence do not identify the app. Follow any required target retry.

Read the repo's product context, design decisions, and rejected directions. `PRODUCT.md`
holds audience, tasks, and constraints; `DESIGN.md` and `.impeccable/design.json` capture
the visual system. Reuse them rather than
asking the same discovery questions each session. Preserve the installed playbooks as upstream
files; fix Quzzar's integration here rather than patching a vendor skill that an update replaces.
If Impeccable is missing, follow setup's installation step. On launcher failure, make the
installed skill's required disclosure, read existing context directly, and continue permitted
work without pretending context loaded. Do not run init, document, or doctor repairs as an
unrelated side effect. Use their playbooks when the task requires or authorizes that work.

Commands below are agent-chat operations: `/impeccable <command> <target>` in Claude Code,
`$impeccable <command> <target>` in Codex. Read the corresponding installed playbook and carry
out its steps through the tools available in the session. Typing a command name in a report
does not run a review. The bundled launcher's helper subcommands are terminal operations;
use only those documented by the installed playbook.

## Design and refine

| Situation | Use Impeccable for |
| --- | --- |
| New surface or substantial redesign | Its new-design workflow; `shape` when planning is the deliverable |
| Several versions of a named element | `generate <count> <action> <element>` for live comparison |
| First rendered implementation or an unclear visual problem | `critique`, including its AI-pattern assessment |
| Weak hierarchy, spacing, or typography | `layout` or `typeset` |
| Clutter or excessive decoration | `distill` or `quieter` |
| Bland composition with room for expression | `bolder`; `delight` for a meaningful signature moment |
| Unclear labels or instructions | `clarify` |
| Responsive layout or difficult states | `adapt` or `harden` |
| Finishing a feature or substantial visual change | `polish` for craft and `audit` for implementation quality |

These commands are tools for the current problem, not a checklist of effects to add. A task
interface earns distinction through hierarchy, density, and precise feedback; a marketing
page can give more space to expressive imagery and composition. Keep established product
identity during refinement. Respect the repo's tokens, accessible primitives, architecture,
and existing behavior when applying upstream suggestions.

Read and execute the selected playbook, including its prerequisites, helper scripts,
persistence, isolation/delegation when required and available, and completion conditions.
Before any UI edit, load its craft-floor reference. New-work, local refinement, shape,
critique, and audit have different contracts: shape returns a brief; critique and audit
report findings rather than silently implementing every suggestion. An authorized build or
fix task can act on in-scope findings without asking again. Existing user instructions and
authorization take precedence over redundant confirmations in a playbook.

Do not flatten every task into a mandatory critique/polish/audit chain. Use the installed
workflow's review requirements for builds and substantive changes, the requested diagnostic
for review-only work, and a focused refinement plus rendered verification for a narrow fix.
Follow the selected playbook's detector rules even when they differ from context's general
guidance. If an isolated review is required, run it when tools and governing instructions
allow; otherwise disclose the degraded run as that playbook requires. Never report an
inline self-review as an independent assessment.

## Explore multiple directions

Treat “eight versions,” “different directions,” or “give me options” as a request to explore.
Honor the requested count and fidelity: concept sketches, image mockups, and working screens
are different deliverables. A request for working versions needs working versions. Use the
brief to choose useful directions autonomously when the user has invited exploration.

- **For variants of an existing element**, use Impeccable `generate` with the requested count,
  a suitable action, and the named target. For example, `/impeccable generate 8 polish the
  pricing cards` in Claude Code, or `$impeccable generate 8 polish the pricing cards` in Codex.
  Check the installed playbook for current limits and group larger requests without reducing
  the requested total.
- **For whole-page concepts or new identities**, use the new-design/shape workflow to develop
  the requested directions. Give each a clear premise grounded in the subject, a composition,
  typographic character, imagery approach, and signature. An exploration of a new identity
  can change those axes; variants inside an established system use the freedom that system
  actually allows.
- **Make the differences legible.** Vary structure, hierarchy, density, image treatment, or
  interaction where the brief permits. Compare the silhouettes before investing in detail;
  eight palette swaps of one layout do not satisfy a request for distinct directions. Keep
  the product facts and task comparable so differences come from design rather than claims.
- **Refine every delivered candidate.** Use `layout`, `typeset`, `bolder`, `quieter`,
  `distill`, or `delight` to strengthen its own premise, then `polish` its execution. Judge
  each against its intended character so refinement does not make them all converge. Use
  `critique` to catch weak or repetitive candidates and replace them before presenting.
- **Make selection easy.** Show the requested alternatives at consistent sizes with concise
  names and the main tradeoff of each. Let the user choose before replacing the final
  surface. Preserve the chosen direction in the design context for subsequent work.

During `generate`/`live`, follow the installed preview and acceptance protocol: the live
overlay is the review channel while temporary variants are mounted. Run final screenshot,
audit, and detector checks after acceptance and cleanup, not against transient variant
wrappers. For independently delivered working versions, verify every candidate. Clean up
preview scaffolding through the supported workflow so it does not become application code.

## Mockups and finished assets

Use a real image-generation tool when the direction benefits from visual exploration or a
specific illustration, photo, texture, or cutout. Prefer the host's built-in image tool and
load its image-generation skill. A mockup is a composition target; controls, text, responsive
layout, and accessibility still need implementation and verification in the running app.

Follow the user's selected build path. When visual ambition matters and image generation is
available, offer Impeccable's mockup-first workflow. Build directly when that fits the task.
Both paths can generate finished assets. Do not create credentials or switch to a separately
billed API merely because the built-in tool is absent or fails; report the constraint and
resolve that choice with the user. See the [image-generation
guide](https://impeccable.style/docs/image-generation/) for current setup details.

## Detect AI patterns and verify the result

Respect the user's hook settings. Check hook status through the installed hooks playbook
when feedback appears absent or setup changed. A manifest or enabled setting does not prove
the host trusted and executed the hook; silence does not prove a clean scan.

Follow `context`, the active playbook, and hook directives for detector ownership. In the
current installation, automatic hooks cover ordinary edits; without an active detector,
`MANUAL_DETECTOR_REQUIRED` requests one completion scan. A selected critique playbook can
require its own detector assessment. Do not append redundant source and URL scans to every
workflow. Use the bundled launcher and installed detector reference for supported syntax;
do not fetch a different CLI version with `bunx` during routine work.

When a rendered detector check is required, confirm it reached the actual surface. For a
shared component use its gallery route. For an authenticated route, use supported session
access and verify the intended screen rather than a login page. If unavailable, report that
check as incomplete and use the authenticated screenshot loop where possible. Source scans
cannot substitute for browser inspection.

Read the findings and advisories. Exit `0` means no primary findings, `2` means primary
findings, and `1` means a scan failed, including a partial scan. Fix relevant faults or record
a narrow, reasoned exception for an intentional brand choice. Keep unrelated detector rules
enabled. Never blanket-ignore findings just to obtain a clean result.

Review the screenshots against `quzzar-design`'s rubric as well. Look for generic composition,
irrelevant decoration, repetitive cards, weak hierarchy, filler copy, and visual choices that
could belong to any product. Detection finds known patterns; it cannot certify originality,
usability, or accessibility. Trigger real states, use keyboard focus, check both themes when
supported, and re-capture after the final changes.

## Evidence that the tools were used

Keep a compact execution record in the existing surface brief, feature record, or review
artifact: target/workspace; actual Mobbin flow/report sources when research is needed;
Impeccable playbooks read and helper commands executed; concrete findings and resulting
changes; rendered evidence; detector coverage; unresolved findings and access limitations.
Reuse upstream-generated artifacts and link them rather than duplicating their contents.
Do not claim a playbook ran because its name appears in a plan, a scan passed because a hook
was configured, or a flow was inspected because a search returned its title.

A visual task is complete when its scoped workflow has run, changed surfaces and journeys
have been inspected rendered, and findings are resolved or explicitly accounted for. Report
the material result and verification limits concisely. For review-only tasks, deliver the
requested assessment and preserve its no-edit scope.

Maintained sources: the installed playbooks, [workflow guide](https://impeccable.style/docs/),
[design-system guide](https://impeccable.style/docs/design-system/), and
[automatic checks](https://impeccable.style/docs/hooks/).
