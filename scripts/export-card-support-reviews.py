"""Join a completed AI sample to the independently measured whole-card coverage."""

import argparse
import csv
import hashlib
import json
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def source_links(references):
    links = []
    for ref in references:
        path = Path(ref["file"])
        if path.is_relative_to(ROOT):
            path = path.relative_to(ROOT)
        links.append(f"{ref['id']} ({path}:{ref['line']})")
    return " | ".join(links)


def file_digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_proposals(databases, queue_hash):
    proposals = {}
    for path in databases:
        connection = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)
        try:
            config = json.loads(connection.execute("SELECT value FROM manifest WHERE id=1").fetchone()[0])
            if config["queue_sha256"] != queue_hash:
                raise ValueError(f"Review queue differs in {path}")
            for identity, value in connection.execute("SELECT oracle_id, value FROM proposals"):
                record = json.loads(value)
                if identity in proposals or record["oracle_id"] != identity:
                    raise ValueError(f"Unexpected or duplicate review identity {identity}")
                proposals[identity] = (record, config)
        finally:
            connection.close()
    return proposals


def unit_rows(source, record):
    if record["source"] != source or record["evidence"] != "unverified-model-proposal":
        raise ValueError(f"Source or evidence classification differs for {source['name']}")
    units = record["units"]
    reviews = record["reviews"]
    references = record["references"]
    if len(units) != len(reviews) or len(units) != len(references):
        raise ValueError(f"Incomplete reviews for {source['name']}")
    positions = [0] * len(source["faces"])
    seen = set()
    for number, (unit, review, refs) in enumerate(zip(units, reviews, references, strict=True)):
        index, start, end = unit["face"], unit["start"], unit["end"]
        text = source["faces"][index].get("oracle_text", "")
        if (unit["unit"] != number or review["unit"] != number or refs["unit"] != number or
                not positions[index] <= start <= end <= len(text) or
                text[start:end] != unit["text"] or text[positions[index]:start].strip()):
            raise ValueError(f"Invalid source span for {source['name']}, unit {number}")
        positions[index] = end
        seen.add(index)
        scoped = {}
        if "paragraph_probes" in record:
            probes = record["paragraph_probes"]
            if (
                len(probes) != len(units)
                or probes[number]["unit"] != number
                or probes[number]["face_index"] != index
            ):
                raise ValueError(
                    f"Scoped paragraph receipts differ for {source['name']}, unit {number}"
                )
            parent = probes[number]
            if (
                text[parent["paragraph_start"] : parent["paragraph_end"]]
                != parent["original_paragraph"]
            ):
                raise ValueError(
                    f"Scoped paragraph source differs for {source['name']}, unit {number}"
                )
            receipt = parent["probe"]
            scoped = {
                "measured_paragraph_probe": parent["verdict"],
                "probe_unit_relation": parent["unit_relation"],
                "probed_parent_paragraph": parent["original_paragraph"],
                "probe_scope": parent["scope"],
                "probe_context": receipt["context"] if receipt else "",
                "probe_source": f"{receipt['file']} row {receipt['row']}"
                if receipt
                else "",
            }
        yield {
            **scoped,
            "face": source["faces"][index]["name"], "face_index": index, "unit": number,
            "paragraph": unit["paragraph"], "start": start, "end": end, "oracle_text": unit["text"],
            "proposed_behavior": review["behavior"], "proposed_scenario": review["scenario"],
            **({"binding_basis": review["binding_basis"]} if "binding_basis" in review else {}),
            "agent_assessment": review["disposition"], "suggested_rules": source_links(refs["rules"]),
            "suggested_engine_symbols": source_links(refs["mechanics"]),
            "suggested_missing_operations": " | ".join(review["missing_primitives"]),
        }
    if seen != set(range(len(positions))) or any(
        face.get("oracle_text", "")[pos:].strip() for face, pos in zip(source["faces"], positions, strict=True)
    ):
        raise ValueError(f"Missing face or printed text for {source['name']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("sample", "coverage", "queue", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--independent-review", type=Path, help="Card-level findings keyed by Oracle ID")
    parser.add_argument("databases", type=Path, nargs="+")
    args = parser.parse_args()
    sample = json.loads(args.sample.read_text())
    queue_hash = file_digest(args.queue)
    if sample["catalogue_queue_sha256"] != queue_hash or sample["coverage_csv_sha256"] != file_digest(args.coverage):
        raise ValueError("Selected sample and coverage inputs differ")
    ids = [card["oracle_id"] for card in sample["cards"]]
    proposals = read_proposals(args.databases, queue_hash)
    if len(set(ids)) != len(ids) or set(ids) != proposals.keys():
        raise ValueError("Completed reviews must exactly match the selected sample")
    independent = json.loads(args.independent_review.read_text()) if args.independent_review else {}
    if independent.keys() - proposals.keys():
        raise ValueError("Independent review contains cards outside the selected sample")
    with args.queue.open() as stream:
        sources = {card["oracle_id"]: card for card in map(json.loads, stream) if card["oracle_id"] in proposals}
    with args.coverage.open(newline="") as stream:
        measured = {r["oracle_id"]: r for r in csv.DictReader(stream) if r["oracle_id"] in proposals}
    rows = []
    for position, identity in enumerate(ids, 1):
        source = sources[identity]
        record, config = proposals[identity]
        card = measured[identity]
        verdict = independent.get(identity)
        if card["support"] != sample["cards"][position - 1]["support_at_selection"]:
            raise ValueError(f"Support classification changed for {card['name']}")
        common = {
            "sample_position": position, "name": source["name"], "oracle_id": identity,
            "measured_card_support": card["support"], "measured_reason": card["reason"],
            "measured_evidence": card["behavior_evidence"], "compiled_instructions": card["compiled_instructions"],
            "verified_shared_abilities": card["verified_shared_abilities"],
            "unresolved_oracle_text": card["unverified_or_rejected_clauses"],
            "requested_model": config["model"], "requested_reasoning": config.get("reasoning_effort", "unrecorded"),
            "independent_card_review": verdict["assessment"] if verdict else "Outside independent quality sample",
            "independent_finding": verdict["finding"] if verdict else "",
        }
        if "paragraph_probes" in record and card["current_processor"] == "Blocked":
            common["verified_shared_abilities"] = ""
            if common["measured_evidence"].startswith(
                "Reminder-stripped Oracle paragraphs compile"
            ):
                common["measured_evidence"] = (
                    "See the measured paragraph probe for each source unit."
                )
        rows.extend(common | row for row in unit_rows(source, record))
    with args.output.open("w", newline="", encoding="utf-8") as stream:
        writer = csv.DictWriter(stream, fieldnames=dict.fromkeys(key for row in rows for key in row))
        writer.writeheader()
        writer.writerows(rows)
    print(f"Verified {len(ids)} cards, {sum(len(c['faces']) for c in sources.values())} faces and {len(rows)} source units")


if __name__ == "__main__":
    main()
