# Spellwake Testing and Benchmark Infrastructure

Spellwake keeps unit and crate integration tests colocated with their owning Cargo crates, services, and Bun workspaces, while organizing all domain suite manifests, subdomains, cross-system fixtures, reference comparisons, and declarative benchmarks under `tests/`.

## Directory Layout

```text
tests/
  README.md                        # This guide
  registry.json                    # Master index of all 15 domain suites and benchmark roots
  domains/                         # One directory per Spellwake domain
    patreon/suite.json             # Active: oauth-association, webhook-intake, membership-refresh, financial-integrity
    stripe/suite.json              # Active: webhooks, checkout, settlement, purchase contracts
    matchmaking/suite.json         # Active: intents, formats, lobbies, rooms, invitations
    predictions/suite.json         # Active: price ingestion, orders, publication, resolution
    market/suite.json              # Active: listings, trades, pack shop, inventory
    cosmetics/suite.json           # Active (partial): card-backs, sleeves, dice, playmats; pets unqualified
    account-auth/suite.json        # Active: sign-in-and-sessions, onboarding-and-profile, account-deletion
    decks-collection/suite.json    # Active: deck-shelf-and-builder, deck-import, card-collection, pack-opening
    custom-cards/suite.json        # Active: compiler-and-ast, studio-and-vault, export-and-import, patron-play-gate
    social/suite.json              # Active: friends-and-contacts, direct-messages, read-receipts-and-notifications
    league-progression/suite.json  # Active: standings-and-ranks, season-and-quests, match-history-and-replays
    rendering/suite.json           # Active: 3d-tabletop (WebGL2 benchmark), ui-responsive (12x3), component-gallery
    oracle-parser/suite.json       # Automated daily Scryfall polling + real-card semantic suite
    rules-engine/
      suite.json                   # Hold full suite until engine completes; regression checks + benchmarks
      benchmarks/
        schema.json                # JSON Schema for declarative MTG rules interaction benchmarks
        layers-and-sagas/
          urzas-saga-blood-moon.benchmark.json
          humility-opalescence-timestamps.benchmark.json
        replacement-effects/
          rest-in-peace-and-doubling-season.benchmark.json
        copy-effects/
          clone-and-copyable-values.benchmark.json
        combat-and-sba/
          deathtouch-trample-lifelink-and-sba.benchmark.json
        stack-and-priority/
          split-second-and-apnap-triggers.benchmark.json
    ai/suite.json                  # Hold gameplay AI expansion until rules engine completes; offline quota/slot checks active
  catalogue/                       # Card catalogue fixtures and health/refresh unit tests
  reference/                       # Forge and XMage reference harnesses, evidence, and guards
  scenarios/                       # Low-level projection expectation fixtures
  support/                         # Shared CI vocabulary preparation and Rust contract cases
```

## Single Source of Truth

`tests/registry.json` and `tests/domains/<slug>/suite.json` are the authoritative definition of every domain and subdomain's checks. `scripts/health_checks.py` reads these manifests directly instead of hardcoding domain test commands in Python, and `.github/workflows/domain-health.yml` runs both unit/Postgres checks and browser E2E suites (`--include-browser-e2e`).

Honesty invariants enforced across all suites and badges:

1. A command that runs zero tests never reports green; it degrades to `unknown`.
2. Unqualified domains or subdomains (such as `cosmetics` -> `pets`) never render green; domains with an unqualified subdomain degrade to `local-partial` (yellow) while surfacing the exact gap in `unqualifiedSubdomains`.
3. Preemptive rules benchmarks with `execution_status: "pending-engine"` are validated strictly against `schema.json`, counted honestly as pending, and never marked `passing` without an execution receipt.

## Common Commands

Validate the test registry, all 15 domain/subdomain manifests, and all Rules Engine benchmark specs:

```sh
python3 scripts/rules_benchmarks.py --list
```

Run domain health checks for one or more domains (add `--include-browser-e2e` to run Playwright browser suites too):

```sh
python3 scripts/health_checks.py --domain patreon --domain stripe --skip-db --include-browser-e2e --output work/health-status.json
```

Run standalone Playwright browser E2E and rendering benchmark suites directly:

```sh
bun run play:qualify-patreon
bun run play:qualify-stripe
bun run play:qualify-matchmaking
bun run play:qualify-predictions
bun run play:qualify-market
bun run play:qualify-auth
bun run play:qualify-collection
bun run play:qualify-custom-cards
bun run play:qualify-social
bun run play:qualify-league
bun run play:qualify-responsive
bun run play:qualify-3d
bun run controls:smoke
```

Run the suite/registry/benchmark validator unit tests:

```sh
python3 -m unittest discover -s tests/catalogue -p 'test_*.py'
```

## Adding a New Rules Engine Interaction Benchmark

1. Pick or create a `kebab-case` category folder under `tests/domains/rules-engine/benchmarks/` (for example, `layers-and-sagas/`, `stack-and-priority/`, `replacement-effects/`, `copy-effects/`, `combat-and-sba/`).
2. Add `<interaction-slug>.benchmark.json` matching `tests/domains/rules-engine/benchmarks/schema.json` with verified Scryfall `oracle_id`s, pinned MTG Comprehensive Rules citations, and alphabetically sorted `required_capabilities`.
3. Run `python3 scripts/rules_benchmarks.py --list` to verify schema compliance, CR citations, card references, and invariants.
