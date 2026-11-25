# Spellwake

Work in progress: a browser and Tauri tabletop application. The current preview uses sample game state; it is not a connected multiplayer match.

Use Bun 1.3.11 and the Rust toolchain pinned in `rust-toolchain.toml`. Run `bun install --frozen-lockfile`, `bun run play:check`, `bun run play:test`, and `cargo test --workspace` for source checks. Runtime artwork is supplied separately and is not included in this repository.

## Build & Card Support

[![Multi-OS Release](https://github.com/Spellwake/spellwake/actions/workflows/tauri.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/tauri.yml)
[![Rust](https://github.com/Spellwake/spellwake/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/rust.yml)
[![Application API](https://github.com/Spellwake/spellwake/actions/workflows/application-api.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/application-api.yml)
[![Domain Health](https://github.com/Spellwake/spellwake/actions/workflows/domain-health.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/domain-health.yml)
[![Catalog Refresh](https://github.com/Spellwake/spellwake/actions/workflows/catalog-refresh.yml/badge.svg?branch=main)](https://github.com/Spellwake/spellwake/actions/workflows/catalog-refresh.yml)

![Parser](https://raw.githubusercontent.com/Spellwake/spellwake/badges/card-support-ast.svg)
![Rules Engine](https://raw.githubusercontent.com/Spellwake/spellwake/badges/card-support-engine.svg)

## System Health

| Domain | Local CI | Live |
| --- | --- | --- |
| **Patreon** | ![Patreon local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-patreon.svg) | ![Patreon live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-patreon-live.svg) |
| **Stripe** | ![Stripe local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-stripe.svg) | ![Stripe live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-stripe-live.svg) |
| **AI** | ![AI local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-ai.svg) | ![AI live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-ai-live.svg) |
| **Matchmaking** | ![Matchmaking local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-matchmaking.svg) | ![Matchmaking live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-matchmaking-live.svg) |
| **Predictions** | ![Predictions local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-predictions.svg) | ![Predictions live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-predictions-live.svg) |
| **Market** | ![Market local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-market.svg) | ![Market live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-market-live.svg) |
| **Pet cosmetics** | ![Pet cosmetics local](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-pet-cosmetics.svg) | ![Pet cosmetics live](https://raw.githubusercontent.com/Spellwake/spellwake/badges/health-pet-cosmetics-live.svg) |

Updated automatically from `main` and daily CI (`badges` branch). Live checks require hosted secrets; pet cosmetics are not wired yet.

<details>
<summary>Hosted credentials for live checks</summary>

- **Patreon**: `SPELLWAKE_PATREON_CLIENT_ID`, `SPELLWAKE_PATREON_CLIENT_SECRET`, `SPELLWAKE_PATREON_REDIRECT_URI`, `SPELLWAKE_PATREON_WEBHOOK_SECRET`, `SPELLWAKE_PATREON_EVIDENCE_BUCKET`
- **Stripe**: `SPELLWAKE_STRIPE_SECRET_KEY`, `SPELLWAKE_STRIPE_WEBHOOK_SECRET`, `SPELLWAKE_STRIPE_API_VERSION`, `SPELLWAKE_STRIPE_EVIDENCE_BUCKET`
- **AI**: `SPELLWAKE_AI_PROVIDER`, `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GEMINI_API_KEY`, `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, `LANGFUSE_BASE_URL`
- **Hosted API & Storage**: `SPELLWAKE_SUPABASE_URL`, `SPELLWAKE_SUPABASE_PUBLISHABLE_KEY`, `SPELLWAKE_SUPABASE_SECRET_KEY`, `SPELLWAKE_APPLICATION_API_URL`, `SPELLWAKE_ROOM_INVITATION_KEY`, `SPELLWAKE_RECONCILE_SECRET`, `SPELLWAKE_PRICE_BUCKET`, `SPELLWAKE_CATALOGUE_BUCKET`

</details>

No software license is granted by publishing this source. Magic: The Gathering and its card imagery belong to Wizards of the Coast and their respective artists. This is unofficial fan content, not approved or endorsed by Wizards. Third-party dependencies retain their own licenses.
