#!/usr/bin/env python3
"""Run real domain health checks and generate honest README SVG/JSON badges.

Covers the seven tracked domains:
- Patreon
- Stripe
- AI
- Matchmaking
- Predictions
- Market
- Pet cosmetics (explicitly unknown while end-to-end slot/entitlement checks are absent)

Honesty invariants:
1. A command that executes zero tests is NEVER treated as passing; it degrades to `unknown`.
2. Absent or unqualified checks (such as pet cosmetics and credential-gated live providers)
   always render visibly `unknown` (grey badge), never green.
3. Local fixture/Postgres checks are labeled explicitly as local checks (`local verified`),
   distinct from hosted/live provider probes (`live: unknown`).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import resource
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
PLAY_DIR = ROOT / "apps" / "play"
APP_MANIFEST = ROOT / "services" / "application" / "Cargo.toml"
DB_HARNESS = ROOT / "services" / "application" / "tools" / "test_database.py"

sys.path.insert(0, str(Path(__file__).resolve().parent))
import badge_svg  # noqa: E402


@dataclass(frozen=True)
class CommandSpec:
    kind: str  # "cargo" | "bun" | "postgres_db"
    label: str
    args: tuple[str, ...]
    cwd: Path = ROOT
    env_extra: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class DomainSpec:
    slug: str
    label: str
    rust_checks: tuple[CommandSpec, ...]
    db_filters: tuple[str, ...]
    frontend_checks: tuple[CommandSpec, ...]
    hosted_credentials: tuple[str, ...]
    unqualified_reason: str | None = None
    notes: str = ""


DOMAINS: tuple[DomainSpec, ...] = (
    DomainSpec(
        slug="patreon",
        label="Patreon",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application patreon unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "patreon",
                ),
            ),
        ),
        db_filters=(
            "patreon_association",
            "patreon_intake",
            "patreon_refresh",
            "patreon_financial_integrity",
            "patreon_notification_processing_integrity",
        ),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend patreon contract tests",
                args=(
                    "bun",
                    "test",
                    "src/data/patreon-association-api.test.ts",
                    "src/data/patreon-link.test.ts",
                ),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_PATREON_CLIENT_ID",
            "SPELLWAKE_PATREON_CLIENT_SECRET",
            "SPELLWAKE_PATREON_REDIRECT_URI",
            "SPELLWAKE_PATREON_WEBHOOK_SECRET",
            "SPELLWAKE_PATREON_EVIDENCE_BUCKET",
        ),
        notes="Retained source/association fixtures, Postgres RLS/financial integrity, and frontend API contracts; live Patreon OAuth/webhooks require hosted credentials.",
    ),
    DomainSpec(
        slug="stripe",
        label="Stripe",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application stripe_webhooks official SDK vectors",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--test",
                    "stripe_webhooks",
                ),
            ),
            CommandSpec(
                kind="cargo",
                label="application stripe client/service unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "stripe",
                ),
            ),
        ),
        db_filters=("stripe",),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend stripe checkout contracts",
                args=(
                    "bun",
                    "test",
                    "src/data/checkout-intent.test.ts",
                    "src/data/purchase-rows.test.ts",
                ),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_STRIPE_SECRET_KEY",
            "SPELLWAKE_STRIPE_WEBHOOK_SECRET",
            "SPELLWAKE_STRIPE_API_VERSION",
            "SPELLWAKE_STRIPE_EVIDENCE_BUCKET",
        ),
        notes="Retained official Stripe SDK webhook vectors, client/service policy tests, Postgres settlement tests, and frontend checkout tests; live merchant checkout/portal require hosted credentials.",
    ),
    DomainSpec(
        slug="ai",
        label="AI",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application ai_runtime offline tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "ai_runtime",
                ),
            ),
        ),
        db_filters=(
            "ai_runtime_integrity",
            "ai_ready_integrity",
            "ai_budget_integrity",
            "ai_ante_port_contracts",
            "manual_ai_slot",
        ),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend ai-tier-quota contracts",
                args=("bun", "test", "src/data/ai-tier-quota.test.ts"),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_AI_PROVIDER",
            "OPENAI_API_KEY / ANTHROPIC_API_KEY / GEMINI_API_KEY",
            "LANGFUSE_PUBLIC_KEY",
            "LANGFUSE_SECRET_KEY",
            "LANGFUSE_BASE_URL",
        ),
        notes="Offline scripted AI runtime, compact prompt/schema, quota/budget/slot Postgres integrity, and frontend tier-quota tests; live LLM providers and Langfuse tracing require hosted credentials. Revealed-card bot memory is owned separately.",
    ),
    DomainSpec(
        slug="matchmaking",
        label="Matchmaking",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application qualified_intent_contracts",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--test",
                    "qualified_intent_contracts",
                ),
            ),
            CommandSpec(
                kind="cargo",
                label="application preview-format isolation tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--test",
                    "formats",
                ),
            ),
            CommandSpec(
                kind="cargo",
                label="application matchmaking/room/format unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "matchmaking",
                ),
            ),
        ),
        db_filters=(
            "matchmaking_api",
            "queue_intent_integrity",
            "lobbies",
            "lobby_access",
            "lobby_entry_integrity",
            "lobby_cleanup_integrity",
            "room_configuration",
            "room_start_integrity",
            "room_invitations",
        ),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend room-journal contracts",
                args=("bun", "test", "src/data/room-journal.test.ts"),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_SUPABASE_URL",
            "SPELLWAKE_SUPABASE_PUBLISHABLE_KEY",
            "SPELLWAKE_SUPABASE_SECRET_KEY",
            "SPELLWAKE_APPLICATION_API_URL",
            "SPELLWAKE_ROOM_INVITATION_KEY",
        ),
        notes="Qualified intent contracts, queue/lobby/room Postgres integrity, and frontend room journal tests; hosted live matchmaking queue requires deployed Supabase and Fly API.",
    ),
    DomainSpec(
        slug="predictions",
        label="Predictions",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application price_source tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--test",
                    "price_source",
                ),
            ),
            CommandSpec(
                kind="cargo",
                label="application prediction unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "prediction",
                ),
            ),
        ),
        db_filters=(
            "predictions",
            "prediction_commands",
            "prediction_reads",
            "prediction_publication",
            "prediction_resolution",
            "pricing",
        ),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend prediction contracts",
                args=(
                    "bun",
                    "test",
                    "src/data/prediction-api.test.ts",
                    "src/data/prediction-publication-api.test.ts",
                    "src/data/prediction-reads-api.test.ts",
                    "src/data/prediction-rules.test.ts",
                ),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_SUPABASE_URL",
            "SPELLWAKE_SUPABASE_SECRET_KEY",
            "SPELLWAKE_PRICE_BUCKET",
        ),
        notes="Price source ingestion, prediction market/order/settlement Postgres integrity, and frontend prediction rules; live daily Scryfall price capture and Storage retention require hosted credentials.",
    ),
    DomainSpec(
        slug="market",
        label="Market",
        rust_checks=(
            CommandSpec(
                kind="cargo",
                label="application pack/shop unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "pack",
                ),
            ),
            CommandSpec(
                kind="cargo",
                label="application exchange unit tests",
                args=(
                    "cargo",
                    "test",
                    "--locked",
                    "--manifest-path",
                    str(APP_MANIFEST),
                    "--lib",
                    "exchange",
                ),
            ),
        ),
        db_filters=(
            "market",
            "trades",
            "economy_commands",
            "exchange_reads",
            "packs",
            "pack_shop",
            "inventory",
        ),
        frontend_checks=(
            CommandSpec(
                kind="bun",
                label="frontend market/trade/economy contracts",
                args=(
                    "bun",
                    "test",
                    "src/data/market-rules.test.ts",
                    "src/data/trade-rules.test.ts",
                    "src/data/economy-api.test.ts",
                    "src/data/economy-journal.test.ts",
                    "src/data/economy-queries.test.ts",
                    "src/data/exchange-api.test.ts",
                ),
                cwd=PLAY_DIR,
            ),
        ),
        hosted_credentials=(
            "SPELLWAKE_SUPABASE_URL",
            "SPELLWAKE_SUPABASE_PUBLISHABLE_KEY",
            "SPELLWAKE_SUPABASE_SECRET_KEY",
            "SPELLWAKE_APPLICATION_API_URL",
        ),
        notes="Marketplace listings, direct trades, pack shop, inventory Postgres integrity, and frontend economy/market rules; hosted live exchange and real-world supply feeds require hosted configuration.",
    ),
    DomainSpec(
        slug="pet-cosmetics",
        label="Pet cosmetics",
        rust_checks=(),
        db_filters=(),
        frontend_checks=(),
        hosted_credentials=(
            "Approved pet cosmetic assets (Spellwake/spellwake-assets)",
            "Connected pet appearance slot + entitlement/equip DB & API tests",
        ),
        unqualified_reason=(
            "Unqualified: migration 20261121028300 adds cosmetic kind='pet', profile.default_pet_id, "
            "deck.pet_id, and GET /v1/cosmetics/catalogue?kind=pet, but AppearanceSelection "
            "(services/application/src/cosmetics.rs) and connected-cosmetics.ts have no pet slot, "
            "no pet equip/entitlement DB tests exist, and no approved pet assets are published."
        ),
        notes="Kept visibly unknown until connected pet appearance selection, entitlement/equip tests, and approved pet assets exist.",
    ),
)

CARGO_RESULT_RE = re.compile(
    r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
)
BUN_PASS_RE = re.compile(r"^\s*(\d+)\s+pass\b", re.MULTILINE)
BUN_FAIL_RE = re.compile(r"^\s*(\d+)\s+fail\b", re.MULTILINE)


def parse_test_counts(kind: str, output: str) -> tuple[int, int, int]:
    """Extract (passed, failed, ignored) counts from cargo test or bun test output."""
    if kind in ("cargo", "postgres_db"):
        passed = 0
        failed = 0
        ignored = 0
        for match in CARGO_RESULT_RE.finditer(output):
            passed += int(match.group(2))
            failed += int(match.group(3))
            ignored += int(match.group(4))
        return passed, failed, ignored
    if kind == "bun":
        p_match = BUN_PASS_RE.search(output)
        f_match = BUN_FAIL_RE.search(output)
        return (
            int(p_match.group(1)) if p_match else 0,
            int(f_match.group(1)) if f_match else 0,
            0,
        )
    return 0, 0, 0


def _raise_nofile_limit() -> None:
    """Ensure child processes (notably Bun on macOS) do not hit a 256-FD soft limit."""
    try:
        soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
        target = min(max(soft, 4096), hard if hard != resource.RLIM_INFINITY else 65536)
        if target > soft:
            resource.setrlimit(resource.RLIMIT_NOFILE, (target, hard))
    except (OSError, ValueError):
        pass


def run_command_spec(spec: CommandSpec) -> dict[str, Any]:
    """Execute one real check command and parse its test count honestly."""
    _raise_nofile_limit()
    env = os.environ.copy()
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / "services" / "application" / "target"))
    env.update(spec.env_extra)
    started = time.monotonic()
    proc = subprocess.run(
        list(spec.args),
        cwd=spec.cwd,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    elapsed_ms = int((time.monotonic() - started) * 1000)
    combined = (proc.stdout or "") + "\n" + (proc.stderr or "")
    passed, failed, ignored = parse_test_counts(spec.kind, combined)
    if proc.returncode != 0 or failed > 0:
        status = "failing"
    elif passed == 0:
        # Honesty invariant: a filter matching zero tests never produces a green badge.
        status = "unknown"
    else:
        status = "passing"
    tail_lines = [line for line in combined.splitlines() if line.strip()][-8:]
    return {
        "kind": spec.kind,
        "label": spec.label,
        "status": status,
        "exitCode": proc.returncode,
        "passed": passed,
        "failed": failed,
        "ignored": ignored,
        "elapsedMs": elapsed_ms,
        "outputTail": tail_lines,
    }


def postgres_container_ready() -> bool:
    """Return True if the isolated local Postgres 17 test container is running on port 55439."""
    try:
        out = subprocess.check_output(
            ["docker", "inspect", "spellwake-application-db"],
            stderr=subprocess.DEVNULL,
            text=True,
        )
        info = json.loads(out)[0]
        return (
            info["Config"]["Image"] == "postgres:17"
            and info["HostConfig"]["PortBindings"]
            == {"5432/tcp": [{"HostIp": "127.0.0.1", "HostPort": "55439"}]}
            and bool(info["State"]["Running"])
        )
    except (subprocess.CalledProcessError, FileNotFoundError, json.JSONDecodeError, KeyError, IndexError):
        return False


def evaluate_domain(
    spec: DomainSpec,
    *,
    run_db: bool,
    run_frontend: bool,
    db_available: bool,
) -> dict[str, Any]:
    """Run all configured real checks for one domain and classify its badge honestly."""
    if spec.unqualified_reason is not None:
        return {
            "slug": spec.slug,
            "label": spec.label,
            "localStatus": "unknown",
            "localBadge": {
                "label": spec.label,
                "message": "unknown (unqualified)",
                "color": "lightgrey",
            },
            "liveStatus": "unknown",
            "liveBadge": {
                "label": f"{spec.label} live",
                "message": "unknown",
                "color": "lightgrey",
            },
            "passedTests": 0,
            "failedTests": 0,
            "checks": [],
            "unqualifiedReason": spec.unqualified_reason,
            "hostedCredentialsRequired": list(spec.hosted_credentials),
            "notes": spec.notes,
        }

    results: list[dict[str, Any]] = []
    for check in spec.rust_checks:
        results.append(run_command_spec(check))

    db_skipped_reason: str | None = None
    if spec.db_filters:
        if run_db and db_available:
            args = ["python3", str(DB_HARNESS)]
            for flt in spec.db_filters:
                args.extend(["--filter", flt])
            results.append(
                run_command_spec(
                    CommandSpec(
                        kind="postgres_db",
                        label=f"isolated Postgres DB tests ({len(spec.db_filters)} suites)",
                        args=tuple(args),
                    )
                )
            )
        elif not run_db:
            db_skipped_reason = "skipped by --skip-db"
        else:
            db_skipped_reason = "isolated Postgres container spellwake-application-db not running"

    if run_frontend:
        for check in spec.frontend_checks:
            results.append(run_command_spec(check))

    total_passed = sum(r["passed"] for r in results)
    total_failed = sum(r["failed"] for r in results)
    any_failing = any(r["status"] == "failing" for r in results)
    any_unknown = any(r["status"] == "unknown" for r in results)

    if any_failing:
        local_status = "failing"
        badge_msg = (
            f"failing ({total_failed} failed)"
            if total_failed > 0
            else "failing (check error)"
        )
        badge_color = "red"
    elif not results or total_passed == 0 or any_unknown:
        local_status = "unknown"
        badge_msg = "unknown"
        badge_color = "lightgrey"
    elif db_skipped_reason is not None:
        local_status = "local-partial"
        badge_msg = f"unit/web {total_passed} passed (DB unrun)"
        badge_color = "yellow"
    else:
        local_status = "local-verified"
        badge_msg = f"local verified ({total_passed} tests)"
        badge_color = "green"

    return {
        "slug": spec.slug,
        "label": spec.label,
        "localStatus": local_status,
        "localBadge": {
            "label": spec.label,
            "message": badge_msg,
            "color": badge_color,
        },
        "liveStatus": "unknown",
        "liveBadge": {
            "label": f"{spec.label} live",
            "message": "unknown (needs hosted creds)",
            "color": "lightgrey",
        },
        "passedTests": total_passed,
        "failedTests": total_failed,
        "dbSkippedReason": db_skipped_reason,
        "checks": results,
        "hostedCredentialsRequired": list(spec.hosted_credentials),
        "notes": spec.notes,
    }


def write_domain_badges(report: dict[str, Any], badges_dir: Path) -> dict[str, Any]:
    """Render deterministic SVG badges and Shields endpoint descriptors for every domain."""
    endpoints: dict[str, Any] = {}
    for domain in report["domains"]:
        slug = domain["slug"]
        lb = domain["localBadge"]
        endpoints[slug] = badge_svg.write_badge(
            badges_dir,
            f"health-{slug}",
            lb["label"],
            lb["message"],
            lb["color"],
            is_error=domain["localStatus"] == "failing",
        )
        live_b = domain["liveBadge"]
        endpoints[f"{slug}-live"] = badge_svg.write_badge(
            badges_dir,
            f"health-{slug}-live",
            live_b["label"],
            live_b["message"],
            live_b["color"],
            is_error=False,
        )
    badge_svg.write_atomic_json(badges_dir / "health-endpoints.json", endpoints)
    return endpoints


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--domain",
        action="append",
        choices=[d.slug for d in DOMAINS],
        help="Run only selected domain(s); omit to run all seven domains.",
    )
    parser.add_argument("--skip-db", action="store_true", help="Skip isolated Postgres DB suites")
    parser.add_argument("--skip-frontend", action="store_true", help="Skip frontend Bun contract tests")
    parser.add_argument("--output", type=Path, required=True, help="Path to write health-status.json")
    parser.add_argument("--badges-dir", type=Path, default=None, help="Directory to write SVG/JSON badges")
    args = parser.parse_args()

    selected = (
        [d for d in DOMAINS if d.slug in set(args.domain)]
        if args.domain
        else list(DOMAINS)
    )
    db_available = False if args.skip_db else postgres_container_ready()

    domain_reports = [
        evaluate_domain(
            spec,
            run_db=not args.skip_db,
            run_frontend=not args.skip_frontend,
            db_available=db_available,
        )
        for spec in selected
    ]

    report = {
        "schemaVersion": 1,
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "postgresContainerDetected": db_available,
        "liveProvidersActivated": False,
        "domains": domain_reports,
    }
    badge_svg.write_atomic_json(args.output, report)

    if args.badges_dir is not None:
        write_domain_badges(report, args.badges_dir)

    print(json.dumps(report, indent=2))
    any_failed = any(d["localStatus"] == "failing" for d in domain_reports)
    return 1 if any_failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
