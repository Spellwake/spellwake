#!/usr/bin/env python3
"""Incremental/scheduled Scryfall Oracle catalog refresh and honest card-support counts.

Uses the existing Rust parser (`spellwake-oracle-compiler`), rolling engine compiler
(`spellwake-card-programs support-report`), and card-specific qualification ledger
(`scripts/card-game-qualifications.json`) while keeping preview formats (`future` /
Future Standard) strictly isolated from ordinary base-format legality.

Never fabricates Stage 3 gameplay qualification counts: declared qualifications are
hash-checked against the actual snapshot's printed faces and only marked verified when
a real gameplay test log confirming every named test is provided.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BULK_API_URL = "https://api.scryfall.com/bulk-data/oracle-cards"
ALLOWED_DOWNLOAD_PREFIX = "https://data.scryfall.io/oracle-cards/"
MAX_METADATA_BYTES = 1024 * 1024
MAX_SNAPSHOT_BYTES = 512 * 1024 * 1024
DEFAULT_QUALIFICATIONS_PATH = ROOT / "scripts" / "card-game-qualifications.json"
DEFAULT_VOCAB_PATH = (
    ROOT
    / "work"
    / "resolution-value-choices"
    / "current-s1-packets-v1"
    / "creature-vocabulary.input.v1.json"
)
FACE_KEYS = (
    "name",
    "type_line",
    "mana_cost",
    "oracle_text",
    "power",
    "toughness",
    "loyalty",
    "defense",
    "color_indicator",
)

sys.path.insert(0, str(Path(__file__).resolve().parent))
import badge_svg  # noqa: E402


def _http_get_bytes(url: str, max_bytes: int, timeout: int = 60) -> bytes:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "SpellwakeCatalogRefresh/1.0 (+https://spellwake.com)",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        data = response.read(max_bytes + 1)
    if len(data) > max_bytes:
        raise ValueError(f"response from {url} exceeded {max_bytes} byte safety bound")
    return data


def _http_download_to_file(url: str, destination: Path, expected_size: int, timeout: int = 180) -> str:
    if not url.startswith(ALLOWED_DOWNLOAD_PREFIX):
        raise ValueError(f"refusing untrusted Scryfall download URI: {url}")
    if expected_size <= 0 or expected_size > MAX_SNAPSHOT_BYTES:
        raise ValueError(f"invalid expected compressed_size: {expected_size}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "SpellwakeCatalogRefresh/1.0 (+https://spellwake.com)",
            "Accept": "*/*",
        },
    )
    fd, tmp_name = tempfile.mkstemp(dir=destination.parent, prefix=".oracle-download-", suffix=".tmp")
    tmp_path = Path(tmp_name)
    digest = hashlib.sha256()
    total = 0
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response, os.fdopen(fd, "wb") as out:
            while True:
                chunk = response.read(1024 * 256)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_SNAPSHOT_BYTES:
                    raise ValueError("Scryfall snapshot exceeded safety bound during download")
                digest.update(chunk)
                out.write(chunk)
        if total != expected_size:
            raise ValueError(
                f"downloaded compressed length {total} differs from metadata compressed_size {expected_size}"
            )
        tmp_path.replace(destination)
        return digest.hexdigest()
    finally:
        tmp_path.unlink(missing_ok=True)


def should_download_snapshot(previous_state: dict[str, Any] | None, metadata: dict[str, Any]) -> bool:
    """Return False when the remote bulk descriptor matches persisted state exactly."""
    if not previous_state:
        return True
    return (
        previous_state.get("updatedAt") != metadata.get("updated_at")
        or previous_state.get("jsonlDownloadUri") != metadata.get("jsonl_download_uri")
        or previous_state.get("compressedSize") != metadata.get("compressed_size")
    )


def fetch_or_reuse_scryfall_snapshot(
    state_dir: Path,
    previous_state: dict[str, Any] | None,
    *,
    bulk_api_url: str = DEFAULT_BULK_API_URL,
    force_download: bool = False,
) -> tuple[Path, Path, bool]:
    """Fetch Scryfall bulk metadata and incrementally skip snapshot download if unchanged."""
    raw_meta = _http_get_bytes(bulk_api_url, MAX_METADATA_BYTES)
    metadata = json.loads(raw_meta)
    if metadata.get("object") != "bulk_data" or metadata.get("type") != "oracle_cards":
        raise ValueError("unexpected Scryfall bulk_data metadata payload")

    meta_path = state_dir / "oracle-metadata.json"
    snap_path = state_dir / "oracle-cards.jsonl.gz"

    needs_download = force_download or should_download_snapshot(previous_state, metadata) or not snap_path.is_file()
    if not needs_download and snap_path.is_file():
        actual_size = snap_path.stat().st_size
        if actual_size != metadata.get("compressed_size"):
            needs_download = True

    if needs_download:
        _http_download_to_file(
            metadata["jsonl_download_uri"],
            snap_path,
            int(metadata["compressed_size"]),
        )
        badge_svg.write_atomic_text(meta_path, raw_meta.decode("utf-8"))
        return meta_path, snap_path, True

    if not meta_path.is_file():
        badge_svg.write_atomic_text(meta_path, raw_meta.decode("utf-8"))
    return meta_path, snap_path, False


def _printed_faces_sha256(raw_card: dict[str, Any]) -> str:
    """Compute the exact printed_faces_sha256 used by scripts/card-support-audit.py."""
    faces = raw_card.get("card_faces") or [raw_card]
    normalized = [{k: face[k] for k in FACE_KEYS if k in face} for face in faces]
    encoded = json.dumps(normalized, ensure_ascii=False)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def evaluate_stage3_qualifications(
    snapshot_path: Path,
    qualifications_path: Path,
    bound_oracle_ids: set[str],
    required_oracle_ids: set[str],
    game_log_path: Path | None = None,
) -> dict[str, Any]:
    """Join card-game-qualifications.json against actual printed faces and optional game logs.

    Separates:
    - declared entries whose printed_faces_sha256 matches the snapshot AND whose card is
      Stage-2 engine-bound in an ordinary base format
    - stale entries whose Oracle text drifted or whose card is no longer bound/required
    - live-verified entries confirmed by actual passing tests in `game_log_path`
    """
    if not qualifications_path.is_file():
        return {
            "ledgerPresent": False,
            "declaredEntries": 0,
            "declaredCompleteMatchingSnapshot": 0,
            "declaredPartialMatchingSnapshot": 0,
            "staleOrDriftedEntries": [],
            "verificationState": "unknown-ledger-missing",
            "verifiedCompleteCards": None,
            "verifiedPartialCards": None,
        }

    entries = json.loads(qualifications_path.read_text(encoding="utf-8"))
    by_oracle: dict[str, dict[str, Any]] = {}
    for entry in entries:
        oid = entry["oracle_id"]
        if oid in by_oracle:
            raise ValueError(f"duplicate qualification entry for oracle_id {oid}")
        by_oracle[oid] = entry

    observed_hashes: dict[str, tuple[str, str]] = {}
    with gzip.open(snapshot_path, "rt", encoding="utf-8") as handle:
        for line in handle:
            raw = json.loads(line)
            oid = raw.get("oracle_id")
            if oid in by_oracle:
                observed_hashes[oid] = (raw["name"], _printed_faces_sha256(raw))

    matching_complete: list[dict[str, Any]] = []
    matching_partial: list[dict[str, Any]] = []
    stale: list[dict[str, Any]] = []

    for oid, entry in sorted(by_oracle.items()):
        observed = observed_hashes.get(oid)
        if observed is None:
            stale.append({"oracleId": oid, "name": entry["name"], "reason": "missing-from-snapshot"})
            continue
        obs_name, obs_sha = observed
        if obs_name != entry["name"] or obs_sha != entry["printed_faces_sha256"]:
            stale.append({"oracleId": oid, "name": entry["name"], "reason": "printed-faces-drifted"})
            continue
        if oid not in required_oracle_ids:
            stale.append({"oracleId": oid, "name": entry["name"], "reason": "not-in-ordinary-base-formats"})
            continue
        if oid not in bound_oracle_ids:
            stale.append({"oracleId": oid, "name": entry["name"], "reason": "not-stage2-engine-bound"})
            continue
        if entry.get("scope") == "complete":
            matching_complete.append(entry)
        else:
            matching_partial.append(entry)

    verification_state = "unverified-in-this-run"
    verified_complete: int | None = None
    verified_partial: int | None = None
    missing_tests: list[str] = []

    if game_log_path is not None and game_log_path.is_file():
        log_text = game_log_path.read_text(encoding="utf-8")
        passed_tests = {
            name.rsplit("::", 1)[-1]
            for name in re.findall(r"^test ([\w:]+) \.\.\. ok$", log_text, re.MULTILINE)
        }
        vc = 0
        vp = 0
        for entry in matching_complete:
            missing = [t for t in entry.get("tests", []) if t not in passed_tests]
            if not missing and entry.get("tests"):
                vc += 1
            else:
                missing_tests.extend(missing)
        for entry in matching_partial:
            missing = [t for t in entry.get("tests", []) if t not in passed_tests]
            if not missing and entry.get("tests"):
                vp += 1
            else:
                missing_tests.extend(missing)
        verified_complete = vc
        verified_partial = vp
        verification_state = (
            "verified-by-game-log" if not missing_tests else "partially-verified-by-game-log"
        )

    return {
        "ledgerPresent": True,
        "ledgerPath": str(qualifications_path.relative_to(ROOT))
        if qualifications_path.is_relative_to(ROOT)
        else str(qualifications_path),
        "declaredEntries": len(entries),
        "declaredCompleteMatchingSnapshot": len(matching_complete),
        "declaredPartialMatchingSnapshot": len(matching_partial),
        "staleOrDriftedEntries": stale,
        "verificationState": verification_state,
        "verifiedCompleteCards": verified_complete,
        "verifiedPartialCards": verified_partial,
        "missingTestsFromGameLog": sorted(set(missing_tests)),
    }


def _resolve_card_programs_command(binary_override: Path | None) -> list[str]:
    if binary_override is not None:
        return [str(binary_override)]
    release_bin = ROOT / "target" / "release" / "spellwake-card-programs"
    if release_bin.is_file():
        return [str(release_bin)]
    debug_bin = ROOT / "target" / "debug" / "spellwake-card-programs"
    if debug_bin.is_file():
        return [str(debug_bin)]
    return ["cargo", "run", "--locked", "-p", "spellwake-card-programs", "--"]


def run_catalog_refresh(
    *,
    metadata_path: Path | None,
    snapshot_path: Path | None,
    fetch_scryfall: bool,
    vocab_path: Path,
    state_dir: Path,
    qualifications_path: Path,
    game_log_path: Path | None,
    output_path: Path,
    badges_dir: Path | None,
    binary_override: Path | None = None,
    force_full_recompile: bool = False,
    bulk_api_url: str = DEFAULT_BULK_API_URL,
) -> dict[str, Any]:
    """Run incremental refresh + honest Stage 1/2/3 & preview-format evaluation."""
    state_dir.mkdir(parents=True, exist_ok=True)
    state_file = state_dir / "refresh-state.json"
    cache_file = state_dir / "support-cache.json"

    previous_state: dict[str, Any] | None = None
    if state_file.is_file():
        previous_state = json.loads(state_file.read_text(encoding="utf-8"))

    downloaded = False
    source_mode = "retained-fixture"
    if fetch_scryfall:
        source_mode = "scryfall-live"
        metadata_path, snapshot_path, downloaded = fetch_or_reuse_scryfall_snapshot(
            state_dir,
            previous_state,
            bulk_api_url=bulk_api_url,
            force_download=force_full_recompile,
        )
    else:
        if metadata_path is None or snapshot_path is None:
            raise ValueError("either --fetch-scryfall or both --metadata and --snapshot are required")

    cmd = _resolve_card_programs_command(binary_override) + [
        "support-report",
        str(metadata_path),
        str(snapshot_path),
        "--creature-vocabulary-input",
        str(vocab_path),
        "--write-state",
        str(state_file),
        "--write-cache",
        str(cache_file),
    ]
    if state_file.is_file():
        cmd.extend(["--previous-state", str(state_file)])
    if cache_file.is_file() and not force_full_recompile:
        cmd.extend(["--cache", str(cache_file)])

    proc = subprocess.run(
        cmd,
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"spellwake-card-programs support-report failed (exit {proc.returncode}): {proc.stderr.strip()}"
        )
    envelope = json.loads(proc.stdout)
    if envelope.get("status") != "success":
        raise RuntimeError(f"support-report returned non-success envelope: {envelope}")
    report = envelope["data"]

    written_cache = json.loads(cache_file.read_text(encoding="utf-8"))
    written_state = json.loads(state_file.read_text(encoding="utf-8"))

    bound_oracle_ids = {
        oid
        for oid, info in written_cache.get("cards", {}).items()
        if info.get("stage2EngineBound") is True
    }
    required_oracle_ids = {
        oid
        for oid, info in written_state.get("cards", {}).items()
        if any(
            status.get("status") in ("legal", "restricted")
            for status in info.get("ordinaryLegalities", {}).values()
        )
    }

    stage3 = evaluate_stage3_qualifications(
        snapshot_path,
        qualifications_path,
        bound_oracle_ids,
        required_oracle_ids,
        game_log_path=game_log_path,
    )

    ordinary = report["ordinaryBaseFormats"]
    preview = report["previewFormats"]
    summary = {
        "schemaVersion": 1,
        "sourceMode": source_mode,
        "downloadedNewSnapshot": downloaded,
        "snapshot": {
            "updatedAt": report["source"]["updated_at"],
            "compressedSize": report["source"]["compressed_size"],
            "compressedSha256": report["compressedSha256"],
            "sourceRecords": ordinary["sourceRecords"],
        },
        "creatureVocabularyId": report["creatureVocabularyId"],
        "compilerRevision": report["compilerRevision"],
        "incremental": {
            "refreshStatus": report["incremental"]["diff"]["refreshStatus"],
            "cacheCompatible": report["incremental"]["cacheCompatible"],
            "reusedCachedCards": report["incremental"]["reusedCachedCards"],
            "recompiledCards": report["incremental"]["recompiledCards"],
            "addedCount": len(report["incremental"]["diff"]["addedOracleIds"]),
            "removedCount": len(report["incremental"]["diff"]["removedOracleIds"]),
            "semanticallyChangedCount": len(
                report["incremental"]["diff"]["semanticallyChangedOracleIds"]
            ),
            "ordinaryLegalityChangedCount": len(
                report["incremental"]["diff"]["ordinaryLegalityChangedOracleIds"]
            ),
            "previewLegalityChangedCount": len(
                report["incremental"]["diff"]["previewLegalityChangedOracleIds"]
            ),
            "unchangedCount": report["incremental"]["diff"]["unchangedOracleIds"],
        },
        "honestCardSupport": {
            "ordinaryBaseFormats": ordinary["formats"],
            "requiredBaseFormatCards": ordinary["requiredRecords"],
            "stage1AstParsed": ordinary["stage1AstParsed"],
            "stage1AstFailed": ordinary["stage1AstFailed"],
            "stage2EngineBound": ordinary["stage2EngineBound"],
            "stage2EngineBlocked": ordinary["stage2EngineBlocked"],
            "stage3GameQualification": stage3,
            "byOrdinaryFormat": ordinary["byFormat"],
            "topBlockers": ordinary["topBlockers"],
        },
        "isolatedPreviewFormats": preview,
        "hostedPublicationRequirements": {
            "publishedToHostedDatabase": False,
            "requiredCredentials": [
                "SPELLWAKE_SUPABASE_URL",
                "SPELLWAKE_SUPABASE_SECRET_KEY",
                "SPELLWAKE_CATALOGUE_BUCKET",
            ],
        },
    }

    badge_svg.write_atomic_json(output_path, summary)

    if badges_dir is not None:
        req = ordinary["requiredRecords"]
        s1 = ordinary["stage1AstParsed"]
        s2 = ordinary["stage2EngineBound"]
        s3_verified = stage3["verifiedCompleteCards"]
        s3_declared = stage3["declaredCompleteMatchingSnapshot"]
        s2_pct = (s2 / req * 100.0) if req else 0.0
        s1_pct = (s1 / req * 100.0) if req else 0.0
        badge_svg.write_badge(
            badges_dir,
            "card-support-ast",
            "Stage 1 AST parsed",
            f"{s1:,} / {req:,} ({s1_pct:.1f}%)",
            "blue",
        )
        badge_svg.write_badge(
            badges_dir,
            "card-support-engine",
            "Stage 2 engine bound",
            f"{s2:,} / {req:,} ({s2_pct:.1f}%)",
            "blue",
        )
        if s3_verified is not None:
            badge_svg.write_badge(
                badges_dir,
                "card-support-qualified",
                "Stage 3 game qualified",
                f"{s3_verified:,} verified / {req:,}",
                "green" if s3_verified > 0 else "lightgrey",
            )
        else:
            badge_svg.write_badge(
                badges_dir,
                "card-support-qualified",
                "Stage 3 game qualified",
                f"{s3_declared:,} declared (unverified run)",
                "lightgrey",
            )
        future_eligible = preview["eligibleByFormat"].get("future", 0)
        preview_only = preview["previewOnlyRecords"]
        badge_svg.write_badge(
            badges_dir,
            "preview-formats",
            "preview formats",
            f"isolated (future: {future_eligible:,}, preview-only: {preview_only:,})",
            "green" if preview["isolatedFromOrdinaryLegality"] else "red",
        )

    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, help="Local saved Scryfall bulk metadata JSON")
    parser.add_argument("--snapshot", type=Path, help="Local saved Scryfall oracle-cards .jsonl.gz")
    parser.add_argument(
        "--fetch-scryfall",
        action="store_true",
        help="Fetch live Scryfall oracle_cards bulk metadata and incrementally download only on change",
    )
    parser.add_argument("--vocab", type=Path, default=DEFAULT_VOCAB_PATH)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--qualifications", type=Path, default=DEFAULT_QUALIFICATIONS_PATH)
    parser.add_argument("--game-log", type=Path, default=None)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--badges-dir", type=Path, default=None)
    parser.add_argument("--binary", type=Path, default=None)
    parser.add_argument("--force-full-recompile", action="store_true")
    parser.add_argument("--bulk-api-url", default=DEFAULT_BULK_API_URL)
    args = parser.parse_args()

    summary = run_catalog_refresh(
        metadata_path=args.metadata,
        snapshot_path=args.snapshot,
        fetch_scryfall=args.fetch_scryfall,
        vocab_path=args.vocab,
        state_dir=args.state_dir,
        qualifications_path=args.qualifications,
        game_log_path=args.game_log,
        output_path=args.output,
        badges_dir=args.badges_dir,
        binary_override=args.binary,
        force_full_recompile=args.force_full_recompile,
        bulk_api_url=args.bulk_api_url,
    )
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
