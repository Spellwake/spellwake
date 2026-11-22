---
name: quzzar-frontend
description: Quzzar's frontend conventions covering UI architecture, tokens, state, forms, accessibility, and the component gallery, with reference research and rendered verification for visual work. Use before writing or changing any UI code, React components, styles, or client-side state.
---

# quzzar-frontend

Frontend conventions. Repo-wide rules (naming, file organization, error handling, formatting)
live in `quzzar-workplace`. Read that too.

This file covers UI implementation. For every new UI, including new screens, components,
flows, or redesigned surfaces, read `quzzar-ui-ux-design` and follow its image-first approval,
complete interaction coverage, faithful implementation, and consolidation review process.
Purely nonvisual maintenance does not require a new design round. Follow the user's brief and
existing product constraints; do not add unapproved UI copy or controls.

## Shared application and visual verification

Use the same React application source for browser and Tauri desktop/mobile builds, normally
with Vite. Follow `quzzar-workplace`'s shared-source architecture. Keep platform access in a
small typed host adapter; keep UI, flows, and reusable state shared. Browser support should
be a small addition, not a separate frontend implementation. Put reusable domain logic in
Rust rather than duplicating it in UI code.

For a visual change, render the actual surface, inspect it at relevant viewport sizes and
states, and fix observed issues before reporting completion. Verify native integration in
the relevant native host; a browser screenshot cannot establish native capability behavior.
When asked for alternatives, deliver the requested count and render each. Preserve the
existing UI direction and keep copy terse.

## When to use

- Writing or changing a component, page, layout, or route.
- Touching styles, design tokens, or theme.
- Adding client state, a data fetch, or a form.
- Writing UI copy.

## First: read the repo's own context

Read the applicable repository instructions, starting with `AGENTS.md`, and inspect the
project configuration for its framework, styling approach, and commands. Honor other
instruction files used by the active agent. Repo facts beat general preferences below.

## Recommended tools

Use `quzzar-workplace`'s stack as the starting point: React + Vite + TypeScript for shared UI,
Rust + Tauri for native hosts and application logic, and the listed UI/state libraries for
their intended roles. Motion remains the preferred general UI animation library.

The list is recommended, not closed. Reuse installed tools when they fit. Choose additions
for a concrete unmet need, maintenance cost, accessibility, and supported targets; explain
material tradeoffs. Routine library selection does not need user approval unless it changes
the agreed product or architectural direction.

---

## The layer boundary

The one rule that shapes everything else: **presentation libraries may live inside the shared UI
package; data, state, and routing libraries stay app-level and never become its dependencies.**

Coupling a design system to the data layer is the thing to avoid. A marketing site or a
component gallery must be able to consume the UI package without dragging in a query client, a
router, or database types.

So: Radix, Motion, and the icon set can be dependencies of the shared package. React Query,
Jotai, React Router, nuqs, and anything 3D cannot.

## Component structure

- One component per file, `PascalCase.tsx` (see `quzzar-workplace` for naming).
- **Named exports**, barrelled through the package's `src/index.ts`.
- Each interactive component wraps a primitive from the accessible-primitives library and is
  skinned with token CSS. That's what keeps behaviour correct and appearance ours.
- Share proven responsibilities and avoid speculative global APIs. During an approved UI/UX
  design build, put the required component families and states in the scoped library before
  assembling screens, as described by `quzzar-ui-ux-design`.

## Don't re-implement what the stack already owns

Before adding a hook or a utility, check whether a stack library already covers it. These have
a settled home:

| Need | Use |
| --- | --- |
| local / session storage | Jotai `atomWithStorage` |
| data fetching, server state | React Query |
| pagination, row selection, sorting | TanStack Table |
| input state, masks, validation | React Hook Form + Zod |
| URL / query-string state | nuqs |
| focus trap, click-outside, roving tabindex | Radix primitives |
| slider drag, radial move | Radix Slider or Motion |

A shared hooks library is for what's left over: genuinely generic utilities. Keep its rationale
at the top of its barrel file so nobody re-implements something the stack already provides.

Prefer vendoring a small utility (with attribution) over taking a dependency, when it keeps the
shared package light.

## Styling

vanilla-extract: zero-runtime CSS-in-TypeScript, compiled to static CSS at build time via the
Vite integration. Class names, variables, and theme contracts are type-checked.

This is core guideline 4 applied to CSS: the compiler catches a bad token reference instead of it
silently rendering nothing.

## Design tokens

**Tokens are the source of truth.** A raw hex or px value in a component is a bug. It's a value
that can't be themed and won't move when the system does.

Motion has tokens too (duration, easing). Use them rather than hardcoding timings, so animation
stays consistent across components and respects a single point of change.

## Animation

- **Motion is the default**: component enter/exit, layout animations, gestures, parallax.
- **GSAP is narrow**: SVG plugins (DrawSVG, MorphSVG, MotionPath) and heavy scroll-scrubbed
  timelines. Not for ordinary component animation.
- **Overlays are the exception.** Let the primitive library's `data-state` CSS drive overlay
  enter/exit on the motion tokens. Motion exit through a portal is unreliable. Motion is for
  layout and gesture polish, not overlay teardown.
- Respect reduced-motion.

## Accessibility

Accessibility comes from using the primitives, not from patching markup afterwards. Radix
carries focus management, keyboard interaction, and ARIA wiring for the interactive components
that wrap it.

If you find yourself hand-writing ARIA attributes, that's usually the signal you should have
reached for a primitive instead.

## State

Three stores, three jobs. Pick by where the state belongs:

- **Jotai**: global/client state, atomically. Also the storage layer, via `atomWithStorage`.
- **nuqs**: URL / query-string state. Filters, tabs, and pagination belong in the URL.
- **React Query**: server state and data fetching, in the app's data layer.

## Data fetching

React Query, in the app's data layer. Responses are **parsed through their Zod schema on fetch**
(core guideline 4). Never cast, never trusted. Schemas come from the shared schema package.

Expect the `ApiResponse` shape from `quzzar-workplace` core guideline 5, and handle `fail`
(client's fault, per-field) separately from `error` (server's fault).

## Forms

React Hook Form, with the resolver bridging to the Zod schema. **One schema, validated on both
ends**. The form and the server share it, from the shared schema package.

## UI copy: terse, never verbose

A standing rule, and it holds everywhere:

- A surface gets a short title and **at most one** short supporting line. Aim well under ten
  words.
- Consequences and caveats live in the confirm dialog that gates the action, never as prose
  above the controls. **Don't narrate; gate.**
- If copy needs multiple sentences to justify an element, the element is wrong.

Empty states: a quiet contained box, a title, one short line, one action. No hero graphics, no
multi-sentence pitches, no feature tours. Reach for a set piece only when asked for a moment,
not by default.

## The component gallery

A standalone internal app renders every shared component with mock props. It's the fidelity
check, and it stands in for Storybook.

**When you add or change a shared component, add or update its gallery page.** Cover the real
states the way a story would: variants, sizes, hover/focus/disabled/loading, empty and error.
The gallery page is how the component gets verified; treat it as the story.

It's also the screenshot target. Because it renders every state on one page with no auth and no
seed data, it's the cheapest surface to point a browser at when verifying visual work. Use the gallery to inspect the rendered result.

## Testing

Playwright, end-to-end against a running app, per `quzzar-workplace`.

---

## Rules

- Treat the stack as recommendations; reuse suitable existing tools and explain material additions.
- Data, state, and routing libraries never become dependencies of the shared UI package.
- Fetched data is parsed through its Zod schema. No casting into shape.
- Tokens, not raw values.
- Short copy. Don't narrate; gate.
- Never report a visual change as done without rendering it and looking at it.
- Repository instructions and actual project configuration override general preferences here.
- Deleting an unused component means deleting its styles, gallery page, tests, and exports too
  (core guideline 1).
