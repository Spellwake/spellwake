"""Guard against false agreement and malformed native/reference traces."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("interactions", ROOT / "scripts/compare-interactions.py")
assert SPEC is not None and SPEC.loader is not None
COMPARISON = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARISON)


class ComparisonTests(unittest.TestCase):
    def setUp(self) -> None:
        self.references = {engine: {name: json.loads((ROOT / f"tests/reference/{engine}/evidence/{name}.json").read_text()) for name in COMPARISON.STAGES} for engine in COMPARISON.ENGINES}
        self.expectations = json.loads((ROOT / "tests/scenarios/instant-interactions.expected.json").read_text())
        # Unit fixture review state is synthetic. It does not review or modify the saved expectations.
        self.expectations["review_status"] = "reviewed"
        # An equal projection is fixture setup; mutation tests below exercise comparator behavior.
        self.native = {"status": "success", "data": {"schema_version": 2, "kind": "restricted-instant-interactions", "registry_input": "embedded-four-card-test-subset", "registry_source_sha256": "a" * 64, "scenarios": [{"scenario": name, "observations": copy.deepcopy(trace["observations"])} for name, trace in self.references["xmage"].items()]}}

    def test_matching_projection_preserves_explicit_qualification_limits(self) -> None:
        report = COMPARISON.compare(self.native, self.references, self.expectations)
        self.assertEqual((report["scenariosAgreed"], report["checkpointsAgreed"]), (4, 14))
        self.assertEqual(report["fullyQualifiedCards"], 0)
        self.assertEqual(report["fullGameComparisons"], 0)
        self.assertEqual(report["performanceBenchmarks"], 0)

    def test_wrong_intermediate_stack_zone_or_damage_is_a_disagreement(self) -> None:
        for field in ("stack", "zone", "damage", "target", "owner"):
            native = copy.deepcopy(self.native)
            point = native["data"]["scenarios"][0]["observations"][0]
            if field == "target":
                point = native["data"]["scenarios"][3]["observations"][2]
                point["stack_bottom_first"][-1]["targets"] = ["bolt"]
            elif field == "owner":
                point["cards"][0]["owner"] = "B"
            elif field == "stack":
                point["stack_bottom_first"] = []
            elif field == "zone":
                point["cards"][0]["zone"] = "HAND"
            else:
                point["cards"][-1]["damage"] = 1
            report = COMPARISON.compare(native, self.references, self.expectations)
            self.assertEqual(report["scenariosDisagreed"], 1)
            self.assertEqual(report["checkpointsAgreed"], 13)

    def test_missing_duplicate_reordered_or_extra_checkpoints_never_pass(self) -> None:
        for change in ("missing", "duplicate", "reverse", "extra"):
            native = copy.deepcopy(self.native)
            points = native["data"]["scenarios"][1]["observations"]
            if change == "missing":
                points.pop()
            elif change == "duplicate":
                points[1] = points[0]
            elif change == "reverse":
                points.reverse()
            else:
                points.append(points[0])
            with self.assertRaises(ValueError):
                COMPARISON.compare(native, self.references, self.expectations)

    def test_missing_scenarios_aliases_or_reference_identity_never_pass(self) -> None:
        native = copy.deepcopy(self.native)
        native["data"]["registry_source_sha256"] = "not-a-hash"
        with self.assertRaises(ValueError):
            COMPARISON.compare(native, self.references, self.expectations)
        native = copy.deepcopy(self.native)
        native["data"]["schema_version"] = True
        with self.assertRaises(ValueError):
            COMPARISON.compare(native, self.references, self.expectations)
        native = copy.deepcopy(self.native)
        native["data"]["scenarios"].pop()
        with self.assertRaises(ValueError):
            COMPARISON.compare(native, self.references, self.expectations)
        native = copy.deepcopy(self.native)
        native["data"]["scenarios"][0]["observations"][0]["cards"].pop()
        with self.assertRaises(ValueError):
            COMPARISON.compare(native, self.references, self.expectations)
        self.references["xmage"]["bolt-control"]["commit"] = "wrong"
        with self.assertRaises(ValueError):
            COMPARISON.compare(self.native, self.references, self.expectations)

    def test_primary_reference_is_required_and_engine_policy_is_explicit(self) -> None:
        references = copy.deepcopy(self.references)
        references.pop("forge")
        with self.assertRaises(ValueError):
            COMPARISON.compare(self.native, references, self.expectations)
        for field, bad in [("strict_choices", 1), ("mana_auto_payment", True), ("direct_stack_admission", False), ("casting_scope", "full-cast")]:
            references = copy.deepcopy(self.references)
            references["forge"]["counter-bolt"][field] = bad
            with self.assertRaises(ValueError):
                COMPARISON.compare(self.native, references, self.expectations)

    def test_a_reference_disagreement_is_visible_even_if_the_other_reference_matches(self) -> None:
        references = copy.deepcopy(self.references)
        references["forge"]["counter-counter"]["observations"][3]["stack_bottom_first"] = []
        report = COMPARISON.compare(self.native, references, self.expectations)
        self.assertEqual(report["scenariosDisagreed"], 1)
        self.assertEqual(report["referenceAgreements"]["xmage"]["checkpointsMatchingNative"], 14)
        self.assertEqual(report["referenceAgreements"]["forge"]["checkpointsMatchingNative"], 13)

    def test_equal_engines_do_not_pass_unreviewed_or_unresolved_expectations(self) -> None:
        for status in ("pending-independent-review", "unresolved"):
            expectations = copy.deepcopy(self.expectations)
            expectations["review_status"] = status
            report = COMPARISON.compare(self.native, self.references, expectations)
            self.assertEqual(report["scenariosAgreed"], 0)
            self.assertEqual(report["scenariosAwaitingReview"], 4)
            self.assertEqual(report["referenceAgreements"]["forge"]["checkpointsMatchingNative"], 14)
            self.assertEqual(report["checkpointsAgreed"], 0)

    def test_consensus_is_not_the_expectation_authority(self) -> None:
        native = copy.deepcopy(self.native)
        references = copy.deepcopy(self.references)
        native["data"]["scenarios"][0]["observations"][0]["cards"][-1]["damage"] = 1
        for engine in references:
            point = references[engine]["bolt-control"]["observations"][0]
            next(card for card in point["cards"] if card["alias"] == "lion")["damage"] = 1
        report = COMPARISON.compare(native, references, self.expectations)
        self.assertEqual(report["referenceAgreements"]["forge"]["checkpointsMatchingNative"], 14)
        self.assertEqual(report["referenceAgreements"]["xmage"]["checkpointsMatchingNative"], 14)
        self.assertEqual(report["scenariosDisagreed"], 1)
        self.assertEqual(report["checkpointsAgreed"], 13)

    def test_uncited_or_missing_expected_checkpoints_cannot_pass(self) -> None:
        for field, bad in [("rules", []), ("reasoning", ""), ("observations", [])]:
            expectations = copy.deepcopy(self.expectations)
            expectations["cases"][0][field] = bad
            with self.assertRaises(ValueError):
                COMPARISON.compare(self.native, self.references, expectations)

    def test_manifest_cannot_omit_or_duplicate_the_executed_strict_controller(self) -> None:
        run = next(run for run in COMPARISON.RUNS if run["engine"] == "forge")
        manifest = json.loads((ROOT / "tests/reference/forge/evidence/instant-run-manifest.json").read_text())
        COMPARISON.verify_manifest_contract(manifest, run, COMPARISON.ENGINES["forge"])
        inner = next(item for item in manifest["files"] if "$StrictController.class" in item["path"])
        for replacement in (None, manifest["files"][0]):
            corrupted = copy.deepcopy(manifest)
            corrupted["files"] = [item for item in corrupted["files"] if item["path"] != inner["path"]]
            if replacement is not None:
                corrupted["files"].append(replacement)
            with self.assertRaises(ValueError):
                COMPARISON.verify_manifest_contract(corrupted, run, COMPARISON.ENGINES["forge"])

    def test_manifest_run_identity_and_policy_cannot_be_relabelled(self) -> None:
        run = next(run for run in COMPARISON.RUNS if run["engine"] == "forge")
        manifest = json.loads((ROOT / "tests/reference/forge/evidence/instant-run-manifest.json").read_text())
        for field, bad in [("commit", "wrong"), ("tests", 3), ("testcases", ["boltControl"]), ("skipped", 1), ("failures", False), ("strict_choices", False), ("mana_auto_payment", True)]:
            corrupted = copy.deepcopy(manifest)
            corrupted[field] = bad
            with self.assertRaises(ValueError):
                COMPARISON.verify_manifest_contract(corrupted, run, COMPARISON.ENGINES["forge"])

    def test_rules_numbers_are_exact_and_bool_float_or_bad_strings_reject(self) -> None:
        for value in (True, False, 1.0, "01", "-0", "+1", "1e3"):
            with self.assertRaises(ValueError):
                COMPARISON.number(value)
        self.assertEqual(COMPARISON.number(90071992547409931234567890), "90071992547409931234567890")
        self.assertEqual(COMPARISON.number("-4"), "-4")


if __name__ == "__main__":
    unittest.main()
