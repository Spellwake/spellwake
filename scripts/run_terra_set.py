#!/usr/bin/env python3
"""Review a pinned set with bounded concurrent Terra workers and resumable per-card results."""
import argparse
from collections import Counter, deque
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from contextlib import closing
import csv
import fcntl
import gzip
import json
from pathlib import Path
import runpy
import sqlite3
import time

from terra_frozen_source import frozen_index
from terra_review_catalogue import prepare
from terra_review_packets import build_packet, validate_card

ROOT = Path(__file__).resolve().parents[1]
HARNESS = runpy.run_path(str(ROOT / "scripts/card-support-agent.py"))
MODEL = "gpt-6.1-terra"
EVIDENCE = ROOT / "work/catalogue/definition-test-build-v1"


def write_json(path, value):
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    tmp.replace(path)


def packed_bytes(value):
    return gzip.compress(HARNESS["dumps"](value).encode(), compresslevel=2, mtime=0)


def groups_for(ids, cards, capacity, unit_limit):
    """Keep whole cards together, including unusually long single cards."""
    group, count = [], 0
    for oid in ids:
        size = len(HARNESS["units"](cards[oid]))
        if group and (len(group) >= capacity or count + size > unit_limit):
            yield group
            group, count = [], 0
        group.append(oid)
        count += size
    if group:
        yield group


def review(ids, cards, indexes, config):
    group = [(cards[oid], HARNESS["units"](cards[oid])) for oid in ids]
    system, prompt, schema = build_packet(group, indexes, HARNESS, config)
    request = packed_bytes(dict(system=system, prompt=prompt, schema=schema))
    meter = {}
    options = argparse.Namespace(**vars(config))
    options.record_usage = lambda usage, meta, size: meter.update(usage=usage, metadata=meta, size=size)
    accepted, rejected, error, raw = {}, {}, "", ""
    try:
        result = HARNESS["codex_call"](options, system, prompt, schema)
        raw = result["message"]["content"]
        answers = json.loads(raw, object_pairs_hook=HARNESS["unique_keys"])
        if not isinstance(answers, dict) or set(answers) - set(ids):
            raise ValueError("Unexpected card identities in response")
        for card, block in group:
            oid = card["oracle_id"]
            try:
                accepted[oid] = validate_card(card, block, answers[oid], indexes, HARNESS, config)
            except (KeyError, ValueError, TypeError) as exc:
                rejected[oid] = str(exc)
    except HARNESS["ProviderUnavailable"] as exc:
        error = str(exc)
    except (KeyError, ValueError, TypeError) as exc:
        rejected = dict.fromkeys(ids, str(exc))
    return request, raw, accepted, rejected, meter, error


def open_store(path):
    db = sqlite3.connect(path)
    db.execute("PRAGMA journal_mode=WAL")
    db.executescript("""
        CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS attempts (
          id INTEGER PRIMARY KEY, ids TEXT NOT NULL, started REAL NOT NULL, finished REAL,
          request BLOB, response BLOB, usage TEXT, rejected TEXT, error TEXT
        );
        CREATE TABLE IF NOT EXISTS reviews (id TEXT PRIMARY KEY, attempt INTEGER NOT NULL, answer BLOB NOT NULL);
    """)
    return db


def export(folder, db, ids, cards, indexes):
    valid = {oid: json.loads(gzip.decompress(raw)) for oid, raw in db.execute("SELECT id,answer FROM reviews")}
    fields = ("card", "oracle_id", "engine_support", "current_processor", "terra_review",
              "implementation_plan", "proposed_missing_rules", "acceptance_scenario", "observed_game")
    tmp = folder / "card-results.csv.tmp"
    with tmp.open("w", newline="") as stream:
        out = csv.DictWriter(stream, fields)
        out.writeheader()
        for oid in ids:
            ev = indexes[1].card_evidence[oid]
            answer = valid.get(oid, {})
            out.writerow(dict(card=cards[oid]["name"], oracle_id=oid, engine_support=ev["support"],
                              current_processor=ev["current_processor"], terra_review="reviewed" if answer else "pending",
                              implementation_plan=answer.get("implementation_plan", ""),
                              proposed_missing_rules=" | ".join(answer.get("missing_shared_rules", [])),
                              acceptance_scenario=answer.get("acceptance_scenario", ""), observed_game=ev.get("behavior_evidence", "")))
    tmp.replace(folder / "card-results.csv")


def run(config, folder, db):
    saved = db.execute("SELECT value FROM metadata WHERE key='identity'").fetchone()
    pins = json.loads(saved[0])["source"]["semantic_source_sha256"] if saved else None
    with frozen_index(HARNESS, pins, EVIDENCE/"builder-source.tar.gz"):
        cards, indexes, _, source = prepare(folder.parent/"terra-full-v1", HARNESS)
    membership = json.loads((folder/"set.json").read_bytes())
    ids = [c["oracle_id"] for c in membership["cards"]]
    if len(ids) != len(set(ids)) or set(ids) - cards.keys():
        raise ValueError("Set list contains a duplicate or a card outside the pinned catalogue")
    # Pin the actual evidence and prompts across restarts. Keep worker count adjustable.
    identity = dict(source=source, set_sha=HARNESS["sha_file"](folder/"set.json"), model=config.model,
                    effort=config.effort, top=config.top, packets_sha=HARNESS["sha_file"](ROOT/"scripts/terra_review_packets.py"))
    encoded = HARNESS["dumps"](identity)
    db.execute("INSERT OR IGNORE INTO metadata VALUES ('identity',?)", (encoded,))
    if db.execute("SELECT value FROM metadata WHERE key='identity'").fetchone()[0] != encoded:
        raise ValueError("Existing review uses different inputs, model, or packet contract")
    db.commit()
    done_ids = {x[0] for x in db.execute("SELECT id FROM reviews")}
    todo = [oid for oid in ids if oid not in done_ids]
    # Sort by source family to share retrieval and contract text within each packet.
    todo.sort(key=lambda oid: (cards[oid]["layout"], cards[oid]["faces"][0].get("type_line","").split(" — ")[0],
                               cards[oid]["faces"][0].get("oracle_text","").replace(cards[oid]["name"],"~"),oid))
    queue = deque(groups_for(todo, cards, config.cards_per_call, config.units_per_call))
    provider = HARNESS["codex_preflight"](config)
    first = db.execute("SELECT min(started) FROM attempts").fetchone()[0] or time.time()
    print(json.dumps(dict(set_cards=len(ids), packets=len(queue), resumed=len(done_ids), workers=config.workers, **provider)),flush=True)
    export(folder, db, ids, cards, indexes)
    pending, failures, usage, stop = {}, Counter(), Counter(), ""
    for raw, in db.execute("SELECT usage FROM attempts WHERE usage IS NOT NULL"):
        usage.update(json.loads(raw).get("usage",{}))
    for raw, in db.execute("SELECT rejected FROM attempts WHERE rejected IS NOT NULL"):
        failures.update(json.loads(raw).keys())

    def report():
        count = len(done_ids)
        checkpoint = db.execute("SELECT count(*) FROM reviews JOIN attempts ON reviews.attempt=attempts.id WHERE finished<=?", (first+600,)).fetchone()[0]
        write_json(folder/"progress.json",dict(model=config.model, reasoning=config.effort, source_cards=len(ids),
            accepted=count, remaining=len(ids)-count, attempts=db.execute("SELECT count(*) FROM attempts").fetchone()[0],
            active=len(pending), elapsed_seconds=round(time.time()-first,1), accepted_within_first_600_seconds=checkpoint,
            registered_at=first, updated_at=time.time(), stop=stop, usage=dict(usage)))

    with ThreadPoolExecutor(max_workers=config.workers) as pool:
        while queue or pending:
            # Start a small first wave to catch account-wide or request-schema errors early.
            window = config.workers if done_ids else min(4, config.workers)
            while queue and len(pending) < window and not stop:
                group = queue.popleft()
                row = db.execute("INSERT INTO attempts(ids,started) VALUES (?,?)", (json.dumps(group), time.time())).lastrowid
                db.commit()
                pending[pool.submit(review, group, cards, indexes, config)] = row, group
            report()
            if not pending:
                break
            finished, _ = wait(pending, timeout=15, return_when=FIRST_COMPLETED)
            for future in finished:
                row, group = pending.pop(future)
                req, raw, valid, rejected, meter, error = future.result()
                with db:
                    db.execute("UPDATE attempts SET finished=?,request=?,response=?,usage=?,rejected=?,error=? WHERE id=?",
                               (time.time(), req, packed_bytes(raw), json.dumps(meter), json.dumps(rejected), error, row))
                    for oid, answer in valid.items():
                        db.execute("INSERT INTO reviews VALUES(?,?,?)", (oid, row, packed_bytes(answer)))
                done_ids.update(valid)
                usage.update(meter.get("usage",{}))
                failures.update(rejected.keys())
                queue.extend([oid] for oid in rejected if failures[oid] < 3)
                if error:
                    stop = error
                print(json.dumps(dict(attempt=row, accepted=len(valid), cumulative=len(done_ids), rejected=rejected, error=error)),flush=True)
                export(folder, db, ids, cards, indexes)
    report()
    export(folder, db, ids, cards, indexes)
    db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    return int(len(done_ids)!=len(ids))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    parser.add_argument("--workers", type=int, default=24)
    parser.add_argument("--cards-per-call", type=int, default=4)
    parser.add_argument("--units-per-call", type=int, default=16)
    args = parser.parse_args()
    if min(args.workers, args.cards_per_call, args.units_per_call) <= 0:
        parser.error("packet and worker limits must be positive")
    args.model, args.effort, args.timeout, args.top = MODEL, "high", 300, 4
    with (args.folder / ".run.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with closing(open_store(args.folder/"reviews.sqlite3")) as database:
            raise SystemExit(run(args, args.folder, database))
