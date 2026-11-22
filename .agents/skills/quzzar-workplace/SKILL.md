---
name: quzzar-workplace
description: Quzzar's core engineering guidelines and canonical tool stack, covering language selection with TypeScript, Python, and Rust, dead-code deletion, consolidation, when to ask for alignment, types and validation, the standard API response shape, the top-level docs/ knowledge tree, plus naming, file organization, error handling, comments, and formatting. Read before starting any work in a quzzar repo.
---

# quzzar-workplace

The rules that hold everywhere, and the tools we build with. Read this before starting any
work; then read the skill for the layer you're touching.

- **Frontend things** (UI, components, styling, client state, forms): read `quzzar-frontend`.
- **Backend things** (APIs, server code, database, orchestration, edge functions): read
  `quzzar-backend`.

A change spanning both layers needs both. For any new UI, also read `quzzar-ui-ux-design`
for image-first design, interaction coverage, faithful implementation, and consolidation review.

## First: read the repo's own context

Read the applicable repository instructions, starting with `AGENTS.md`, and inspect the
project configuration for its real stack and commands. Honor other instruction files used
by the active agent. Repo facts beat general preferences in this skill. No generated
marker block or bootstrap skill is required.

---

# Core guidelines: ALWAYS follow these

These five are non-negotiable. Everything below them is detail.

## 1. NEVER keep old, unused code

Git and GitHub exist for this. If something is no longer used or needed, **delete it**. Don't
comment it out, don't rename it to `*.old`, don't leave it behind a dead flag, don't keep a
`legacy/` copy "just in case."

- Old code is recoverable from commit history. That is what history is for.
- Delete superseded code when its replacement is live and verified. For live-data changes,
  expand additively, then migrate and remove old structures in a tracked cleanup phase once
  deployed consumers no longer need them. Temporary compatibility has an explicit end.
- Dead code found while working on something else is a finding: say so, and delete it as part
  of the release that supersedes it.

When you delete, delete completely: the code, its tests, its types, its exports, its config
entries, and any now-orphaned dependency.

## 2. CONSOLIDATE repetition

Avoid repeating or duplicating an existing responsibility. Before adding another implementation,
investigate where the behavior already lives and consolidate the shared responsibility into one
maintained implementation. Reuse or deepen existing modules, keep feature-specific behavior at
its boundary, and delete superseded copies.

Make routine consolidation decisions autonomously. Ask only when the decision would materially
change the requested outcome or depends on information the code and session do not supply.
Investigation is expected; a subagent and user approval are not prerequisites for routine reuse.
For UI consolidation that changes an approved appearance, follow `quzzar-ui-ux-design` and
present the rendered tradeoff for user approval. Exact-output consolidation remains routine.

## 3. Ask with the available question tool

When you need alignment or have a real question, use the active agent's question tool when
available. Give a few distinct options to pick from. If the host has no question tool,
ask a concise question in chat.

This is the preferred way to resolve ambiguity. Reach for it early rather than guessing and
building the wrong thing.

## 4. Strict types, validated schemas, clean documented code

- **Full strict TypeScript**, whenever writing TypeScript. Every strict flag on, explicit
  types, and Zod schemas parsed at the boundary rather than trusted casts. Keep `as` casts
  rare; fix the upstream types, or explain why a cast is unavoidable.
- **Rust uses its native type system.** Model states and invariants with structs, enums,
  ownership, and `Result`. Use safe Rust by default and validate external input at runtime.
- **Python uses type annotations and runtime boundary validation.**
- Use each language's documentation conventions: **JSDoc for TypeScript and JavaScript**,
  rustdoc for Rust, and docstrings for Python. Document APIs, contracts, and non-obvious
  behavior; keep the code clean and readable.

## 5. Standard API response shape

Every API response follows this:

```ts
type ApiResponse<T, F = Record<string, string>> =
  | { status: "success"; data: T }
  | { status: "fail"; data: F }                    // client's fault: validation, business rules
  | { status: "error"; message: string; code?: string; data?: unknown }; // server's fault
```

The distinction matters and isn't optional:

- `fail`: **the client's fault.** Validation errors, business-rule rejections. `data` carries
  the per-field reasons.
- `error`: **the server's fault.** Something broke on our side. `message` is safe to surface;
  never leak internals through it.

---

# The stack

These are the preferred tools. Reuse project choices and choose libraries for the actual
requirements; this is a recommended stack rather than a closed dependency allowlist.

## Language selection

**Rust with Cargo is the default for application logic, domain engines, backends, services,
and workers.** Favor one reusable Rust implementation instead of duplicating behavior across
platforms. Read the bundled `rust-skills` index and its task-relevant rules before writing,
reviewing, or refactoring Rust.

| Language | When to choose it | Toolchain |
| --- | --- | --- |
| **Rust** | Default for application logic, services, processing, and native integration. | **Cargo** for dependencies, builds, tests, and execution. |
| **TypeScript** | React UI, browser/host adapters, and integrations that need JavaScript. | **Bun** for runtime, packages, and scripts; Deno only where required by the host. |
| **Python** | Work that needs Python's data, ML, scientific, or automation ecosystem. | **uv** for dependencies, environments, and execution. |

Use TypeScript or Python when the runtime or ecosystem makes it appropriate; use the toolchain
above. Do not rewrite an existing system merely to change its language unless the task calls
for it. Rust is the preferred starting point, not evidence that a particular implementation
is fast: measure performance claims and test behavior as well as compiling.

## One application source for web, desktop, and mobile

**Use Rust + Tauri with a shared React frontend, normally built with Vite.** The same
application source should build as a website and as desktop/mobile applications. Keep browser
support a small addition to the shared app, not a separate product implementation.

Share UI components, state, contracts, and reusable domain logic. Isolate native/browser
capabilities behind a small typed host adapter; do not fork screens, flows, or business rules
by platform. Tauri hosts the native builds. The browser loads the same frontend with an
explicit web transport to the Rust service, or a shared Rust/WASM core where suitable. Browser
support does not imply that Tauri IPC is available in a website.

Keep reusable Rust logic independent of Tauri-specific commands, with thin host bindings.
Validate the supported targets independently: a shared source tree or successful desktop
build is not proof that the web, iOS, Android, Windows, Linux, and macOS builds all work.
Do not introduce a new hosted service solely to satisfy this pattern when the app does not
need one. Respect the existing application boundary and requested platforms.

## Frontend: the base of every web app

| Tool | Role |
| --- | --- |
| **React + Vite + TypeScript** | Shared UI and frontend build for browser and Tauri targets. |
| **Tauri + Rust + Cargo** | Native desktop/mobile host and reusable Rust application logic. |
| **React Router** (`react-router-dom`) | Client-side routing, host-aware: marketing vs. portal. |
| **vanilla-extract** | Styling, both layers. Zero-runtime CSS-in-TypeScript. Styles compile to static CSS at build time, so nothing ships a style engine to the browser. Type-safe class names, variables, and theme contracts, via the official Vite integration. |

## Frontend · app

| Tool | Role |
| --- | --- |
| **Jotai** | Global/client state (atomic, e.g. the theme atom). Also the storage layer via `atomWithStorage`. |
| **React Query** (`@tanstack/react-query`) | Server state + data fetching. Lives in the app's data layer. |
| **nuqs** | URL / query-string state: filters, tabs, pagination reflected in the URL. |
| **TanStack Table** | Headless tables: sorting, pagination, selection, column logic. |
| **React Hook Form** (`@hookform/resolvers`) | Forms. The resolver bridges RHF to the Zod schema. |
| **three** (`three` + `@types/three`) | 3D rendering, lazy-chunked behind the panel that hosts it. Narrow scope: not a general graphics tool, and **never inside the shared UI package**. |

## Frontend · ui

| Tool | Role |
| --- | --- |
| **Radix** (`radix-ui`, unified pkg) | Accessible primitives under the shared UI package's interactive components. |
| **Motion** (`motion/react`) | Default animation: enter/exit, layout transitions, gestures, cursor-reactive grid + parallax. |
| **GSAP** | Narrow: SVG plugins (DrawSVG, MorphSVG, MotionPath) and heavy scroll-scrubbed timelines (ScrollTrigger). |
| **Vaul** | Mobile bottom-sheet drawers (responsive dialogs). |
| **Sonner** | Toasts / transient notifications. |
| **cmdk** | Command palette (⌘K). Wrap in a Radix Dialog. |
| **Number Flow** | Animated number transitions (metrics, counters). |
| **Headless Tree** (`@headless-tree/core` + `/react`) | Folder/document trees: drag-move, inline rename, multi-select, keyboard nav. The `react-complex-tree` successor. |
| **Lucide** (`lucide-react`) | Icon set. |

## Everywhere

| Tool | Role |
| --- | --- |
| **Zod** | Cross-boundary schemas in TypeScript: wire shapes, intake, agent contracts, and form validation via the RHF resolver. Lives in a shared schema package. For other languages, follow the contract guidance in `quzzar-backend`. |
| **Playwright** | End-to-end testing against a running web app, alongside language-native tests for backend logic. |
| **dayjs** | Dates and time in TypeScript. |
| **lodash** (`lodash-es`) | General TypeScript utilities: deep clone/merge, `groupBy`, `keyBy`, `uniqBy`… |

## Backend

| Tool | Role |
| --- | --- |
| **Bun** | Runtime + package manager for TypeScript service packages and JavaScript workspace tooling. **Never npm.** |
| **uv** | Python dependencies, environments, and execution. |
| **Cargo** | Rust dependencies, builds, tests, and execution. |
| **Temporal** | Optional when long-running workflows need durable orchestration. Adopt only for a concrete requirement. |
| **Deno** | The edge-function runtime (Supabase functions). **Not bun.** |
| **Supabase** | Preferred managed Postgres, Auth, and Storage; RLS protects user reads and writes. |
| **GitHub Actions** | Hosts the runner today (free tier, ~2000 min/mo). An always-on VM is the upgrade path. |
| **Langfuse** | Instrument every application AI/model call across languages and hosts; include quality scores where applicable. |

---

# Process

## Environments and promotion

Three branches, promoted in order:

```
dev  →  staging  →  main
```

That's the whole release process. There's no separate versioning ceremony. A change ships by
being promoted along that path.

This is what makes core guideline 1 safe: deleting an unused system isn't risky, because the
deletion travels the same path as any other change and gets exercised at each stop before it
reaches `main`.

---

# Project docs

**A top-level `docs/` directory holds the project's knowledge, one markdown file per topic.**
It's the repo's brain: what a new engineer, or a fresh agent session, would otherwise have to
ask a person for.

## What belongs there

The test is whether the answer is recoverable from the code. If it can be found by reading the
source, it doesn't need a doc. If it lives only in someone's head, a Slack thread, or a
vendor's dashboard, it belongs in `docs/`.

In practice that's things like how a pipeline runs end to end, why an integration is shaped the
way it is, what the environments are and who can reach them, the business rules behind a
feature, and what to do when a given system fails in production.

There's no fixed topic list, and don't scaffold one. Repos differ, and a canonical set would
sit half-empty in most of them. Add a file when there's something to write.

## Shape

- One topic per file, `kebab-case.md`, flat at the top of `docs/`.
- A topic that outgrows one file gets a subdirectory. Same rule as features inside a layer:
  grouping happens when the volume earns it, not in anticipation.
- `docs/README.md` is the index. One line per topic, saying what it covers. That's how an
  agent finds the right file without opening all of them.

Two paths under `docs/` are reserved, and are not topic files:

| Path | Holds |
| --- | --- |
| `docs/adr/` | Architecture decision records, numbered |
| `docs/agents/` | Agent config: issue tracker, domain layout, triage labels |

`CONTEXT.md` at the repo root is the domain glossary. Terminology goes there, not into a
`docs/` topic file. Maintain these records directly; no external skill package is required.

## Read it, and keep it current

**Before working in an area, read the `docs/` topic covering it.** Start at `docs/README.md`.
Repository instructions carry the session-level context; `docs/` carries the depth you
pull in when the work touches it.

**When a change moves what a doc describes, update that doc in the same change.** Not a
follow-up, not a ticket. The doc ships with the code or it's already wrong.

A stale doc is worse than a missing one, because it gets believed. When a topic stops existing,
delete its file. Core guideline 1 covers documentation too.

---

# Repo-wide code rules

Types, schemas, documentation, and the API response shape are covered in
[Core guidelines](#core-guidelines-always-follow-these), and are not repeated here.

## Naming

The naming rules in this section cover TypeScript. Rust and Python use their native naming
and module conventions, including `snake_case` identifiers and module filenames. Keep directory
naming consistent with that language's tooling and the repo.

| Thing | Casing | Example |
| --- | --- | --- |
| Component file | `PascalCase.tsx` | `SettingsPanel.tsx`, `UserAvatar.tsx` |
| Non-component module | lowercase, one word or `kebab-case` | `format.ts`, `theme.ts`, `asset-layer.ts`, `user-manager.ts` |
| Directory | lowercase, one word or `kebab-case` | `atoms`, `routes`, `site-view`, `vector-db` |
| Type / interface | `PascalCase` | `LoadState`, `FieldSpec` |
| Value / function | `camelCase` | `resolveSite`, `queryClient` |
| True constant | `SCREAMING_SNAKE_CASE` | `HEADER_H`, `PROJECT_SECTIONS`, `SUPABASE_URL` |
| Jotai atom | `camelCase` + `Atom` suffix | `themeAtom`, `selectionAtom` |
| Boolean | `is` / `has` / `can` / `should` prefix | `isLoading`, `hasOutput`, `canDrag`, `shouldCreateUser` |

Prefer `type` over `interface`. That's the dominant choice in practice, and `type` covers
unions, which the result types in [Error handling](#error-handling) need.

Spell words out. `resolveSite`, not `resSite`. The exceptions are terms already standard in the
domain or the platform (`url`, `id`, `db`, `otp`, `ui`).

**camelCase filenames are wrong, including where you find them.** You will find them: a
`resolveSite.ts` sitting next to an `asset-layer.ts` in the same directory. That's drift, not a
second convention. Write `resolve-site.ts`.

This is the one place where "match the surrounding code" does **not** apply. A wrong neighbour
is not a precedent. Matching it is how the drift spread in the first place. Directories are
`kebab-case` for the same reason; `snake_case` directories in older code are drift too.

## File organization

**In TypeScript apps, top level of `src/` is by layer.** One lowercase directory per concern:

```
src/
  atoms/       client state
  auth/        session and route guards
  components/  shared UI
  data/        server state: queries and mutations
  lib/         configured clients and adapters
  routes/      pages
  utils/       pure helpers
```

**Feature grouping happens inside a layer, not above it.** A feature that grows past a couple of
files gets a subdirectory in the layer it belongs to: `components/site-view/`, not a top-level
`site-view/` holding its own atoms, queries, and components.

Shared UI packages organize by component instead: one lowercase directory per component
(`button/`, `accordion/`, `input-cards/`), with named exports barrelled through `src/index.ts`.

Keep shared packages focused on proven responsibilities rather than speculative future use.
For an approved UI/UX build, assemble the scoped component families and states required by
the approved image set before composing screens, following `quzzar-ui-ux-design`.

## Em dashes

**Banned.** Not in prose, not in documentation, not in commit messages, not in code comments.

Rewrite the sentence instead. A colon, a comma, parentheses, or a full stop covers everything
an em dash was doing, and usually reads better:

| Instead of | Write |
| --- | --- |
| `the worker claims it — then the workflow starts` | `the worker claims it, then the workflow starts` |
| `one thing matters — the seam` | `one thing matters: the seam` |
| `it works — mostly — on retry` | `it mostly works on retry` |
| `deleting isn't breaking — history has it` | `deleting isn't breaking. History has it.` |

The one exception is **UI typography**, where an em dash may separate a label from its
description in a list row or a title line. That's a visual separator, not prose.

**Careful in YAML frontmatter.** Don't swap an em dash for a colon there. An unquoted YAML
scalar containing `: ` parses as a nested mapping, so `description: Conventions: the stack`
is a parse error and the whole file gets skipped. Reword instead, or quote the value.

## Comments

Use the language's documentation conventions from core guideline 4.

## Error handling

**Return a result type.** A function that can fail says so in its return type, so the caller
has to deal with it. It's the same discriminated-union shape as `ApiResponse` in core
guideline 5, scaled down to whatever the function needs.

`throw` is a last resort, not the default. It's invisible in the signature and turns every
caller into a maybe-handler. Reach for it only where there's genuinely nothing to return to.

**Log.** Logging is wanted, not tolerated. A failure that's handled quietly still gets
recorded. Swallowing an error without a log is the thing to avoid.

## Formatting and lint

For TypeScript, **ESLint** for linting and **Prettier** for formatting. Their output is the
answer. Don't hand-format around them, and don't argue with a rule in review when it can be
configured instead.

Run `eslint-config-prettier` so ESLint stops enforcing the stylistic rules Prettier owns.
Without it the two fight over the same lines and every save flips the file back and forth.

For Rust, use **rustfmt** through `cargo fmt` and **Clippy** through `cargo clippy`. For Python,
run the repo's configured lint and format tools through uv.

---

# Rules

- The five core guidelines are not negotiable and not context-dependent. Apply them without
  being asked.
- Repository instructions and actual project configuration override general preferences here.
  This skill holds durable preferences that travel between repos.
- Read the `docs/` topic before working in its area, and update it in the same change. A stale
  doc gets believed.
- Where this file is silent, the surrounding code is the convention. Match it and say so.
- Where this file is **not** silent, it beats the surrounding code. Existing code that breaks a
  rule recorded here is drift to be corrected, not a local convention to be matched.
