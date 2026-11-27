#!/usr/bin/env python3
"""Run real domain health checks and generate honest README SVG/JSON badges.

Domain definitions are loaded from tests/registry.json and
tests/domains/<slug>/suite.json so the test layout is the single source of truth.

Also validates and summarizes declarative Rules Engine interaction benchmarks
from tests/domains/rules-engine/benchmarks/ via scripts/rules_benchmarks.py.

Honesty invariants:
1. A command that executes zero tests is NEVER treated as passing; it degrades to `unknown`.
2. Absent or unqualified checks (such as pet cosmetics and credential-gated live providers)
   always render visibly `unknown` (grey badge), never green.
3. Local fixture/Postgres checks are labeled explicitly as local checks (`local verified`),
   distinct from hosted/live provider probes (`live: unknown`).
4. Preemptive rules benchmarks with execution_status='pending-engine' never render green.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    import resource
except ImportError:  # Windows does not ship POSIX resource module.
    resource = None  # type: ignore[assignment]

ROOT = Path(__file__).resolve().parents[1]
PLAY_DIR = ROOT / "apps" / "play"
APP_MANIFEST = ROOT / "services" / "application" / "Cargo.toml"
DB_HARNESS = ROOT / "services" / "application" / "tools" / "test_database.py"
REGISTRY_PATH = ROOT / "tests" / "registry.json"

sys.path.insert(0, str(Path(__file__).resolve().parent))
import badge_svg  # noqa: E402
import rules_benchmarks  # noqa: E402


@dataclass(frozen=True)
class CommandSpec:
    kind: str  # "cargo" | "bun" | "postgres_db" | "python"
    label: str
    args: tuple[str, ...]
    cwd: Path = ROOT
    env_extra: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class SubdomainSpec:
    slug: str
    label: str
    description: str
    unqualified_reason: str | None = None
    related_paths: tuple[str, ...] = ()


@dataclass(frozen=True)
class DomainSpec:
    slug: str
    label: str
    rust_checks: tuple[CommandSpec, ...]
    db_filters: tuple[str, ...]
    frontend_checks: tuple[CommandSpec, ...]
    hosted_credentials: tuple[str, ...]
    browser_e2e_checks: tuple[CommandSpec, ...] = ()
    python_checks: tuple[CommandSpec, ...] = ()
    subdomains: tuple[SubdomainSpec, ...] = ()
    unqualified_reason: str | None = None
    local_only: bool = False
    category: str = "service-domain"
    readiness_stage: str = "active"
    notes: str = ""


def _resolve_placeholder(value: str) -> str:
    return (
        value.replace("{ROOT}", str(ROOT))
        .replace("{APP_MANIFEST}", str(APP_MANIFEST))
        .replace("{PYTHON}", sys.executable)
    )


def _command_spec_from_manifest(raw: dict[str, Any]) -> CommandSpec:
    cwd_raw = raw.get("cwd", ".")
    cwd = ROOT if cwd_raw in ("", ".") else (ROOT / cwd_raw)
    args = tuple(_resolve_placeholder(str(a)) for a in raw["args"])
    env_extra = {
        str(k): _resolve_placeholder(str(v))
        for k, v in (raw.get("envExtra") or {}).items()
    }
    return CommandSpec(
        kind=str(raw["kind"]),
        label=str(raw["label"]),
        args=args,
        cwd=cwd,
        env_extra=env_extra,
    )


def load_domains_from_registry(
    registry_path: Path = REGISTRY_PATH,
    repo_root: Path = ROOT,
) -> tuple[DomainSpec, ...]:
    """Load all DomainSpec definitions from tests/registry.json and tests/domains/*/suite.json."""
    registry = json.loads(registry_path.read_text(encoding="utf-8"))
    specs: list[DomainSpec] = []
    for entry in registry["domains"]:
        suite_path = repo_root / entry["suitePath"]
        suite = json.loads(suite_path.read_text(encoding="utf-8"))
        checks = suite.get("checks", {})
        subdomains = tuple(
            SubdomainSpec(
                slug=str(sub["slug"]),
                label=str(sub["label"]),
                description=str(sub.get("description", "")),
                unqualified_reason=sub.get("unqualifiedReason"),
                related_paths=tuple(str(p) for p in sub.get("relatedPaths", [])),
            )
            for sub in suite.get("subdomains", [])
        )
        specs.append(
            DomainSpec(
                slug=str(suite["slug"]),
                label=str(suite["label"]),
                category=str(suite.get("category", "service-domain")),
                readiness_stage=str(suite.get("readinessStage", "active")),
                rust_checks=tuple(
                    _command_spec_from_manifest(c) for c in checks.get("rust", [])
                ),
                db_filters=tuple(str(f) for f in checks.get("postgresDbFilters", [])),
                frontend_checks=tuple(
                    _command_spec_from_manifest(c) for c in checks.get("frontend", [])
                ),
                browser_e2e_checks=tuple(
                    _command_spec_from_manifest(c) for c in checks.get("browserE2e", [])
                ),
                python_checks=tuple(
                    _command_spec_from_manifest(c) for c in checks.get("python", [])
                ),
                subdomains=subdomains,
                hosted_credentials=tuple(
                    str(c) for c in suite.get("hostedCredentials", [])
                ),
                unqualified_reason=suite.get("unqualifiedReason"),
                local_only=bool(suite.get("localOnly", False)),
                notes=str(suite.get("notes", "")),
            )
        )
    return tuple(specs)


DOMAINS: tuple[DomainSpec, ...] = load_domains_from_registry()

CARGO_RESULT_RE = re.compile(
    r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;"
)
BUN_PASS_RE = re.compile(r"^\s*(\d+)\s+pass\b", re.MULTILINE)
BUN_FAIL_RE = re.compile(r"^\s*(\d+)\s+fail\b", re.MULTILINE)
PYTHON_RAN_RE = re.compile(r"^Ran (\d+) tests? in ", re.MULTILINE)
PYTHON_FAIL_RE = re.compile(r"failures=(\d+)")
PYTHON_ERR_RE = re.compile(r"errors=(\d+)")
PYTHON_SKIP_RE = re.compile(r"skipped=(\d+)")


def parse_test_counts(kind: str, output: str) -> tuple[int, int, int]:
    """Extract (passed, failed, ignored) counts from cargo, bun, or python unittest output."""
    if kind in ("cargo", "postgres_db"):
        passed = 0
        failed = 0
        ignored = 0
        for match in CARGO_RESULT_RE.finditer(output):
            passed += int(match.group(2))
            failed += int(match.group(3))
            ignored += int(match.group(4))
        return passed, failed, ignored
    if kind in ("bun", "playwright"):
        passed = sum(int(m.group(1)) for m in BUN_PASS_RE.finditer(output))
        failed = sum(int(m.group(1)) for m in BUN_FAIL_RE.finditer(output))
        return passed, failed, 0
    if kind == "python":
        ran_match = PYTHON_RAN_RE.search(output)
        if not ran_match:
            return 0, 0, 0
        ran = int(ran_match.group(1))
        fail_match = PYTHON_FAIL_RE.search(output)
        err_match = PYTHON_ERR_RE.search(output)
        skip_match = PYTHON_SKIP_RE.search(output)
        failed = (int(fail_match.group(1)) if fail_match else 0) + (
            int(err_match.group(1)) if err_match else 0
        )
        skipped = int(skip_match.group(1)) if skip_match else 0
        passed = max(0, ran - failed - skipped)
        return passed, failed, skipped
    return 0, 0, 0


def _raise_nofile_limit() -> None:
    """Ensure child processes (notably Bun on macOS) do not hit a 256-FD soft limit."""
    if resource is None:
        return
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
    run_browser_e2e: bool = False,
) -> dict[str, Any]:
    """Run all configured real checks for one domain and classify its badge honestly."""
    if spec.unqualified_reason is not None:
        return {
            "slug": spec.slug,
            "label": spec.label,
            "category": spec.category,
            "localStatus": "unknown",
            "localBadge": {
                "label": "",
                "message": "unknown",
                "color": "lightgrey",
                "ariaLabel": f"{spec.label} local: unknown (unqualified)",
            },
            "liveStatus": "unknown",
            "liveBadge": {
                "label": "",
                "message": "unknown",
                "color": "lightgrey",
                "ariaLabel": f"{spec.label} live: unknown",
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

    for check in spec.python_checks:
        results.append(run_command_spec(check))

    db_skipped_reason: str | None = None
    if spec.db_filters:
        if run_db and db_available:
            args = [sys.executable, str(DB_HARNESS)]
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

    if run_browser_e2e:
        for check in spec.browser_e2e_checks:
            results.append(run_command_spec(check))

    total_passed = sum(r["passed"] for r in results)
    total_failed = sum(r["failed"] for r in results)
    any_failing = any(r["status"] == "failing" for r in results)
    any_unknown = any(r["status"] == "unknown" for r in results)

    unqualified_subdomains = [
        {
            "slug": sub.slug,
            "label": sub.label,
            "unqualifiedReason": sub.unqualified_reason,
        }
        for sub in spec.subdomains
        if sub.unqualified_reason is not None
    ]

    if any_failing:
        local_status = "failing"
        badge_msg = f"fail · {total_failed}" if total_failed > 0 else "fail · error"
        badge_color = "red"
    elif not results or total_passed == 0 or any_unknown:
        local_status = "unknown"
        badge_msg = "unknown"
        badge_color = "lightgrey"
    elif db_skipped_reason is not None or unqualified_subdomains:
        local_status = "local-partial"
        badge_msg = f"partial · {total_passed}"
        badge_color = "yellow"
    else:
        local_status = "local-verified"
        badge_msg = f"pass · {total_passed}"
        badge_color = "green"

    return {
        "slug": spec.slug,
        "label": spec.label,
        "category": spec.category,
        "readinessStage": spec.readiness_stage,
        "localStatus": local_status,
        "localBadge": {
            "label": "",
            "message": badge_msg,
            "color": badge_color,
            "ariaLabel": f"{spec.label} local: {badge_msg}",
        },
        "liveStatus": "not-applicable" if spec.local_only else "unknown",
        "liveBadge": (
            {
                "label": "",
                "message": "n/a · offline",
                "color": "lightgrey",
                "ariaLabel": f"{spec.label} live: not applicable (offline deterministic check)",
            }
            if spec.local_only
            else {
                "label": "",
                "message": "unknown",
                "color": "lightgrey",
                "ariaLabel": f"{spec.label} live: unknown (needs hosted credentials)",
            }
        ),
        "passedTests": total_passed,
        "failedTests": total_failed,
        "dbSkippedReason": db_skipped_reason,
        "subdomains": [
            {
                "slug": sub.slug,
                "label": sub.label,
                "status": "unknown" if sub.unqualified_reason is not None else local_status,
                "unqualifiedReason": sub.unqualified_reason,
            }
            for sub in spec.subdomains
        ],
        "unqualifiedSubdomains": unqualified_subdomains,
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
            aria_label=lb.get("ariaLabel"),
        )
        live_b = domain["liveBadge"]
        endpoints[f"{slug}-live"] = badge_svg.write_badge(
            badges_dir,
            f"health-{slug}-live",
            live_b["label"],
            live_b["message"],
            live_b["color"],
            is_error=False,
            aria_label=live_b.get("ariaLabel"),
        )

    bench_report = report.get("rulesBenchmarks")
    if isinstance(bench_report, dict):
        endpoints["rules-engine-benchmarks"] = rules_benchmarks.write_benchmark_badge(
            bench_report, badges_dir
        )

    badge_svg.write_atomic_json(badges_dir / "health-endpoints.json", endpoints)
    return endpoints


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--domain",
        action="append",
        choices=[d.slug for d in DOMAINS],
        help="Run only selected domain(s); omit to run all registered domains.",
    )
    parser.add_argument("--skip-db", action="store_true", help="Skip isolated Postgres DB suites")
    parser.add_argument("--skip-frontend", action="store_true", help="Skip frontend Bun contract tests")
    parser.add_argument(
        "--include-browser-e2e",
        action="store_true",
        help="Also run Playwright browser E2E checks declared under checks.browserE2e",
    )
    parser.add_argument("--skip-benchmarks", action="store_true", help="Skip rules benchmark spec validation")
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
            run_browser_e2e=args.include_browser_e2e,
        )
        for spec in selected
    ]
    benchmark_report = (
        None if args.skip_benchmarks else rules_benchmarks.evaluate_benchmarks()
    )

    report: dict[str, Any] = {
        "schemaVersion": 1,
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "registryPath": "tests/registry.json",
        "postgresContainerDetected": db_available,
        "liveProvidersActivated": False,
        "domains": domain_reports,
    }
    if benchmark_report is not None:
        report["rulesBenchmarks"] = benchmark_report

    badge_svg.write_atomic_json(args.output, report)

    if args.badges_dir is not None:
        write_domain_badges(report, args.badges_dir)

    print(json.dumps(report, indent=2))
    any_failed = any(d["localStatus"] == "failing" for d in domain_reports)
    benchmarks_failed = (
        benchmark_report is not None and benchmark_report["status"] == "failing"
    )
    return 1 if (any_failed or benchmarks_failed) else 0


if __name__ == "__main__":
    raise SystemExit(main())
