"""Compare actual restricted native checkpoints with pinned Forge/XMage traces and reviewed source-grounded expectations.

This is a post-cast projection comparison, not complete-card/full-game qualification
or a performance benchmark. Run the Rust binary separately. XML/log hashes are local
evidence provenance, not authentication of reports supplied by an untrusted party.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys

ENGINES = {
    "forge": {"commit": "3ead7e68d54bf95c6f778fd6aa7c604c036e0736", "checkout": "forge", "module": "forge-gui-desktop", "package": "forge/ai", "mana_auto_payment": False},
    "xmage": {"commit": "81c2f6d806dbec8eb79ad2a511cb686dff8361e7", "checkout": "mage", "module": "Mage.Tests", "package": "org/mage/test/reference", "mana_auto_payment": True},
}
STAGES = {
    "bolt-control": ["bolt-on-stack", "after-bolt"],
    "target-bounce": ["bolt-on-stack", "response-on-stack", "after-response", "after-bolt"],
    "counter-bolt": ["bolt-on-stack", "counter-on-stack", "after-counter"],
    "counter-counter": ["bolt-on-stack", "counter-on-stack", "rescue-on-stack", "after-rescue", "after-bolt"],
}
ALIASES = {"bolt-control": {"bolt", "lion"}, "target-bounce": {"bolt", "bounce", "lion"}, "counter-bolt": {"bolt", "counter", "lion"}, "counter-counter": {"bolt", "counter", "rescue", "lion"}}
RUNS = [
    {"engine": "xmage", "manifest": "run-manifest.json", "class": "TargetBounceReferenceTest", "folder": "target-bounce", "tests": ["boltTargetLeavesBeforeResolution", "boltAloneKillsLion"], "scenarios": ["bolt-control", "target-bounce"]},
    {"engine": "xmage", "manifest": "counter-run-manifest.json", "class": "CounterspellReferenceTest", "folder": "counterspell", "tests": ["counterBolt", "counterCounter"], "scenarios": ["counter-bolt", "counter-counter"]},
    {"engine": "forge", "manifest": "instant-run-manifest.json", "class": "InstantInteractionReferenceTest", "folder": "forge-interactions", "tests": ["boltControl", "targetBounce", "counterBolt", "counterCounter"], "scenarios": list(STAGES), "extra_classes": ["$StrictController"]},
]


COPY_STAGES = {
    "copy-redirect": ["bolt-on-stack", "twincast-on-stack", "after-copy", "after-copy-resolution", "finished"],
    "counter-original": ["bolt-on-stack", "twincast-on-stack", "after-copy", "counter-on-stack", "after-counter", "finished"],
    "counter-copy": ["bolt-on-stack", "twincast-on-stack", "after-copy", "counter-on-stack", "after-counter", "finished"],
    "copy-retain-missing-target": ["bolt-on-stack", "twincast-on-stack", "bounce-on-stack", "after-bounce", "after-copy", "after-copy-resolution", "finished"],
    "copy-retarget-missing-target": ["bolt-on-stack", "twincast-on-stack", "bounce-on-stack", "after-bounce", "after-copy", "after-copy-resolution", "finished"],
    "copy-target-departs": ["bolt-on-stack", "twincast-on-stack", "after-copy", "bounce-on-stack", "after-bounce", "after-copy-resolution", "finished"],
}
COPY_ALIASES = {name: {"bolt", "twincast", "counter", "bounce", "lion1", "lion2"} for name in COPY_STAGES}
COPY_RUNS = [
    {"engine": "forge", "manifest": "spell-copy-run-manifest.json", "class": "SpellCopyReferenceTest", "folder": "forge-spell-copies", "tests": ["copyRedirect", "counterOriginal", "counterCopy", "copyRetainMissingTarget", "copyRetargetMissingTarget", "copyTargetDeparts"], "scenarios": list(COPY_STAGES), "extra_classes": ["$StrictController"], "include_staged_source": True, "manifest_schema": 2},
    {"engine": "xmage", "manifest": "spell-copy-run-manifest.json", "class": "SpellCopyReferenceTest", "folder": "spell-copy-xmage", "tests": ["copyRedirect", "counterOriginal", "counterCopy", "copyRetainMissingTarget", "copyRetargetMissingTarget", "copyTargetDeparts"], "scenarios": list(COPY_STAGES)},
]


def number(value: object) -> str | None:
    """Preserve arbitrary precision and reject bool/float/noncanonical encodings."""
    if value is None:
        return None
    if type(value) is int:
        return str(value)
    if isinstance(value, str) and re.fullmatch(r"0|-?[1-9][0-9]*", value):
        return value
    raise ValueError("noncanonical rules quantity")


def normalize(observation: dict, aliases: set[str], copy_aware: bool = False) -> dict:
    cards = observation["cards"]
    if len(cards) != len(aliases) or {card["alias"] for card in cards} != aliases:
        raise ValueError("missing, unknown or duplicate card alias")
    normalized = []
    for card in cards:
        if card["owner"] not in ("A", "B") or card["zone"] not in ("HAND", "STACK", "BATTLEFIELD", "GRAVEYARD"):
            raise ValueError("unknown owner or zone")
        normalized.append({key: card[key] for key in ("alias", "owner", "zone")} | {key: number(card[key]) for key in ("damage", "power", "toughness")})
    stack = observation["stack_bottom_first"]
    if copy_aware:
        if len({entry["alias"] for entry in stack}) != len(stack):
            raise ValueError("duplicate stack object")
        for entry in stack:
            if set(entry) != {"alias", "kind", "owner", "controller", "targets"}:
                raise ValueError("unknown copy-aware stack projection")
            if entry["owner"] not in ("A", "B") or entry["controller"] not in ("A", "B"):
                raise ValueError("unknown spell owner/controller")
            if entry["kind"] == "spell-copy":
                if entry["alias"] != "bolt-copy": raise ValueError("unknown copy alias")
            elif entry["kind"] != "card-spell" or entry["alias"] not in aliases:
                raise ValueError("unknown physical spell alias/kind")
            if any(target not in aliases | {"bolt-copy"} for target in entry["targets"]):
                raise ValueError("unknown copy-aware target")
    else:
        if len({entry["card"] for entry in stack}) != len(stack):
            raise ValueError("duplicate physical spell on stack")
        for entry in stack:
            if set(entry) != {"card", "targets"} or entry["card"] not in aliases or any(target not in aliases for target in entry["targets"]):
                raise ValueError("unknown stack/target projection")
    return {"checkpoint": observation["checkpoint"], "stack_bottom_first": stack, "cards": sorted(normalized, key=lambda card: card["alias"])}


def compare(native: dict, references: dict[str, dict[str, dict]], expectations: dict, copy_aware: bool = False) -> dict:
    """Agreement needs both references and reviewed expectations; consensus is insufficient."""
    stages = COPY_STAGES if copy_aware else STAGES
    aliases = COPY_ALIASES if copy_aware else ALIASES
    if native.get("status") != "success":
        raise ValueError("native run did not succeed")
    data = native["data"]
    if type(data["schema_version"]) is not int or data["schema_version"] != (3 if copy_aware else 2) or data["kind"] != ("restricted-spell-copies" if copy_aware else "restricted-instant-interactions"):
        raise ValueError("unknown native trace contract")
    if data["registry_input"] not in (("embedded-five-card-test-subset", "saved-snapshot") if copy_aware else ("embedded-four-card-test-subset", "saved-snapshot")) or re.fullmatch(r"[0-9a-f]{64}", data["registry_source_sha256"]) is None:
        raise ValueError("unknown native registry provenance")
    if type(expectations["schema_version"]) is not int or expectations["schema_version"] != 1 or expectations["kind"] != "source-grounded-projection-expectations":
        raise ValueError("unknown expectation contract")
    review = expectations["review_status"]
    if review not in ("reviewed", "pending-independent-review", "unresolved"):
        raise ValueError("unknown expectation review status")
    cases = expectations["cases"]
    if len(cases) != len(stages) or {case["scenario"] for case in cases} != set(stages):
        raise ValueError("missing, duplicate or unknown expectation case")
    expected_cases = {case["scenario"]: case for case in cases}
    scenarios = data["scenarios"]
    if len(scenarios) != len(stages) or {item["scenario"] for item in scenarios} != set(stages) or set(references) != set(ENGINES):
        raise ValueError("missing, duplicate or unknown scenario/reference")
    if any(set(reference) != set(stages) for reference in references.values()):
        raise ValueError("missing reference scenario")
    if data["registry_input"] == "saved-snapshot" and data["registry_source_sha256"] != expectations["oracle_snapshot_sha256"]:
        raise ValueError("native snapshot differs from frozen expectation input")
    results = []
    for scenario in scenarios:
        name = scenario["scenario"]
        case = expected_cases[name]
        if not case["rules"] or not case["reasoning"].strip():
            raise ValueError("uncited or unexplained expectation")
        observations = {"native": scenario["observations"], "expected": case["observations"]}
        for engine, config in ENGINES.items():
            reference = references[engine][name]
            if (type(reference["schema_version"]) is not int or reference["schema_version"] != (2 if copy_aware else 1) or reference["scenario"] != name
                    or reference["reference"] != engine or reference["commit"] != config["commit"]
                    or reference["strict_choices"] is not True or reference["mana_auto_payment"] is not config["mana_auto_payment"]):
                raise ValueError("reference identity/choice policy mismatch")
            if engine == "forge" and (reference["direct_stack_admission"] is not True or reference["casting_scope"] != "direct-stack-admission-no-payment"):
                raise ValueError("unknown Forge admission scope")
            observations[engine] = reference["observations"]
        if any([point["checkpoint"] for point in points] != stages[name] for points in observations.values()):
            raise ValueError("missing, duplicate or out-of-order checkpoint")
        checkpoints = []
        for index, stage in enumerate(stages[name]):
            views = {key: normalize(points[index], aliases[name], copy_aware) for key, points in observations.items()}
            equal = all(view == views["expected"] for view in views.values())
            status = ("agreed" if review == "reviewed" else "unreviewed" if review == "pending-independent-review" else "unresolved") if equal else "disagreed"
            checkpoints.append({"checkpoint": stage, "status": status, **views})
        statuses = {point["status"] for point in checkpoints}
        status = "disagreed" if "disagreed" in statuses else "agreed" if statuses == {"agreed"} else "unreviewed" if "unreviewed" in statuses else "unresolved"
        results.append({"scenario": name, "status": status, "rules": case["rules"], "reasoning": case["reasoning"], "checkpoints": checkpoints})
    reference_agreements = {}
    for engine in ENGINES:
        reference_agreements[engine] = {
            "scenariosMatchingNative": sum(all(point["native"] == point[engine] for point in case["checkpoints"]) for case in results),
            "checkpointsMatchingNative": sum(point["native"] == point[engine] for case in results for point in case["checkpoints"]),
        }
    return {"kind": "restricted-source-grounded-differential", "primaryReference": "forge", "crossCheck": "xmage",
            "referenceCommits": {engine: config["commit"] for engine, config in ENGINES.items()},
            "expectationReview": review,
            "comparedFields": [("ordered spell aliases/kinds/owners/controllers/targets" if copy_aware else "ordered stack card aliases/targets"), "card owner/zone", "battlefield power/toughness/marked damage"],
            "excluded": ["casting/admission equivalence", "mana production/payment equivalence", "turn/phase traversal", "priority callback schedule", "life/player targets", "unaliased lands/decks/cards", "engine-internal object IDs", "general card semantics", "performance", *( ["pre-SBA copy exit equivalence", "permanent/ability copies", "production resumable choice API"] if copy_aware else [])],
            "scenariosAgreed": sum(item["status"] == "agreed" for item in results),
            "scenariosDisagreed": sum(item["status"] == "disagreed" for item in results),
            "scenariosAwaitingReview": sum(item["status"] in ("unreviewed", "unresolved") for item in results),
            "checkpointsAgreed": sum(point["status"] == "agreed" for item in results for point in item["checkpoints"]),
            "referenceAgreements": reference_agreements,
            "fullGameComparisons": 0, "fullyQualifiedCards": 0, "performanceBenchmarks": 0,
            "nativeRegistry": {"input": data["registry_input"], "sourceSha256": data["registry_source_sha256"]}, "results": results}


def verify_manifest_contract(manifest: dict, run: dict, config: dict) -> tuple[str, set[str]]:
    """Require exactly the expected run identity and every provenance artifact."""
    engine = run["engine"]
    if "manifest_schema" in run and (type(manifest.get("schema_version")) is not int or manifest["schema_version"] != run["manifest_schema"]):
        raise ValueError("unknown manifest schema")
    class_name = config["package"].replace("/", ".") + "." + run["class"]
    if manifest["reference"] != engine or manifest["commit"] != config["commit"] or manifest["test_class"] != class_name:
        raise ValueError("unknown reference manifest")
    if any(type(manifest[field]) is not int for field in ("tests", "failures", "errors", "skipped")):
        raise ValueError("non-integer reference run counts")
    if manifest["tests"] != len(run["tests"]) or any(manifest[field] != 0 for field in ("failures", "errors", "skipped")) or sorted(manifest["testcases"]) != sorted(run["tests"]):
        raise ValueError("reference test run incomplete")
    if engine == "forge" and (manifest["strict_choices"] is not True or manifest["mana_auto_payment"] is not False or manifest["casting_scope"] != "direct-stack-admission-no-payment"):
        raise ValueError("unknown Forge manifest execution scope")
    prefix = f"work/reference-research/{run['folder']}"
    source = f"tests/reference/{engine}/{run['class']}.java"
    xml = f"{prefix}/TEST-{class_name}.xml"
    compiled_prefix = f"work/reference-research/{config['checkout']}/{config['module']}/target/test-classes/{config['package']}/{run['class']}"
    required = {f"{prefix}/run.log", xml, source, f"{compiled_prefix}.class"}
    required.update(f"{compiled_prefix}{suffix}.class" for suffix in run.get("extra_classes", []))
    required.update(f"{prefix}/{name}.json" for name in run["scenarios"])
    if run.get("include_staged_source"):
        required.add(f"work/reference-research/{config['checkout']}/{config['module']}/src/test/java/{config['package']}/{run['class']}.java")
    if len(manifest["files"]) != len(required) or {item["path"] for item in manifest["files"]} != required:
        raise ValueError("missing or duplicate provenance artifact")
    return class_name, required


def verify_evidence(project: Path, runs: list[dict] | None = None) -> dict:
    """Validate pinned raw artifacts for each engine, including the strict controller."""
    for config in ENGINES.values():
        checkout = project / f"work/reference-research/{config['checkout']}"
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
        dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=checkout, text=True).strip()
        if head != config["commit"] or dirty:
            raise ValueError("pinned reference checkout changed")
    module_spec = importlib.util.spec_from_file_location("smoke", project / "scripts/collect-reference-smoke.py")
    if module_spec is None or module_spec.loader is None:
        raise ValueError("missing evidence parser")
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    provenance = []
    for run in RUNS if runs is None else runs:
        engine = run["engine"]
        config = ENGINES[engine]
        evidence = project / f"tests/reference/{engine}/evidence"
        payload = (evidence / run["manifest"]).read_bytes()
        manifest = json.loads(payload)
        class_name, _ = verify_manifest_contract(manifest, run, config)
        prefix = f"work/reference-research/{run['folder']}"
        source = f"tests/reference/{engine}/{run['class']}.java"
        xml = f"{prefix}/TEST-{class_name}.xml"
        for item in manifest["files"]:
            if hashlib.sha256((project / item["path"]).read_bytes()).hexdigest() != item["sha256"]:
                raise ValueError("local reference artifact hash mismatch")
        checkout = project / f"work/reference-research/{config['checkout']}"
        staged = checkout / f"{config['module']}/src/test/java/{config['package']}/{run['class']}.java"
        if staged.read_bytes() != (project / source).read_bytes():
            raise ValueError("staged adapter changed")
        if not module.run_passed((project / xml).read_text(), class_name, run["tests"]):
            raise ValueError("exact reference run failed or did not execute")
        for name in run["scenarios"]:
            if (evidence / f"{name}.json").read_bytes() != (project / f"{prefix}/{name}.json").read_bytes():
                raise ValueError("retained reference trace differs from hashed run")
        provenance.append({"engine": engine, "class": class_name, "manifestSha256": hashlib.sha256(payload).hexdigest(), "adapterSha256": hashlib.sha256((project / source).read_bytes()).hexdigest()})
    return {"referenceRuns": provenance}


def read_expectations(project: Path, copy_aware: bool = False) -> tuple[dict, dict]:
    """Freeze source bytes and verify cited clauses exist; reasoning still requires review."""
    path = project / ("tests/scenarios/spell-copies.expected.json" if copy_aware else "tests/scenarios/instant-interactions.expected.json")
    payload = path.read_bytes()
    expected = json.loads(payload)
    rules = expected["rules"]
    if rules["edition"] != "2026-09-25" or rules["url"] != "https://media.wizards.com/2026/downloads/MagicCompRules%2020260925.txt":
        raise ValueError("unknown rules edition")
    raw = (project / "work/rules/MagicCompRules-20260925.txt").read_bytes()
    if hashlib.sha256(raw).hexdigest() != rules["sha256"]:
        raise ValueError("frozen rules source changed")
    rule_text = raw.decode()
    for case in expected["cases"]:
        for clause in case["rules"]:
            if re.search(r"^" + re.escape(clause) + r"(?:\.\s|\s)", rule_text, re.MULTILINE) is None:
                raise ValueError("cited rules clause absent from source")
    return expected, {"expectationsSha256": hashlib.sha256(payload).hexdigest(), "rulesSha256": rules["sha256"], "expectationDisclosure": expected["observation_disclosure"]}


def main() -> int:
    try:
        copy_aware = len(sys.argv) == 3 and sys.argv[1] == "--copies"
        if len(sys.argv) != 2 and not copy_aware:
            print(json.dumps({"status": "fail", "data": {"arguments": "expected native trace path"}}))
            return 1
        project = Path(__file__).resolve().parents[1]
        stages = COPY_STAGES if copy_aware else STAGES
        native_path = Path(sys.argv[2] if copy_aware else sys.argv[1])
        payload = native_path.read_bytes()
        provenance = verify_evidence(project, COPY_RUNS if copy_aware else None)
        references = {engine: {name: json.loads((project / f"tests/reference/{engine}/evidence/{name}.json").read_text()) for name in stages} for engine in ENGINES}
        expectations, source_provenance = read_expectations(project, copy_aware)
        report = compare(json.loads(payload), references, expectations, copy_aware)
        report["evidence"] = provenance | source_provenance | {"nativeTraceSha256": hashlib.sha256(payload).hexdigest()}
        print(json.dumps({"status": "success", "data": report}, indent=2))
        return 0 if report["scenariosAgreed"] == len(stages) else 1
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.CalledProcessError) as error:
        print(f"comparison unavailable: {error}", file=sys.stderr)
        print(json.dumps({"status": "error", "message": "Interaction comparison unavailable", "code": "comparison-unavailable"}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
