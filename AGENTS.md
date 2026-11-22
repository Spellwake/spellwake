# Spellwake

## Repository routing

Before committing code, exporting artwork, changing product planning, or deploying, read the private working copy of
`docs/github-and-deployment.md` when available for repository ownership and the verified deployment scope.

- `Spellwake/spellwake`: application source, tests and build configuration.
- `Spellwake/spellwake-assets` (private): artwork, cosmetic sources, exports and provenance.
- `Spellwake/spellwake-ops` (private): all project documentation, internal product/business planning and operating policy. Keep local `docs/` and `PRODUCT.md` as ignored working copies; export updates manually to ops, never force-add them to the code repo.

The shared checkout still contains local asset and planning work. Preserve those working paths;
commit each deliverable only to its owning repository. Confirm the Git remote before staging,
stage explicit paths, and verify the pushed commit. Asset/ops exports are manual, not automatic.
Read asset approval records and handoff notes before selecting production assets; backup presence
does not mean approval. Keep new and in-progress work distinct from accepted exports.

The code repository has a clean public history. Assets, operations and the pre-cutover
repository archive remain private. Never push old-history branches into this public repository.
No software license is implied by public visibility. Render deploys only the code repo's checked
`main` snapshot; private artwork and operations changes do not deploy themselves.

## Temporary storage and cleanup

Cleanup is part of completing every task, experiment, review and milestone. Use temporary
storage as needed, then remove owned, obsolete build output and redundant temporary copies
when their consumers have finished. Available disk space is not a reason to retain every
candidate's or reviewer's full build tree indefinitely.

- Reuse compatible build output where safe. Keep separate targets when isolation requires
  them, and retire those caches after use rather than accumulating full copies per attempt.
- Before deleting, identify the exact generated paths and verify that no active process,
  Cargo build lock, other agent or retained evidence depends on them. Coordinate shared
  targets; skip anything whose ownership or use is uncertain. Limit cleanup to verified
  disposable output, such as obsolete Cargo dependency, incremental and build-script caches.
- Preserve authored and uncommitted source, unique candidate source snapshots, required
  inputs, databases and Docker volumes, accepted assets, and evidence needed to substantiate
  or reproduce results. This includes source/input manifests and hashes, required test and
  review results, and diagnostic logs needed to explain failures or qualification limits.
  Ignored or untracked files may be the only copy of important work.
- Keep a concise authoritative evidence set for each retained result. Remove duplicate
  working/runtime copies only after verifying their contents and ensuring retained records
  still resolve to the required source and evidence. Generated caches are not evidence merely
  because they sit inside an experiment directory; preserve any specific binary needed for
  an irreproducible result before retiring its cache.
- At closeout, report what was cleaned and any substantial temporary storage retained, with
  its purpose and cleanup condition. Completion requires cleanup or a documented reason it
  cannot yet be done safely. Leave active work intact.

<!-- quzzar-skills:start -->
## Agent skills

Read `.agents/skills/quzzar-workplace/SKILL.md` before work in this repo.
Before touching a layer, read its local `quzzar-frontend` or `quzzar-backend` skill;
read both for changes spanning the layers.

For new or redesigned UI, read `quzzar-ui-ux-design`: approve the image direction and
screen/state walkthrough, implement faithfully, verify rendered results with independent
review, and present appearance-changing consolidation for user approval.
For system architecture, read `quzzar-system-design`. For Rust, follow the Rust workflow below.
For animation, read `motion-art-direction`, `animation-principles`, and `shot-composition`,
then select additional installed motion skills for the task.

Resolve these skills from `.agents/skills/` so this repo uses its installed revision.

### Stack

- Backend: Rust Cargo workspace. See `Cargo.toml` for members and `rust-toolchain.toml` for the toolchain.
- Checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Before engine or service work, read `docs/backend-foundation.md` for scope, commands and qualification limits.
<!-- quzzar-skills:end -->

For component-library work, read `.agents/skills/modern-web-guidance/SKILL.md` and use
the project-pinned Chrome guidance CLI. Run `bun run controls:guidance search "<one UI capability>"`
then `bun run controls:guidance retrieve <guide-id>`. Check the returned Baseline and fallback
notes against the target and verify the relevant interaction in the gallery. The skill and
query index are supplied by the locked development dependency after `bun install --frozen-lockfile`.

Skills are installed in `.agents/skills/`. For system architecture work, read
`quzzar-system-design` before proposing or implementing the design.

Built-in image generation is available in Codex for future design work.

## Rust workflow

Before writing, reviewing, or refactoring Rust, read
`.agents/skills/rust-skills/SKILL.md`, then open the rule files relevant to the task.
Use its rule IDs when explaining findings. Project conventions take precedence over
generic recommendations: preserve typed domain errors, deterministic ordering,
atomic rejected transitions, and explicit qualification boundaries. Profile before
changing snapshot ownership or adding optimization dependencies.

Use the edition declared by each manifest and the toolchain pinned in
`rust-toolchain.toml`. The pinned compiler is not a declared minimum supported Rust
version; no MSRV is currently promised. Preserve the unsafe-code prohibition.
Return recoverable failures through `Result`; reserve production `expect` for
documented internal invariants. Test assertions may use `unwrap`/`expect`.
Keep underlying errors available through `Error::source()` where an error implements
`std::error::Error`; expose only safe messages at public boundaries. Document public
APIs and their error conditions. Explain scoped Clippy allowances at their source.

After a coherent batch of Rust edits, run `cargo check --locked` for the affected
workspace. Before handing off Rust changes, run formatting, warnings-denied all-target
Clippy, and relevant tests. Root-workspace commands do not cover the independent
`services/application`, `src-tauri`, or `benchmarks/browser-rendering/rust` workspaces;
use `--manifest-path <directory>/Cargo.toml` when touching them. Use each workspace's
lockfile. Root qualification tests require an explicit vocabulary:

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
SPELLWAKE_CREATURE_VOCABULARY_INPUT="$PWD/work/resolution-value-choices/current-s1-packets-v1/creature-vocabulary.input.v1.json" cargo test --locked --workspace
```

If that retained input is unavailable, report the blocked qualification tests and
obtain the source-qualified input described in `docs/backend-foundation.md`.
Synthetic fallback data cannot qualify the engine.

Claude Code also discovers the same Rust skill through `.claude/skills/rust-skills`.
Its Rust edit hook runs a compiler check for the nearest manifest; shell-based edits
and other agents still follow the explicit check workflow above. Compiler and lint
success supplement behavioral and reference tests; they do not certify game rules.
