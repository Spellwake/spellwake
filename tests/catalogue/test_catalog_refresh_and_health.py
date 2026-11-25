"""Unit and integration tests for incremental catalog refresh, preview isolation, and health badges."""

from __future__ import annotations

import gzip
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import badge_svg  # noqa: E402
import catalog_refresh  # noqa: E402
import health_checks  # noqa: E402


def _make_card(
    scryfall_id: str,
    oracle_id: str,
    name: str,
    type_line: str,
    mana_cost: str,
    oracle_text: str,
    *,
    power: str | None = None,
    toughness: str | None = None,
    standard: str = "not_legal",
    modern: str = "legal",
    commander: str = "legal",
    future: str = "not_legal",
) -> dict:
    card = {
        "object": "card",
        "id": scryfall_id,
        "oracle_id": oracle_id,
        "name": name,
        "layout": "normal",
        "type_line": type_line,
        "mana_cost": mana_cost,
        "oracle_text": oracle_text,
        "legalities": {
            "commander": commander,
            "standard": standard,
            "pioneer": "not_legal",
            "modern": modern,
            "legacy": modern,
            "vintage": modern,
            "future": future,
        },
    }
    if power is not None:
        card["power"] = power
    if toughness is not None:
        card["toughness"] = toughness
    return card


def _write_snapshot(directory: Path, prefix: str, updated_at: str, cards: list[dict]) -> tuple[Path, Path]:
    snap_path = directory / f"{prefix}.jsonl.gz"
    meta_path = directory / f"{prefix}.meta.json"
    raw_lines = "".join(json.dumps(card) + "\n" for card in cards).encode("utf-8")
    compressed = gzip.compress(raw_lines)
    snap_path.write_bytes(compressed)
    metadata = {
        "object": "bulk_data",
        "id": "27bf3214-1271-490b-bdfe-c0be6c23d02e",
        "type": "oracle_cards",
        "updated_at": updated_at,
        "jsonl_download_uri": f"https://data.scryfall.io/oracle-cards/{prefix}.jsonl.gz",
        "compressed_size": len(compressed),
    }
    meta_path.write_text(json.dumps(metadata), encoding="utf-8")
    return meta_path, snap_path


class CatalogRefreshTests(unittest.TestCase):
    def test_should_download_snapshot_skips_unchanged_descriptor(self) -> None:
        prev = {
            "updatedAt": "2026-10-29T21:01:58.033+00:00",
            "jsonlDownloadUri": "https://data.scryfall.io/oracle-cards/oracle-cards-20261029210158.jsonl.gz",
            "compressedSize": 25229019,
        }
        same_remote = {
            "updated_at": "2026-10-29T21:01:58.033+00:00",
            "jsonl_download_uri": "https://data.scryfall.io/oracle-cards/oracle-cards-20261029210158.jsonl.gz",
            "compressed_size": 25229019,
        }
        changed_remote = dict(same_remote, updated_at="2026-10-30T21:01:58.033+00:00")
        self.assertFalse(catalog_refresh.should_download_snapshot(prev, same_remote))
        self.assertTrue(catalog_refresh.should_download_snapshot(prev, changed_remote))
        self.assertTrue(catalog_refresh.should_download_snapshot(None, same_remote))

    def test_stage3_qualifications_detect_drifted_text_and_require_game_log(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            oid_ok = "11111111-1111-1111-1111-111111111111"
            oid_drift = "22222222-2222-2222-2222-222222222222"
            card_ok = _make_card("s1", oid_ok, "Grizzly Bears", "Creature — Bear", "{1}{G}", "", power="2", toughness="2")
            card_drift = _make_card("s2", oid_drift, "Errata Bolt", "Instant", "{R}", "Errata Bolt deals 4 damage to any target.")
            _, snap = _write_snapshot(root, "snap", "2026-11-05T00:00:00Z", [card_ok, card_drift])

            ok_sha = catalog_refresh._printed_faces_sha256(card_ok)
            old_drift_sha = hashlib.sha256(b"old-text").hexdigest()
            qual_path = root / "qualifications.json"
            qual_path.write_text(
                json.dumps(
                    [
                        {
                            "name": "Grizzly Bears",
                            "oracle_id": oid_ok,
                            "printed_faces_sha256": ok_sha,
                            "scope": "complete",
                            "evidence": "Vanilla bear game check.",
                            "tests": ["grizzly_bears_cast_and_combat"],
                        },
                        {
                            "name": "Errata Bolt",
                            "oracle_id": oid_drift,
                            "printed_faces_sha256": old_drift_sha,
                            "scope": "complete",
                            "evidence": "Old text check.",
                            "tests": ["errata_bolt_damage"],
                        },
                    ]
                ),
                encoding="utf-8",
            )

            # Without a game log, verifiedCompleteCards MUST remain None (unverified).
            unverified = catalog_refresh.evaluate_stage3_qualifications(
                snap, qual_path, {oid_ok, oid_drift}, {oid_ok, oid_drift}, game_log_path=None
            )
            self.assertEqual(unverified["declaredCompleteMatchingSnapshot"], 1)
            self.assertEqual(len(unverified["staleOrDriftedEntries"]), 1)
            self.assertEqual(unverified["staleOrDriftedEntries"][0]["reason"], "printed-faces-drifted")
            self.assertEqual(unverified["verificationState"], "unverified-in-this-run")
            self.assertIsNone(unverified["verifiedCompleteCards"])

            # With an actual passing game log, verifiedCompleteCards reflects the confirmed test.
            log_path = root / "games.log"
            log_path.write_text(
                "test keyword_creatures::tests::grizzly_bears_cast_and_combat ... ok\n",
                encoding="utf-8",
            )
            verified = catalog_refresh.evaluate_stage3_qualifications(
                snap, qual_path, {oid_ok, oid_drift}, {oid_ok, oid_drift}, game_log_path=log_path
            )
            self.assertEqual(verified["verificationState"], "verified-by-game-log")
            self.assertEqual(verified["verifiedCompleteCards"], 1)


class HealthChecksTests(unittest.TestCase):
    def test_pet_cosmetics_remains_visibly_unknown_without_real_checks(self) -> None:
        pet_spec = next(d for d in health_checks.DOMAINS if d.slug == "pet-cosmetics")
        result = health_checks.evaluate_domain(
            pet_spec, run_db=True, run_frontend=True, db_available=True
        )
        self.assertEqual(result["localStatus"], "unknown")
        self.assertEqual(result["localBadge"]["color"], "lightgrey")
        self.assertIn("unknown", result["localBadge"]["message"])
        self.assertEqual(result["liveStatus"], "unknown")
        self.assertEqual(result["passedTests"], 0)
        self.assertTrue(result["unqualifiedReason"])

    def test_zero_test_command_never_produces_green_badge(self) -> None:
        spec = health_checks.DomainSpec(
            slug="synthetic-empty",
            label="Synthetic",
            rust_checks=(
                health_checks.CommandSpec(
                    kind="cargo",
                    label="empty filter simulation",
                    args=("python3", "-c", "print('test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')"),
                ),
            ),
            db_filters=(),
            frontend_checks=(),
            hosted_credentials=("HOSTED_SECRET",),
        )
        result = health_checks.evaluate_domain(
            spec, run_db=False, run_frontend=False, db_available=False
        )
        self.assertEqual(result["localStatus"], "unknown")
        self.assertEqual(result["localBadge"]["color"], "lightgrey")
        self.assertEqual(result["liveStatus"], "unknown")

    def test_failing_command_produces_red_badge(self) -> None:
        spec = health_checks.DomainSpec(
            slug="synthetic-fail",
            label="Synthetic",
            rust_checks=(
                health_checks.CommandSpec(
                    kind="cargo",
                    label="failing check simulation",
                    args=(
                        "python3",
                        "-c",
                        "import sys; print('test result: FAILED. 2 passed; 1 failed; 0 ignored;'); sys.exit(1)",
                    ),
                ),
            ),
            db_filters=(),
            frontend_checks=(),
            hosted_credentials=("HOSTED_SECRET",),
        )
        result = health_checks.evaluate_domain(
            spec, run_db=False, run_frontend=False, db_available=False
        )
        self.assertEqual(result["localStatus"], "failing")
        self.assertEqual(result["localBadge"]["color"], "red")
        self.assertEqual(result["failedTests"], 1)

    def test_parser_audit_badges_enforce_honest_purity_and_defect_gating(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            badges_dir = Path(tmp)
            passing = {
                "totalCards": 32155,
                "strictlyCompiledCards": 32155,
                "zeroCriticalDefectCards": 32155,
                "zeroStructuralDefectCards": 32155,
                "fullyTypedPureCards": 29309,
                "cardsWithCriticalDefects": 0,
                "cardsWithStructuralDefects": 0,
                "cardsWithStrictFailures": 0,
                "cardsWithOpaqueStrings": 2846,
            }
            eps = catalog_refresh.write_parser_audit_badges(badges_dir, passing)
            self.assertEqual(eps["parserAuditDefects"]["color"], "green")
            self.assertFalse(eps["parserAuditDefects"]["isError"])
            self.assertIn("0 defects", eps["parserAuditDefects"]["message"])
            self.assertEqual(eps["parserTypedAst"]["color"], "blue")
            self.assertIn("91.1%", eps["parserTypedAst"]["message"])
            self.assertNotIn("100.0%", eps["parserTypedAst"]["message"])

            new_set_unparsed = dict(
                passing,
                totalCards=32682,
                strictlyCompiledCards=32541,
                zeroCriticalDefectCards=32541,
                zeroStructuralDefectCards=32682,
                cardsWithCriticalDefects=141,
                cardsWithStructuralDefects=0,
                cardsWithStrictFailures=141,
            )
            unparsed_eps = catalog_refresh.write_parser_audit_badges(badges_dir, new_set_unparsed)
            self.assertEqual(unparsed_eps["parserAuditDefects"]["color"], "yellow")
            self.assertFalse(unparsed_eps["parserAuditDefects"]["isError"])
            self.assertIn("0 corruption", unparsed_eps["parserAuditDefects"]["message"])
            self.assertIn("141 unparsed", unparsed_eps["parserAuditDefects"]["message"])

            failing = dict(
                passing,
                zeroCriticalDefectCards=32150,
                zeroStructuralDefectCards=32150,
                cardsWithCriticalDefects=5,
                cardsWithStructuralDefects=5,
            )
            fail_eps = catalog_refresh.write_parser_audit_badges(badges_dir, failing)
            self.assertEqual(fail_eps["parserAuditDefects"]["color"], "red")
            self.assertTrue(fail_eps["parserAuditDefects"]["isError"])
            self.assertIn("fail", fail_eps["parserAuditDefects"]["message"])
            self.assertIn("5 structural", fail_eps["parserAuditDefects"]["message"])

            empty = dict(
                passing,
                totalCards=0,
                strictlyCompiledCards=0,
                zeroCriticalDefectCards=0,
                zeroStructuralDefectCards=0,
                fullyTypedPureCards=0,
            )
            empty_eps = catalog_refresh.write_parser_audit_badges(badges_dir, empty)
            self.assertEqual(empty_eps["parserAuditDefects"]["color"], "lightgrey")
            self.assertEqual(empty_eps["parserAuditDefects"]["message"], "unknown")

    def test_oracle_parser_domain_reports_offline_live_status_honestly(self) -> None:
        spec = next(d for d in health_checks.DOMAINS if d.slug == "oracle-parser")
        self.assertTrue(spec.local_only)
        synth = health_checks.DomainSpec(
            slug="oracle-parser-synth",
            label="Oracle Parser",
            rust_checks=(
                health_checks.CommandSpec(
                    kind="cargo",
                    label="synthetic parser pass",
                    args=("python3", "-c", "print('test result: ok. 4 passed; 0 failed; 0 ignored;')"),
                ),
            ),
            db_filters=(),
            frontend_checks=(),
            hosted_credentials=(),
            local_only=True,
        )
        res = health_checks.evaluate_domain(
            synth, run_db=False, run_frontend=False, db_available=False
        )
        self.assertEqual(res["localStatus"], "local-verified")
        self.assertEqual(res["localBadge"]["color"], "green")
        self.assertEqual(res["liveStatus"], "not-applicable")
        self.assertEqual(res["liveBadge"]["message"], "n/a · offline")

    def test_badge_svg_renders_valid_svg_and_escapes_xml(self) -> None:
        svg = badge_svg.render_svg("AI & Pets", "unknown <unqualified>", "lightgrey")
        self.assertTrue(svg.startswith("<svg "))
        self.assertIn("viewBox=", svg)
        self.assertIn("AI &amp; Pets", svg)
        self.assertIn("unknown &lt;unqualified&gt;", svg)
        self.assertIn("#57606a", svg)


if __name__ == "__main__":
    unittest.main()
