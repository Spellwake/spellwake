"""Validate pinned local reference smoke artifacts. Never claim a Rust comparison.

Run Maven separately using docs/reference-engine-harness-research.md. XML is trusted
local build evidence, not proof supplied by an untrusted remote engine or user.
"""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET


def case_passed(xml: str, class_name: str, method: str) -> bool:
    """Require one named executed testcase and a clean enclosing suite."""
    root = ET.fromstring(xml)
    for key in ("failures", "errors", "skipped"):
        if root.get(key) != "0":
            return False
    if int(root.get("tests", "0")) < 1:
        return False
    cases = [case for case in root.findall("testcase") if case.get("classname") == class_name and case.get("name") == method]
    return len(cases) == 1 and not any(cases[0].find(tag) is not None for tag in ("failure", "error", "skipped"))


def run_passed(xml: str, class_name: str, methods: list[str]) -> bool:
    """Require a clean exact named run, not just successful selected cases in a larger suite."""
    try:
        root = ET.fromstring(xml)
    except ET.ParseError:
        return False
    if root.tag != "testsuite" or root.get("tests") != str(len(methods)) or not methods or len(set(methods)) != len(methods):
        return False
    cases = root.findall("testcase")
    actual = sorted((case.get("classname", ""), case.get("name", "")) for case in cases)
    if actual != sorted((class_name, method) for method in methods):
        return False
    return all(case_passed(xml, class_name, method) for method in methods)


def collect(project: Path) -> dict:
    """Check source identity and exact named reports; keep Rust evidence absent."""
    manifest = json.loads((project / "tests/reference/smoke-manifest.json").read_text())
    if manifest["version"] != 1:
        raise ValueError("unsupported smoke manifest")
    results = []
    for engine in manifest["engines"]:
        checkout = project / engine["checkout"]
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=checkout, text=True).strip()
        dirty = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=no"], cwd=checkout, text=True).strip()
        if commit != engine["commit"] or dirty:
            raise ValueError(f"{engine['name']}: source identity/cleanliness mismatch")
        log = project / engine["log"]
        log_hash = hashlib.sha256(log.read_bytes()).hexdigest()
        for case in engine["cases"]:
            report = checkout / case["report"]
            payload = report.read_bytes()
            passed = case_passed(payload.decode(), case["class"], case["method"])
            results.append({"id": case["id"], "reference": engine["name"], "commit": commit, "referenceStatus": "passed" if passed else "failed", "rustStatus": "not-compared", "reportSha256": hashlib.sha256(payload).hexdigest(), "logSha256": log_hash})
    if not results:
        raise ValueError("empty reference run")
    return {"kind": "reference-only-smoke", "cases": results, "referencePassed": sum(case["referenceStatus"] == "passed" for case in results), "rustDifferentialMatches": 0, "performanceBenchmarks": 0, "qualifiedCards": 0}


def main() -> int:
    """Emit the standard envelope and reject absent or failing evidence."""
    try:
        data = collect(Path(__file__).resolve().parents[1])
        print(json.dumps({"status": "success", "data": data}, indent=2))
        return 0 if all(case["referenceStatus"] == "passed" for case in data["cases"]) else 1
    except (OSError, ValueError, KeyError, TypeError, ET.ParseError, subprocess.CalledProcessError) as error:
        print(f"reference evidence unavailable: {error}", file=sys.stderr)
        print(json.dumps({"status": "error", "message": "Reference smoke evidence unavailable", "code": "reference-evidence-unavailable"}))
        return 1

if __name__ == "__main__":
    raise SystemExit(main())
