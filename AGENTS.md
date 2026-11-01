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

<!-- quzzar-skills:start -->
## Agent skills

**Before starting any work in this repo, read the `quzzar-workplace` skill.** It holds the
conventions for issues, branches, commits, PRs, and review, plus the code rules that apply
at every layer: naming, file organization, comments, types, error handling, dependencies.

Then, before writing code, read the skill for the layer you're touching:

- **`quzzar-frontend`**: UI, components, styling, client state, data fetching, forms,
  accessibility, frontend tests.
- **`quzzar-backend`**: API routes, server code, database queries, migrations, validation,
  auth, background jobs, backend tests.

Read the layer skill *before* writing, not as a check afterwards. A change spanning both
layers needs both.

For visual work, also read **`quzzar-design`** and the installed **`impeccable`** skill.
For new or materially changed journeys, require Mobbin flow comparisons and a source-linked
visual report before implementation. Execute the installed Impeccable playbook for the task,
including context loading, review artifacts, and detector directives; naming a command is
not execution. Keep the repo's established components and tokens.
Never report a visual change as done without having looked at it rendered.

For animation work, read `motion-art-direction`, `animation-principles`, and
`shot-composition`; select other installed motion skills to fit the task and existing engine.

### Stack

<!-- filled in by /setup from the repo itself. Correct it by hand if it drifts -->

- Backend: Rust Cargo workspace. See `Cargo.toml` for members and `rust-toolchain.toml` for the toolchain.
- Checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Before engine or service work, read `docs/backend-foundation.md` for scope, commands and qualification limits.
<!-- quzzar-skills:end -->

Skills are installed in `.agents/skills/`. For system architecture work, read
`quzzar-system-design` before proposing or implementing the design.

Built-in image generation is available in Codex for future design work.
