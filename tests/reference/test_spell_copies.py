"""False-pass guards for the copy-aware comparison profile, not engine execution evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("copy_comparison", ROOT / "scripts/compare-interactions.py")
assert SPEC is not None and SPEC.loader is not None
COMPARISON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARISON)


class CopyComparisonTests(unittest.TestCase):
    def setUp(self) -> None:
        self.references = {engine: {name: json.loads((ROOT / f"tests/reference/{engine}/evidence/{name}.json").read_text()) for name in COMPARISON.COPY_STAGES} for engine in COMPARISON.ENGINES}
        self.expectations = json.loads((ROOT / "tests/scenarios/spell-copies.expected.json").read_text())
        # Synthetic review status/projections are unit fixtures, never source-review or execution evidence.
        self.expectations["review_status"] = "reviewed"
        self.native = {"status": "success", "data": {"schema_version": 3, "kind": "restricted-spell-copies", "registry_input": "embedded-five-card-test-subset", "registry_source_sha256": "a" * 64, "scenarios": [{"scenario": name, "observations": copy.deepcopy(trace["observations"])} for name, trace in self.references["xmage"].items()]}}

    def test_six_cases_and_38_checkpoints_keep_scope_exclusions(self) -> None:
        result = COMPARISON.compare(self.native, self.references, self.expectations, True)
        self.assertEqual((result["scenariosAgreed"], result["checkpointsAgreed"]), (6, 38))
        self.assertIn("pre-SBA copy exit equivalence", result["excluded"])
        self.assertEqual(result["fullyQualifiedCards"], 0)

    def test_pending_review_blocks_an_engine_consensus(self) -> None:
        self.expectations["review_status"] = "pending-independent-review"
        result = COMPARISON.compare(self.native, self.references, self.expectations, True)
        self.assertEqual((result["scenariosAgreed"], result["scenariosAwaitingReview"]), (0, 6))

    def test_wrong_copy_ownership_even_with_reference_consensus_disagrees_with_source(self) -> None:
        for points in [self.native["data"]["scenarios"][0]["observations"], self.references["forge"]["copy-redirect"]["observations"], self.references["xmage"]["copy-redirect"]["observations"]]:
            points[2]["stack_bottom_first"][-1]["owner"] = "A"
        result = COMPARISON.compare(self.native, self.references, self.expectations, True)
        self.assertEqual(result["scenariosDisagreed"], 1)

    def test_missing_kind_or_physical_copy_row_never_passes(self) -> None:
        for mutation in ("kind", "physical", "duplicate"):
            native = copy.deepcopy(self.native)
            point = native["data"]["scenarios"][0]["observations"][2]
            if mutation == "kind": point["stack_bottom_first"][-1]["kind"] = "card-spell"
            elif mutation == "physical": point["cards"].append(dict(point["cards"][0], alias="bolt-copy"))
            else: point["stack_bottom_first"].append(point["stack_bottom_first"][-1])
            with self.assertRaises(ValueError): COMPARISON.compare(native, self.references, self.expectations, True)

    def test_changed_counter_target_or_lifetime_is_detected(self) -> None:
        for mutation in ("target", "missing"):
            native = copy.deepcopy(self.native)
            point = native["data"]["scenarios"][2]["observations"][3]
            if mutation == "target": point["stack_bottom_first"][-1]["targets"] = ["bolt"]
            else: point["stack_bottom_first"].pop(1)
            result = COMPARISON.compare(native, self.references, self.expectations, True)
            self.assertEqual(result["scenariosDisagreed"], 1)

    def test_missing_stage_bool_quantity_or_wrong_source_contract_rejects(self) -> None:
        for mutation in ("stage", "number", "schema"):
            native = copy.deepcopy(self.native)
            if mutation == "stage": native["data"]["scenarios"][0]["observations"].pop()
            elif mutation == "number": native["data"]["scenarios"][0]["observations"][0]["cards"][-1]["damage"] = False
            else: native["data"]["schema_version"] = True
            with self.assertRaises(ValueError): COMPARISON.compare(native, self.references, self.expectations, True)

    def test_copy_run_requires_staged_source_inner_class_and_exact_case_count(self) -> None:
        run = COMPARISON.COPY_RUNS[0]
        manifest = json.loads((ROOT / "tests/reference/forge/evidence/spell-copy-run-manifest.json").read_text())
        COMPARISON.verify_manifest_contract(manifest, run, COMPARISON.ENGINES["forge"])
        for match in ("$StrictController.class", "src/test/java/forge/ai/SpellCopyReferenceTest.java"):
            bad = copy.deepcopy(manifest)
            bad["files"] = [item for item in bad["files"] if match not in item["path"]]
            with self.assertRaises(ValueError): COMPARISON.verify_manifest_contract(bad, run, COMPARISON.ENGINES["forge"])
        bad = copy.deepcopy(manifest)
        bad["tests"] = 5
        with self.assertRaises(ValueError): COMPARISON.verify_manifest_contract(bad, run, COMPARISON.ENGINES["forge"])


if __name__ == "__main__":
    unittest.main()
