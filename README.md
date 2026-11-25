# Spellwake

Work in progress: a browser and Tauri tabletop application. The current preview uses sample game state; it is not a connected multiplayer match.

Use Bun 1.3.11 and the Rust toolchain pinned in `rust-toolchain.toml`. Run `bun install --frozen-lockfile`, `bun run play:check`, `bun run play:test`, and `cargo test --workspace` for source checks. Runtime artwork is supplied separately and is not included in this repository.

## Health & CI Badges

[![Spellwake Multi-OS Release](https://github.com/Spellwake/spellwake/actions/workflows/tauri.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/tauri.yml)
[![Rust CI](https://github.com/Spellwake/spellwake/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/rust.yml)
[![Application API CI](https://github.com/Spellwake/spellwake/actions/workflows/application-api.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/application-api.yml)
[![Domain Health](https://github.com/Spellwake/spellwake/actions/workflows/domain-health.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/domain-health.yml)
[![Catalog Refresh](https://github.com/Spellwake/spellwake/actions/workflows/catalog-refresh.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/catalog-refresh.yml)

Per-domain health badges are generated from real Rust, isolated Postgres 17, and frontend contract checks by [`scripts/health_checks.py`](scripts/health_checks.py) and published to the `badges` branch by [`.github/workflows/domain-health.yml`](.github/workflows/domain-health.yml). Checks that execute zero tests, lack an end-to-end verification surface, or require hosted credentials remain visibly `unknown`.

| Domain | Local CI | Live / Hosted | Coverage / Status |
| --- | --- | --- | --- |
| **Patreon** | ![Patreon local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-patreon.svg) | ![Patreon live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-patreon-live.svg) | Rust unit, Postgres RLS/integrity, web contracts. Live needs OAuth/webhook secrets. |
| **Stripe** | ![Stripe local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-stripe.svg) | ![Stripe live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-stripe-live.svg) | SDK webhook vectors, unit, Postgres settlement, checkout contracts. Live needs Stripe keys. |
| **AI** | ![AI local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-ai.svg) | ![AI live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-ai-live.svg) | Offline runtime, Postgres budget/slot integrity, quota contracts. Live needs LLM/Langfuse keys. |
| **Matchmaking** | ![Matchmaking local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-matchmaking.svg) | ![Matchmaking live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-matchmaking-live.svg) | Queue intents, preview-format isolation, lobby/room Postgres, web journal. Live needs hosted API. |
| **Predictions** | ![Predictions local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-predictions.svg) | ![Predictions live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-predictions-live.svg) | Price source, unit, Postgres pool/settlement, prediction contracts. Live needs price bucket. |
| **Market** | ![Market local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-market.svg) | ![Market live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-market-live.svg) | Pack/shop/exchange unit, Postgres wallet/trade/market, web contracts. Live needs hosted API. |
| **Pet cosmetics** | ![Pet cosmetics local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-pet-cosmetics.svg) | ![Pet cosmetics live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-pet-cosmetics-live.svg) | Unqualified: DB column exists, but equip slot, tests, and approved assets are absent. |

## Honest Card Support & Preview Formats

![Card Support Stage 1 AST](https://raw.githubusercontent.com/Spellwake/spellwake/badges/card-support-ast.svg)
![Card Support Stage 2 Engine Bound](https://raw.githubusercontent.com/Spellwake/spellwake/badges/card-support-engine.svg)
![Card Support Stage 3 Qualified](https://raw.githubusercontent.com/Spellwake/spellwake/badges/card-support-qualified.svg)
![Preview Formats Isolated](https://raw.githubusercontent.com/Spellwake/spellwake/badges/preview-formats.svg)

Incremental and scheduled catalog refreshes ([`scripts/catalog_refresh.py`](scripts/catalog_refresh.py), [`.github/workflows/catalog-refresh.yml`](.github/workflows/catalog-refresh.yml)) separate support tiers instead of collapsing parsing into gameplay qualification, and keep Scryfall's `future` (Future Standard) legality strictly isolated from ordinary formats (`commander`, `standard`, `pioneer`, `modern`, `legacy`, `vintage`):

- **Stage 1 (Strict AST parsed, `spellwake-oracle-compiler`)**: `32,095 / 32,155` (`99.81%`) required ordinary base-format cards on the `2026-10-29` Scryfall Oracle snapshot (`60` failed).
- **Stage 2 (Validated engine bound, `spellwake-card-programs`)**: `23,573 / 32,155` (`73.31%`) lowered into native engine IR and validated (`8,582` blocked).
- **Stage 3 (Card-specific gameplay qualified, `scripts/card-game-qualifications.json`)**: `135` declared complete (`6` partial, `0` stale against the snapshot); marked `unverified` until an actual `--game-log` confirms the named gameplay tests.
- **Future Standard (`future`) vs unreleased (`preview-only`)**: `future` is Scryfall's full upcoming Standard pool (`5,134` cards on the `2026-10-29` snapshot: mostly current Standard cards that will stay legal, plus any upcoming set cards). Unreleased (`preview-only`) cards are newly spoiled cards that are legal in `future` but not yet legal in any ordinary format (`0` on the `2026-10-29` snapshot; rises during spoiler season). Both are tracked separately from ordinary legality and blocked from competitive queues; invented overlays (`futuremodern`, `futurelegacy`) are rejected.

## Hosted Credentials Required for Live Checks & Publication

Local CI never spends money, activates live providers, or publishes private assets. Live provider probes and hosted catalog publication require hosted environment credentials:

- **Patreon**: `SPELLWAKE_PATREON_CLIENT_ID`, `SPELLWAKE_PATREON_CLIENT_SECRET`, `SPELLWAKE_PATREON_REDIRECT_URI`, `SPELLWAKE_PATREON_WEBHOOK_SECRET`, `SPELLWAKE_PATREON_EVIDENCE_BUCKET`
- **Stripe**: `SPELLWAKE_STRIPE_SECRET_KEY`, `SPELLWAKE_STRIPE_WEBHOOK_SECRET`, `SPELLWAKE_STRIPE_API_VERSION`, `SPELLWAKE_STRIPE_EVIDENCE_BUCKET`
- **AI**: `SPELLWAKE_AI_PROVIDER`, provider API key (`OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GEMINI_API_KEY`), `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, `LANGFUSE_BASE_URL`
- **Matchmaking, Market, Predictions & Catalog Publication**: `SPELLWAKE_SUPABASE_URL`, `SPELLWAKE_SUPABASE_PUBLISHABLE_KEY`, `SPELLWAKE_SUPABASE_SECRET_KEY`, `SPELLWAKE_APPLICATION_API_URL`, `SPELLWAKE_ROOM_INVITATION_KEY`, `SPELLWAKE_RECONCILE_SECRET`, `SPELLWAKE_PRICE_BUCKET`, `SPELLWAKE_CATALOGUE_BUCKET`

No software license is granted by publishing this source. Magic: The Gathering and its card imagery belong to Wizards of the Coast and their respective artists. This is unofficial fan content, not approved or endorsed by Wizards. Third-party dependencies retain their own licenses.
