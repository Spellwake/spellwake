"""Offline checks for what an Oracle reviewer is allowed to infer from a Rust symbol."""

import runpy
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2] / "scripts"
sys.path.insert(0, str(SCRIPTS))
harness = runpy.run_path(str(SCRIPTS / "card-support-agent.py"))


class ReviewLabels(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.index = harness["operation_index"]()
        cls.rules = harness["Index"]({})
        directory = tempfile.TemporaryDirectory()
        cls.addClassCleanup(directory.cleanup)
        cls.empty_probes = Path(directory.name) / "clauses.tsv"
        cls.empty_probes.write_text("oracle_id\tface_name\tclause\tstatus\tcontext\n")

    def packet(self, text):
        card = {
            "oracle_id": "example",
            "layout": "normal",
            "faces": [
                {
                    "name": "Example",
                    "face_index": 0,
                    "oracle_text": text,
                    "type_line": "Enchantment",
                }
            ],
        }
        (unit,) = harness["units"](card)
        self.index.card_evidence = {"example": {"current_processor": "Blocked"}}
        self.index.paragraph_probes = harness["load_paragraph_probes"](
            {"example": card}, self.empty_probes
        )
        choices = {0: harness["candidates"](card, unit, self.rules, self.index, 5)}
        return harness["prompt_for"](card, [unit], choices, (self.rules, self.index))

    def test_selector_and_effect_are_distinct(self):
        operations = self.packet("Creatures you control have haste.")["operations"]
        self.assertIn("ControllerSelector::SourceController", operations)
        self.assertIn("StaticPart::Abilities", operations)
        selector = operations["ControllerSelector::SourceController"]
        self.assertEqual(selector["role"], "controller filter")
        self.assertEqual(selector["evidence"], "source-checked behavior")
        self.assertTrue(selector["sources"])
        effect = self.index.rows["StaticPart::ControlSourceController"]["contract"]
        self.assertIn("controller", effect["documentation"])
        self.assertNotEqual(selector["role"], effect["role"])

    def test_zone_input_is_visible_with_spell_target(self):
        packet = self.packet("Return target spell to its owner’s hand.")
        operations = packet["operations"]
        self.assertIn("BodyTarget::SingleSpell", operations)
        self.assertIn(
            "battlefield permanent",
            operations["Instruction::ReturnToOwnersHand"]["label"],
        )
        self.assertEqual(
            packet["measured_card_evidence"]["current_processor"], "Blocked"
        )

    def test_conditional_entry_preserves_the_condition(self):
        simple = self.packet("This land enters tapped.")
        conditional = self.packet(
            "This land enters tapped unless you control two or more other lands."
        )
        self.assertEqual(
            simple["review_units"][0]["engine_symbols"][0], "EntryTap::Always"
        )
        self.assertEqual(
            conditional["review_units"][0]["engine_symbols"][0],
            "EntryTap::UnlessTwoOtherLands",
        )

    def test_temporary_grant_has_a_distinct_effect(self):
        packet = self.packet("Creatures you control gain flying until end of turn.")
        self.assertEqual(
            packet["review_units"][0]["engine_symbols"][0],
            "Instruction::CreateContinuousEffect",
        )

    def test_declaration_alone_cannot_support_binding(self):
        choices = {
            0: {"rules": [], "mechanics": ["Example::Variant"], "documented": []}
        }
        card = {"oracle_id": "example"}
        response = {
            "unit": 0,
            "behavior": "An example behavior",
            "binding_basis": "Only a variant name",
            "disposition": "candidate-existing",
            "rule_ids": [],
            "engine_symbols": ["Example::Variant"],
            "missing_primitives": [],
            "scenario": "An example scenario",
        }
        with self.assertRaisesRegex(ValueError, "documented operation"):
            harness["validate_reply"](
                {"oracle_id": "example", "reviews": [response]},
                card,
                [{"unit": 0}],
                choices,
            )


class ParagraphEvidence(unittest.TestCase):
    def test_face_paragraph_and_duplicate_scopes(self):
        from card_oracle_paragraphs import load_paragraph_probes, probe_for_unit

        card = {
            "oracle_id": "example",
            "faces": [
                {
                    "name": "Front",
                    "face_index": 0,
                    "oracle_text": "Flying\nDraw.\nDraw.",
                },
                {
                    "name": "Back",
                    "face_index": 1,
                    "oracle_text": "Draw. (Then exile it.)",
                },
            ],
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "clauses.tsv"
            path.write_text(
                "oracle_id\tface_name\tclause\tstatus\tcontext\n"
                "example\tFront\tFlying\tcompiled\tisolated face\n"
                "example\tFront\tDraw.\tcompiled\tisolated face\n"
                "example\tBack\tDraw.\tunmatched\tisolated face\n"
            )
            probes = load_paragraph_probes({"example": card}, path)
        results = [probe_for_unit(card, u, probes) for u in harness["units"](card)]
        self.assertEqual(
            [r["verdict"] for r in results],
            ["compiled_in_isolation", "unprobed", "unprobed", "unmatched", "unmatched"],
        )
        self.assertEqual(results[0]["probe"]["row"], 2)
        self.assertIsNone(results[1]["probe"])
        self.assertEqual(results[-1]["original_paragraph"], "Draw. (Then exile it.)")
        self.assertEqual(results[-1]["probe"]["normalized_text"], "Draw.")
        self.assertEqual(results[-1]["unit_relation"], "no_operative_text_in_unit")
        engine = harness["operation_index"]()
        engine.paragraph_probes = probes
        indexes = (harness["Index"]({}), engine)
        card["name"] = "Example"
        card["layout"] = "normal"
        for face in card["faces"]:
            face["type_line"] = "Instant"
        units = harness["units"](card)
        answers = [
            {
                "unit": unit["unit"],
                "behavior": "Example behavior",
                "binding_basis": "Insufficient evidence",
                "disposition": "needs-evidence",
                "rule_ids": [],
                "engine_symbols": [],
                "missing_primitives": [],
                "scenario": "Example scenario",
            }
            for unit in units
        ]
        record = harness["make_record"](card, units, answers, [], indexes)
        self.assertEqual(harness["check_record"](card, record, indexes, 5), len(units))
        record["paragraph_probes"][-1]["verdict"] = "compiled_in_isolation"
        with self.assertRaisesRegex(ValueError, "scoped paragraph probe"):
            harness["check_record"](card, record, indexes, 5)


if __name__ == "__main__":
    unittest.main()
