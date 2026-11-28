#!/usr/bin/env python3
"""Validate and summarize Spellwake's test registry and Rules Engine benchmarks.

Enforces strict structural and honesty invariants on:
1. tests/registry.json and every tests/domains/<slug>/suite.json manifest.
2. Every declarative MTG interaction benchmark under
   tests/domains/rules-engine/benchmarks/**/*.benchmark.json.

Honesty invariants:
- Preemptive benchmarks with execution_status='pending-engine' are validated
  structurally and counted as pending, never as passing.
- A benchmark cannot declare execution_status='passing' without an explicit
  execution_receipt naming the command, timestamp, and engine target.
- Any validation error or execution_status='failing' produces a red badge and
  exits non-zero.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
REGISTRY_PATH = ROOT / "tests" / "registry.json"
BENCHMARKS_ROOT = ROOT / "tests" / "domains" / "rules-engine" / "benchmarks"

sys.path.insert(0, str(Path(__file__).resolve().parent))
import badge_svg  # noqa: E402

SLUG_RE = re.compile(r"^[a-z0-9-]+$")
BENCHMARK_ID_RE = re.compile(r"^[a-z0-9-]+/[a-z0-9-]+$")
CAPABILITY_RE = re.compile(r"^[a-z0-9-]+\.[a-z0-9-]+$")
RULE_RE = re.compile(r"^[0-9]{3}(\.[0-9]+[a-z]?)?$")
UUID_RE = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
)
ALIAS_RE = re.compile(r"^[a-z0-9_-]+$")

VALID_EXECUTION_STATUSES = {
    "pending-engine",
    "ready",
    "passing",
    "failing",
    "blocked",
}
VALID_REVIEW_STATUSES = {
    "draft",
    "pending-independent-review",
    "reviewed",
}
VALID_ZONES = {
    "LIBRARY",
    "HAND",
    "BATTLEFIELD",
    "GRAVEYARD",
    "STACK",
    "EXILE",
    "COMMAND",
}
VALID_ACTION_KINDS = {
    "play-land",
    "cast-spell",
    "attempt-illegal-cast",
    "resolve-top",
    "resolve-all",
    "advance-phase",
    "advance-to-next-turn",
    "activate-ability",
    "attempt-illegal-activation",
    "special-action",
    "declare-attackers",
    "declare-blockers",
    "assign-combat-damage",
    "choose-replacement-order",
    "pass-priority",
    "check-sba",
}
VALID_ASSERTION_KINDS = {
    "object-zone",
    "object-types",
    "object-subtypes",
    "object-stats",
    "object-counters",
    "object-abilities-present",
    "object-abilities-absent",
    "object-chapter-abilities",
    "stack-contains",
    "stack-order",
    "stack-empty",
    "trigger-count",
    "trigger-not-fired",
    "action-rejected",
    "choice-accepted",
    "damage-assigned",
    "player-life",
    "player-alive",
    "token-count",
    "token-stats",
    "sba-not-sacrificed",
    "sba-sacrificed",
}
VALID_SUITE_CATEGORIES = {"core-engine", "service-domain", "client-ui"}
VALID_READINESS_STAGES = {
    "active",
    "hold-for-rules-engine",
    "automated-daily-polling",
}
VALID_CHECK_KINDS = {"cargo", "bun", "python", "playwright"}


def _load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def validate_registry(
    registry_path: Path = REGISTRY_PATH,
    repo_root: Path = ROOT,
) -> list[str]:
    """Validate tests/registry.json and every domain suite.json it references."""
    errors: list[str] = []
    if not registry_path.is_file():
        return [f"Missing test registry: {registry_path}"]

    try:
        registry = _load_json(registry_path)
    except json.JSONDecodeError as exc:
        return [f"Invalid JSON in {registry_path}: {exc}"]

    if not isinstance(registry, dict) or registry.get("schemaVersion") != 1:
        errors.append("tests/registry.json must be an object with schemaVersion == 1")
        return errors

    domains = registry.get("domains")
    if not isinstance(domains, list) or not domains:
        errors.append("tests/registry.json must contain a non-empty 'domains' list")
        return errors

    seen_slugs: set[str] = set()
    for index, entry in enumerate(domains):
        if not isinstance(entry, dict):
            errors.append(f"registry.domains[{index}] must be an object")
            continue
        slug = entry.get("slug")
        suite_rel = entry.get("suitePath")
        if not isinstance(slug, str) or not SLUG_RE.match(slug):
            errors.append(f"registry.domains[{index}].slug is invalid: {slug!r}")
            continue
        if slug in seen_slugs:
            errors.append(f"Duplicate domain slug in registry: {slug}")
            continue
        seen_slugs.add(slug)

        if not isinstance(suite_rel, str) or not suite_rel:
            errors.append(f"Domain '{slug}' missing suitePath")
            continue
        suite_path = repo_root / suite_rel
        if not suite_path.is_file():
            errors.append(f"Domain '{slug}' suite manifest not found: {suite_rel}")
            continue
        errors.extend(_validate_suite_manifest(suite_path, expected_slug=slug))

    # Ensure no orphan domain suite directories exist outside registry.json.
    domains_dir = repo_root / "tests" / "domains"
    if domains_dir.is_dir():
        for child in sorted(domains_dir.iterdir()):
            if child.is_dir() and (child / "suite.json").is_file():
                if child.name not in seen_slugs:
                    errors.append(
                        f"Domain suite '{child.name}' exists under tests/domains/ but is not registered in tests/registry.json"
                    )

    return errors


def _validate_suite_manifest(path: Path, *, expected_slug: str) -> list[str]:
    errors: list[str] = []
    rel = path.as_posix()
    try:
        suite = _load_json(path)
    except json.JSONDecodeError as exc:
        return [f"{rel}: invalid JSON: {exc}"]

    if not isinstance(suite, dict):
        return [f"{rel}: manifest must be a JSON object"]

    if suite.get("schemaVersion") != 1:
        errors.append(f"{rel}: schemaVersion must be 1")
    if suite.get("slug") != expected_slug:
        errors.append(
            f"{rel}: slug {suite.get('slug')!r} does not match registry slug {expected_slug!r}"
        )
    if not isinstance(suite.get("label"), str) or not suite["label"].strip():
        errors.append(f"{rel}: label must be a non-empty string")
    if suite.get("category") not in VALID_SUITE_CATEGORIES:
        errors.append(
            f"{rel}: category must be one of {sorted(VALID_SUITE_CATEGORIES)}"
        )
    if suite.get("readinessStage") not in VALID_READINESS_STAGES:
        errors.append(
            f"{rel}: readinessStage must be one of {sorted(VALID_READINESS_STAGES)}"
        )
    if not isinstance(suite.get("localOnly"), bool):
        errors.append(f"{rel}: localOnly must be a boolean")

    subdomains = suite.get("subdomains", [])
    if not isinstance(subdomains, list):
        errors.append(f"{rel}: subdomains must be a list when present")
    else:
        seen_sub_slugs: set[str] = set()
        for idx, sub in enumerate(subdomains):
            if not isinstance(sub, dict):
                errors.append(f"{rel}: subdomains[{idx}] must be an object")
                continue
            sub_slug = sub.get("slug")
            if not isinstance(sub_slug, str) or not SLUG_RE.match(sub_slug):
                errors.append(f"{rel}: subdomains[{idx}].slug is invalid: {sub_slug!r}")
            elif sub_slug in seen_sub_slugs:
                errors.append(f"{rel}: duplicate subdomain slug {sub_slug!r}")
            else:
                seen_sub_slugs.add(sub_slug)
            if not isinstance(sub.get("label"), str) or not sub["label"].strip():
                errors.append(f"{rel}: subdomains[{idx}].label must be non-empty")
            if not isinstance(sub.get("description"), str) or not sub["description"].strip():
                errors.append(f"{rel}: subdomains[{idx}].description must be non-empty")
            unqual = sub.get("unqualifiedReason")
            if unqual is not None and (not isinstance(unqual, str) or not unqual.strip()):
                errors.append(f"{rel}: subdomains[{idx}].unqualifiedReason must be null or a non-empty string")

    checks = suite.get("checks")
    if not isinstance(checks, dict):
        errors.append(f"{rel}: checks must be an object")
        return errors

    for bucket in ("rust", "frontend", "browserE2e", "python"):
        items = checks.get(bucket, [])
        if not isinstance(items, list):
            errors.append(f"{rel}: checks.{bucket} must be a list")
            continue
        for i, cmd in enumerate(items):
            if not isinstance(cmd, dict):
                errors.append(f"{rel}: checks.{bucket}[{i}] must be an object")
                continue
            if cmd.get("kind") not in VALID_CHECK_KINDS:
                errors.append(
                    f"{rel}: checks.{bucket}[{i}].kind must be one of {sorted(VALID_CHECK_KINDS)}"
                )
            if not isinstance(cmd.get("label"), str) or not cmd["label"].strip():
                errors.append(f"{rel}: checks.{bucket}[{i}].label must be non-empty")
            args = cmd.get("args")
            if not isinstance(args, list) or not args or not all(isinstance(a, str) for a in args):
                errors.append(
                    f"{rel}: checks.{bucket}[{i}].args must be a non-empty list of strings"
                )

    db_filters = checks.get("postgresDbFilters", [])
    if not isinstance(db_filters, list) or not all(
        isinstance(f, str) and f.strip() for f in db_filters
    ):
        errors.append(f"{rel}: checks.postgresDbFilters must be a list of non-empty strings")

    return errors


def validate_benchmark_spec(
    data: Any,
    *,
    path: Path,
    benchmarks_root: Path = BENCHMARKS_ROOT,
) -> list[str]:
    """Validate one rules-engine benchmark JSON payload against the v1 contract."""
    errors: list[str] = []
    label = path.as_posix()

    if not isinstance(data, dict):
        return [f"{label}: top-level document must be a JSON object"]

    if data.get("schema_version") != 1:
        errors.append(f"{label}: schema_version must be 1")

    expected_category = path.parent.name
    expected_stem = path.name.removesuffix(".benchmark.json")
    expected_id = f"{expected_category}/{expected_stem}"

    bench_id = data.get("id")
    category = data.get("category")
    if not isinstance(category, str) or not SLUG_RE.match(category):
        errors.append(f"{label}: category must be kebab-case, got {category!r}")
    elif path.parent != benchmarks_root and category != expected_category:
        errors.append(
            f"{label}: category {category!r} must match parent directory {expected_category!r}"
        )

    if not isinstance(bench_id, str) or not BENCHMARK_ID_RE.match(bench_id):
        errors.append(f"{label}: id must match '<category>/<slug>', got {bench_id!r}")
    elif path.parent != benchmarks_root and bench_id != expected_id:
        errors.append(
            f"{label}: id {bench_id!r} must match relative path '{expected_id}'"
        )

    if not isinstance(data.get("title"), str) or len(data["title"].strip()) < 4:
        errors.append(f"{label}: title must be a string with at least 4 characters")

    exec_status = data.get("execution_status")
    if exec_status not in VALID_EXECUTION_STATUSES:
        errors.append(
            f"{label}: execution_status must be one of {sorted(VALID_EXECUTION_STATUSES)}"
        )

    receipt = data.get("execution_receipt")
    if exec_status == "passing":
        if not isinstance(receipt, dict):
            errors.append(
                f"{label}: execution_status='passing' requires a non-null execution_receipt"
            )
        else:
            for req in ("command", "verified_at", "engine_target"):
                if not isinstance(receipt.get(req), str) or not receipt[req].strip():
                    errors.append(
                        f"{label}: execution_receipt.{req} must be a non-empty string"
                    )
    elif exec_status == "pending-engine" and receipt is not None:
        errors.append(
            f"{label}: execution_status='pending-engine' must have execution_receipt=null"
        )

    if data.get("review_status") not in VALID_REVIEW_STATUSES:
        errors.append(
            f"{label}: review_status must be one of {sorted(VALID_REVIEW_STATUSES)}"
        )

    rules_ed = data.get("rules_edition")
    if not isinstance(rules_ed, dict):
        errors.append(f"{label}: rules_edition must be an object")
    else:
        if not isinstance(rules_ed.get("edition"), str) or len(rules_ed["edition"].strip()) < 4:
            errors.append(f"{label}: rules_edition.edition must be non-empty")
        if not isinstance(rules_ed.get("source_url"), str) or len(rules_ed["source_url"].strip()) < 8:
            errors.append(f"{label}: rules_edition.source_url must be non-empty")
        cited = rules_ed.get("cited_rules")
        if not isinstance(cited, list) or not cited:
            errors.append(f"{label}: rules_edition.cited_rules must be a non-empty list")
        else:
            for i, entry in enumerate(cited):
                if not isinstance(entry, dict):
                    errors.append(f"{label}: cited_rules[{i}] must be an object")
                    continue
                if not isinstance(entry.get("rule"), str) or not RULE_RE.match(entry["rule"]):
                    errors.append(f"{label}: cited_rules[{i}].rule is invalid: {entry.get('rule')!r}")
                if not isinstance(entry.get("summary"), str) or len(entry["summary"].strip()) < 5:
                    errors.append(f"{label}: cited_rules[{i}].summary must be non-empty")

    provenance = data.get("provenance")
    if not isinstance(provenance, dict):
        errors.append(f"{label}: provenance must be an object")
    else:
        for req in ("source_summary", "scope_notes"):
            if not isinstance(provenance.get(req), str) or len(provenance[req].strip()) < 5:
                errors.append(f"{label}: provenance.{req} must be non-empty")

    caps = data.get("required_capabilities")
    if not isinstance(caps, list) or not caps:
        errors.append(f"{label}: required_capabilities must be a non-empty list")
    else:
        if len(set(caps)) != len(caps):
            errors.append(f"{label}: required_capabilities contains duplicates")
        if caps != sorted(caps):
            errors.append(f"{label}: required_capabilities must be sorted deterministically")
        for cap in caps:
            if not isinstance(cap, str) or not CAPABILITY_RE.match(cap):
                errors.append(
                    f"{label}: invalid capability key {cap!r} (expected '<subsystem>.<capability>')"
                )

    cards = data.get("cards")
    card_aliases: set[str] = set()
    if not isinstance(cards, dict) or not cards:
        errors.append(f"{label}: cards must be a non-empty object")
    else:
        for alias, card in cards.items():
            if not isinstance(alias, str) or not ALIAS_RE.match(alias):
                errors.append(f"{label}: invalid card alias {alias!r}")
                continue
            card_aliases.add(alias)
            if not isinstance(card, dict):
                errors.append(f"{label}: cards.{alias} must be an object")
                continue
            if not isinstance(card.get("name"), str) or not card["name"].strip():
                errors.append(f"{label}: cards.{alias}.name must be non-empty")
            oid = card.get("oracle_id")
            if oid is not None and (not isinstance(oid, str) or not UUID_RE.match(oid)):
                errors.append(f"{label}: cards.{alias}.oracle_id must be a valid UUID or null")
            for req in ("mana_cost", "type_line", "oracle_text"):
                if not isinstance(card.get(req), str):
                    errors.append(f"{label}: cards.{alias}.{req} must be a string")

    cases = data.get("cases")
    if not isinstance(cases, list) or not cases:
        errors.append(f"{label}: cases must be a non-empty list")
        return errors

    seen_case_ids: set[str] = set()
    for idx, case in enumerate(cases):
        prefix = f"{label}: cases[{idx}]"
        if not isinstance(case, dict):
            errors.append(f"{prefix} must be an object")
            continue
        case_id = case.get("id")
        if not isinstance(case_id, str) or not SLUG_RE.match(case_id):
            errors.append(f"{prefix}.id must be kebab-case")
        elif case_id in seen_case_ids:
            errors.append(f"{prefix}.id duplicate: {case_id}")
        else:
            seen_case_ids.add(case_id)

        for req in ("name", "summary"):
            if not isinstance(case.get(req), str) or len(case[req].strip()) < 4:
                errors.append(f"{prefix}.{req} must be a non-empty string")

        case_rules = case.get("rules")
        if not isinstance(case_rules, list) or not case_rules:
            errors.append(f"{prefix}.rules must be a non-empty list")
        else:
            for r in case_rules:
                if not isinstance(r, str) or not RULE_RE.match(r):
                    errors.append(f"{prefix}.rules contains invalid rule citation {r!r}")

        init = case.get("initial_state")
        player_ids: set[str] = set()
        if not isinstance(init, dict):
            errors.append(f"{prefix}.initial_state must be an object")
        else:
            players = init.get("players")
            if not isinstance(players, list) or len(players) < 2:
                errors.append(f"{prefix}.initial_state.players must list at least 2 players")
            else:
                for p in players:
                    if not isinstance(p, dict) or not isinstance(p.get("id"), str) or not p["id"]:
                        errors.append(f"{prefix}.initial_state.players contains invalid player")
                    else:
                        player_ids.add(p["id"])
                    if isinstance(p, dict) and (isinstance(p.get("life"), bool) or not isinstance(p.get("life"), int)):
                        errors.append(f"{prefix}.initial_state.players life must be an integer")

            if init.get("active_player") not in player_ids:
                errors.append(f"{prefix}.initial_state.active_player must reference a declared player")
            turn_num = init.get("turn_number")
            if isinstance(turn_num, bool) or not isinstance(turn_num, int) or turn_num < 1:
                errors.append(f"{prefix}.initial_state.turn_number must be an integer >= 1")
            if not isinstance(init.get("phase"), str) or not init["phase"].strip():
                errors.append(f"{prefix}.initial_state.phase must be non-empty")

            objs = init.get("objects")
            if not isinstance(objs, list) or not objs:
                errors.append(f"{prefix}.initial_state.objects must be a non-empty list")
            else:
                seen_obj_aliases: set[str] = set()
                for o_idx, obj in enumerate(objs):
                    if not isinstance(obj, dict):
                        errors.append(f"{prefix}.initial_state.objects[{o_idx}] must be an object")
                        continue
                    o_alias = obj.get("alias")
                    if not isinstance(o_alias, str) or not ALIAS_RE.match(o_alias):
                        errors.append(f"{prefix}.initial_state.objects[{o_idx}].alias is invalid")
                    elif o_alias in seen_obj_aliases:
                        errors.append(f"{prefix}.initial_state.objects duplicate alias {o_alias!r}")
                    else:
                        seen_obj_aliases.add(o_alias)
                    if obj.get("card") not in card_aliases:
                        errors.append(
                            f"{prefix}.initial_state.objects[{o_idx}].card {obj.get('card')!r} not in cards"
                        )
                    if obj.get("owner") not in player_ids:
                        errors.append(f"{prefix}.initial_state.objects[{o_idx}].owner not in players")
                    if obj.get("controller") not in player_ids:
                        errors.append(f"{prefix}.initial_state.objects[{o_idx}].controller not in players")
                    if obj.get("zone") not in VALID_ZONES:
                        errors.append(f"{prefix}.initial_state.objects[{o_idx}].zone is invalid")

        steps = case.get("steps")
        if not isinstance(steps, list) or not steps:
            errors.append(f"{prefix}.steps must be a non-empty list")
        else:
            checkpoint_ids: set[str] = set()
            for s_idx, step in enumerate(steps, start=1):
                if not isinstance(step, dict):
                    errors.append(f"{prefix}.steps[{s_idx - 1}] must be an object")
                    continue
                if step.get("step") != s_idx:
                    errors.append(
                        f"{prefix}.steps[{s_idx - 1}].step must be sequential ({s_idx}), got {step.get('step')!r}"
                    )
                action = step.get("action")
                if not isinstance(action, dict):
                    errors.append(f"{prefix}.steps[{s_idx - 1}].action must be an object")
                else:
                    if action.get("kind") not in VALID_ACTION_KINDS:
                        errors.append(
                            f"{prefix}.steps[{s_idx - 1}].action.kind is invalid: {action.get('kind')!r}"
                        )
                    actor = action.get("actor")
                    if actor != "game" and actor not in player_ids:
                        errors.append(
                            f"{prefix}.steps[{s_idx - 1}].action.actor {actor!r} must be 'game' or a declared player"
                        )
                    if not isinstance(action.get("description"), str) or len(action["description"].strip()) < 4:
                        errors.append(f"{prefix}.steps[{s_idx - 1}].action.description must be non-empty")

                checkpoint = step.get("checkpoint")
                if checkpoint is not None:
                    if not isinstance(checkpoint, dict):
                        errors.append(f"{prefix}.steps[{s_idx - 1}].checkpoint must be an object")
                    else:
                        cp_id = checkpoint.get("id")
                        if not isinstance(cp_id, str) or not SLUG_RE.match(cp_id):
                            errors.append(f"{prefix}.steps[{s_idx - 1}].checkpoint.id must be kebab-case")
                        elif cp_id in checkpoint_ids:
                            errors.append(f"{prefix}: duplicate checkpoint id {cp_id!r}")
                        else:
                            checkpoint_ids.add(cp_id)
                        assertions = checkpoint.get("assertions")
                        if not isinstance(assertions, list) or not assertions:
                            errors.append(f"{prefix}.steps[{s_idx - 1}].checkpoint.assertions must be non-empty")
                        else:
                            for a_idx, assertion in enumerate(assertions):
                                if not isinstance(assertion, dict) or assertion.get("kind") not in VALID_ASSERTION_KINDS:
                                    errors.append(
                                        f"{prefix}.steps[{s_idx - 1}].checkpoint.assertions[{a_idx}].kind is invalid"
                                    )
            if not checkpoint_ids:
                errors.append(f"{prefix}: every case must define at least one checkpoint")

        invariants = case.get("invariants")
        if not isinstance(invariants, list) or not invariants:
            errors.append(f"{prefix}.invariants must be a non-empty list")
        else:
            seen_inv_ids: set[str] = set()
            for inv_idx, inv in enumerate(invariants):
                if not isinstance(inv, dict):
                    errors.append(f"{prefix}.invariants[{inv_idx}] must be an object")
                    continue
                inv_id = inv.get("id")
                if not isinstance(inv_id, str) or not SLUG_RE.match(inv_id):
                    errors.append(f"{prefix}.invariants[{inv_idx}].id must be kebab-case")
                elif inv_id in seen_inv_ids:
                    errors.append(f"{prefix}.invariants duplicate id {inv_id!r}")
                else:
                    seen_inv_ids.add(inv_id)
                inv_rules = inv.get("rules")
                if not isinstance(inv_rules, list) or not inv_rules or not all(
                    isinstance(r, str) and RULE_RE.match(r) for r in inv_rules
                ):
                    errors.append(f"{prefix}.invariants[{inv_idx}].rules must list valid CR numbers")
                if not isinstance(inv.get("description"), str) or len(inv["description"].strip()) < 8:
                    errors.append(f"{prefix}.invariants[{inv_idx}].description must be non-empty")

    return errors


def discover_benchmark_files(
    benchmarks_root: Path = BENCHMARKS_ROOT,
    *,
    category: str | None = None,
) -> list[Path]:
    """Return sorted list of all *.benchmark.json files under benchmarks_root."""
    if not benchmarks_root.is_dir():
        return []
    files = sorted(benchmarks_root.rglob("*.benchmark.json"))
    if category is not None:
        files = [f for f in files if f.parent.name == category]
    return files


def evaluate_benchmarks(
    *,
    benchmarks_root: Path = BENCHMARKS_ROOT,
    registry_path: Path = REGISTRY_PATH,
    repo_root: Path = ROOT,
    category: str | None = None,
    check_registry: bool = True,
) -> dict[str, Any]:
    """Validate registry + benchmark specs and compute an honest scorecard."""
    validation_errors: list[str] = []
    if check_registry:
        validation_errors.extend(validate_registry(registry_path=registry_path, repo_root=repo_root))

    files = discover_benchmark_files(benchmarks_root, category=category)
    entries: list[dict[str, Any]] = []
    seen_ids: set[str] = set()
    status_counts: Counter[str] = Counter()
    case_status_counts: Counter[str] = Counter()
    category_counts: Counter[str] = Counter()
    missing_capabilities: Counter[str] = Counter()

    for path in files:
        try:
            data = _load_json(path)
        except json.JSONDecodeError as exc:
            validation_errors.append(f"{path.as_posix()}: invalid JSON: {exc}")
            continue

        spec_errors = validate_benchmark_spec(data, path=path, benchmarks_root=benchmarks_root)
        validation_errors.extend(spec_errors)

        bench_id = data.get("id", path.name)
        if isinstance(bench_id, str):
            if bench_id in seen_ids:
                validation_errors.append(f"Duplicate benchmark id: {bench_id}")
            seen_ids.add(bench_id)

        exec_status = data.get("execution_status", "blocked")
        case_list = data.get("cases", []) if isinstance(data.get("cases"), list) else []
        case_count = len(case_list)
        cat = data.get("category", path.parent.name)
        caps = data.get("required_capabilities", []) if isinstance(data.get("required_capabilities"), list) else []

        status_counts[exec_status] += 1
        case_status_counts[exec_status] += case_count
        category_counts[cat] += 1
        if exec_status != "passing":
            for cap in caps:
                if isinstance(cap, str):
                    missing_capabilities[cap] += 1

        try:
            rel_path = path.relative_to(repo_root).as_posix()
        except ValueError:
            rel_path = path.as_posix()

        entries.append(
            {
                "id": bench_id,
                "title": data.get("title", ""),
                "category": cat,
                "path": rel_path,
                "executionStatus": exec_status,
                "reviewStatus": data.get("review_status", "draft"),
                "caseCount": case_count,
                "cases": [c.get("id", "") for c in case_list if isinstance(c, dict)],
                "requiredCapabilities": caps,
                "valid": len(spec_errors) == 0,
                "errors": spec_errors,
            }
        )

    total_benchmarks = len(entries)
    total_cases = sum(e["caseCount"] for e in entries)
    passing_cases = case_status_counts["passing"]
    failing_cases = case_status_counts["failing"]
    pending_cases = case_status_counts["pending-engine"] + case_status_counts["ready"] + case_status_counts["blocked"]

    if validation_errors:
        badge_status = "failing"
        badge_msg = f"invalid · {len(validation_errors)} errors"
        badge_color = "red"
    elif status_counts["failing"] > 0:
        badge_status = "failing"
        badge_msg = f"fail · {failing_cases}/{total_cases} cases"
        badge_color = "red"
    elif total_cases == 0:
        badge_status = "unknown"
        badge_msg = "0 specs"
        badge_color = "lightgrey"
    elif passing_cases == total_cases:
        badge_status = "passing"
        badge_msg = f"pass · {passing_cases}/{total_cases} cases"
        badge_color = "green"
    elif passing_cases > 0:
        badge_status = "partial"
        badge_msg = f"{passing_cases}/{total_cases} pass · {pending_cases} pending"
        badge_color = "yellow"
    else:
        badge_status = "pending-engine"
        badge_msg = f"0/{total_cases} pass · {pending_cases} pending"
        badge_color = "lightgrey"

    return {
        "schemaVersion": 1,
        "generatedAt": datetime.now(timezone.utc).isoformat(),
        "validationPassed": len(validation_errors) == 0,
        "validationErrors": validation_errors,
        "status": badge_status,
        "badge": {
            "label": "Rules Benchmarks",
            "message": badge_msg,
            "color": badge_color,
            "isError": badge_status == "failing",
            "ariaLabel": f"Rules Benchmarks: {badge_msg}",
        },
        "summary": {
            "totalBenchmarks": total_benchmarks,
            "totalCases": total_cases,
            "passingBenchmarks": status_counts["passing"],
            "failingBenchmarks": status_counts["failing"],
            "pendingEngineBenchmarks": status_counts["pending-engine"],
            "readyBenchmarks": status_counts["ready"],
            "blockedBenchmarks": status_counts["blocked"],
            "passingCases": passing_cases,
            "failingCases": failing_cases,
            "pendingCases": pending_cases,
            "categories": dict(sorted(category_counts.items())),
            "missingCapabilities": dict(sorted(missing_capabilities.items())),
        },
        "benchmarks": entries,
    }


def write_benchmark_badge(report: dict[str, Any], badges_dir: Path) -> dict[str, Any]:
    """Write rules-engine-benchmarks.svg and rules-engine-benchmarks.json."""
    b = report["badge"]
    endpoint = badge_svg.write_badge(
        badges_dir,
        "rules-engine-benchmarks",
        b["label"],
        b["message"],
        b["color"],
        is_error=bool(b["isError"]),
        aria_label=b.get("ariaLabel"),
    )
    badge_svg.write_atomic_json(badges_dir / "rules-engine-benchmarks.json", endpoint)
    return endpoint


def _print_table(report: dict[str, Any]) -> None:
    s = report["summary"]
    print(
        f"Rules Engine Benchmarks: {s['totalBenchmarks']} specs, {s['totalCases']} cases "
        f"({s['passingCases']} pass, {s['failingCases']} fail, {s['pendingCases']} pending)"
    )
    print(f"Registry & schema validation: {'PASS' if report['validationPassed'] else 'FAIL'}")
    print("")
    for entry in report["benchmarks"]:
        print(
            f"- [{entry['executionStatus']}] {entry['id']} "
            f"({entry['caseCount']} cases, review={entry['reviewStatus']})"
        )
        print(f"  Title: {entry['title']}")
        print(f"  Capabilities: {', '.join(entry['requiredCapabilities'])}")
    if report["validationErrors"]:
        print("\nValidation errors:")
        for err in report["validationErrors"]:
            print(f"  - {err}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--category",
        default=None,
        help="Filter benchmarks to a single category folder.",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="Print a concise human-readable benchmark summary table instead of JSON.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="Optional path to write the benchmark evaluation JSON report.",
    )
    parser.add_argument(
        "--badges-dir",
        type=Path,
        default=None,
        help="Optional directory to write rules-engine-benchmarks.svg and .json.",
    )
    args = parser.parse_args()

    report = evaluate_benchmarks(category=args.category)
    if args.output is not None:
        badge_svg.write_atomic_json(args.output, report)
    if args.badges_dir is not None:
        write_benchmark_badge(report, args.badges_dir)

    if args.list:
        _print_table(report)
    else:
        print(json.dumps(report, indent=2))

    return 0 if report["status"] != "failing" else 1


if __name__ == "__main__":
    raise SystemExit(main())
