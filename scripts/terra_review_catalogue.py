#!/usr/bin/env python3
"""Prepare an exact, source-pinned catalogue review without model calls."""

import argparse
import csv
import io
import json
import os
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
AUDIT = ROOT / "work/catalogue/card-support-audit-v1"
CURRENT = ROOT / "work/catalogue/definition-test-build-v1"
SECOND = AUDIT / "next-100-v4.1"


def _json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def _historical(cards: dict, H: dict) -> tuple[dict, dict]:
    """Check actual saved reviews and their completed calls, not selection alone."""
    require = H["require"]
    sha = H["sha_file"]
    old_cards = H["load_queue"](AUDIT / "ai-review-input.jsonl")
    first_file = AUDIT / "agent-first-100-manifest.json"
    second_file = SECOND / "manifest.json"
    first = _json(first_file)
    second = _json(second_file)
    require(first["catalogue_queue_sha256"] == sha(AUDIT / "ai-review-input.jsonl"), "first queue mismatch")
    require(second["catalogue_queue_sha256"] == sha(AUDIT / "ai-review-input.jsonl"), "second queue mismatch")
    first_ids = {c["oracle_id"] for c in first["cards"]}
    second_ids = {c["oracle_id"] for c in second["cards"]}
    require(len(first_ids) == len(second_ids) == 100 and not first_ids & second_ids, "historical selection mismatch")
    records = {}
    fingerprints = {str(p.relative_to(ROOT)): sha(p) for p in (first_file, second_file, AUDIT / "ai-review-input.jsonl")}
    for name in ("agent-codex-sample-v3.sqlite3", "agent-first-100.sqlite3", "agent-first-100-high.sqlite3"):
        path = AUDIT / name
        fingerprints[str(path.relative_to(ROOT))] = sha(path)
        with sqlite3.connect(f"file:{path}?mode=ro", uri=True) as db:
            tables = {r[0] for r in db.execute("select name from sqlite_master where type='table'")}
            for oid, value in db.execute("select oracle_id, value from proposals"):
                if oid not in first_ids:
                    continue
                record = json.loads(value)
                require(oid not in records and record["oracle_id"] == oid and record["name"] == old_cards[oid]["name"], "first review identity mismatch")
                require(record["input_sha256"] == H["digest"](H["dumps"](old_cards[oid]).encode()), "first review source mismatch")
                require(record.get("reviews") and record.get("generation"), "first review incomplete")
                if "calls" in tables:
                    for gen in record["generation"]:
                        call = db.execute("select state, members from calls where id=?", (gen["call_id"],)).fetchone()
                        require(call is not None and call[0] == "complete" and oid in json.loads(call[1]), "first review call incomplete")
                records[oid] = ("first-100", f"{name}#proposals/{oid}")
    require(set(records) == first_ids, "first review actual completion coverage mismatch")
    for oid in second_ids:
        path = SECOND / "responses" / f"{oid}.json"
        record = _json(path)
        require(record["oracle_id"] == oid and record["name"] == old_cards[oid]["name"], "second review identity mismatch")
        require(record["input_sha256"] == H["digest"](H["dumps"](old_cards[oid]).encode()) and record.get("reviews"), "second review source mismatch")
        generation = record["generation"]
        require(len(generation) == 1, "second review generation mismatch")
        gen = generation[0]
        attempt_path = SECOND / "attempts" / f"{gen['attempt']:03d}.json"
        attempt = _json(attempt_path)
        require(attempt["status"] in ("complete", "partial") and oid in attempt["accepted"] and attempt["packet_sha256"] == gen["request_sha256"], "second review call incomplete")
        fingerprints[str(path.relative_to(ROOT))] = sha(path)
        fingerprints[str(attempt_path.relative_to(ROOT))] = sha(attempt_path)
        records[oid] = ("next-100", str(path.relative_to(ROOT)))
    require(set(records) <= cards.keys(), "historical cards missing from current queue")
    for oid in records:
        require(old_cards[oid] == cards[oid], f"current queue changed historic source: {oid}")
    return records, fingerprints


def prepare(output: Path, H: dict) -> tuple[dict, tuple, list, dict]:
    """Prepare all cards and frozen evidence, atomically saving the membership manifest."""
    output.mkdir(parents=True, exist_ok=True)
    args = argparse.Namespace(input=CURRENT / "ai-review-input.jsonl", rules=H["RULE_FILE"], evidence_dir=CURRENT)
    require = H["require"]
    cards = H["load_queue"](args.input)
    require(len(cards) == 32155, "unexpected current catalogue size")
    source = H["frozen_source"](args)
    source["named_games_sha256"] = H["sha_file"](CURRENT / "named-games.json")
    historical, prior_hashes = _historical(cards, H)
    source["prior_review_sha256"] = prior_hashes
    indexes = H["indices"](args.rules)
    indexes[1].card_evidence = H["measured_card_evidence"](args, cards, source)
    indexes[1].paragraph_probes = H["load_paragraph_probes"](cards, CURRENT / "clauses.tsv")
    require(H["frozen_source"](args) == {k: v for k, v in source.items() if k not in ("named_games_sha256", "prior_review_sha256")}, "source changed during preparation")
    require(H["sha_file"](CURRENT / "named-games.json") == source["named_games_sha256"], "named evidence changed during preparation")
    remaining = [oid for oid in cards if oid not in historical]
    stream = io.StringIO(newline="")
    writer = csv.writer(stream, lineterminator="\n")
    writer.writerow(("oracle_id", "name", "support", "status", "prior_batch", "provenance"))
    for oid, card in cards.items():
        prior, provenance = historical.get(oid, ("", ""))
        writer.writerow((oid, card["name"], indexes[1].card_evidence[oid]["support"], "previously_reviewed" if prior else "remaining", prior, provenance))
    path = output / "manifest.csv"
    content = stream.getvalue().encode()
    if path.exists():
        require(path.read_bytes() == content, "existing manifest differs; refusing to overwrite")
    else:
        temporary = path.with_suffix(".csv.part")
        with temporary.open("xb") as f:
            f.write(content)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temporary, path)
    source["manifest_sha256"] = H["sha_file"](path)
    return cards, indexes, remaining, source
