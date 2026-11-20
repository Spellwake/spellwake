"""Resolve canonical English Oracle witnesses against both retained bulk sources."""
import argparse
import gzip
import hashlib
import json
import sys
from pathlib import Path
from uuid import UUID


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def file_hash(path):
    with Path(path).open("rb") as stream:
        return "sha256:" + hashlib.file_digest(stream, "sha256").hexdigest()


def load(path):
    return json.loads(Path(path).read_bytes())


def semantic_hash(value):
    # Same array and ordered CatalogFace fields as NativeProgramSource::card_source_refs.
    data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
    return "sha256:" + hashlib.sha256(data).hexdigest()


def selected_records(path, ids, compressed):
    selected = {}
    decoded_bytes = count = 0
    with (gzip.open if compressed else open)(path, "rb") as stream:
        for raw in stream:
            count += 1
            decoded_bytes += len(raw)
            row = json.loads(raw)
            if not compressed:
                if row["state"] != "supported":
                    continue
                row = row["metadata"]
            key = row["id" if compressed else "externalPrintingRef"]
            if key in ids:
                require(key not in selected, "duplicate selected printing")
                selected[key] = row
    require(selected.keys() == ids, "selected printing missing")
    return selected, count, decoded_bytes


def projection(card):
    fields = ("name", "type_line", "oracle_text", "mana_cost", "power", "toughness")
    return [{field: face.get(field) for field in fields} for face in card.get("card_faces") or [card]]


def witness(plan, receipt, candidate, oracle_path):
    terms = plan["terms"]
    stage = plan["stage"]
    source = stage["source"]
    pub = receipt["catalogue"]
    require(receipt["passed"] and receipt["cleanupVerified"], "unsuccessful publication")
    UUID(pub["publicationId"])
    for field in ("stageHash", "supportedRecords", "unresolvedRecords", "printingCount", "batchCount"):
        require(terms[field] == pub[field], "publication mismatch: " + field)
    require(pub["sourceHash"] == terms["artifactHash"] == file_hash(source["artifactPath"]), "artifact hash")
    require(terms["metadataHash"] == file_hash(source["metadataPath"]), "metadata hash")
    require(terms["stageHash"] == file_hash(stage["recordsPath"]), "stage hash")
    require(terms["artifactRef"] == "prices/scryfall/" + terms["artifactHash"][7:] + ".jsonl.gz", "locator")
    require(file_hash(oracle_path) == candidate["descriptor"]["snapshot"]["sha256"], "Oracle snapshot hash")
    cards = candidate["bindings"]
    ids = {card["sourcePrinting"] for card in cards}
    require(cards and len(ids) == len(cards) == len({c["oracleRef"] for c in cards}), "ambiguous selection")
    canonical, _, _ = selected_records(oracle_path, ids, True)
    published, count, size = selected_records(source["artifactPath"], ids, True)
    require((count, size) == (terms["recordCount"], terms["decompressedBytes"]), "incomplete artifact")
    projected, count, size = selected_records(stage["recordsPath"], ids, False)
    require((count, size) == (terms["recordCount"], terms["stageBytes"]), "incomplete publication stage")
    rows = []
    for binding in sorted(cards, key=lambda c: c["oracleRef"]):
        key, oracle = binding["sourcePrinting"], binding["oracleRef"]
        a, b, meta = canonical[key], published[key], projected[key]
        require(a["lang"] == b["lang"] == meta["language"] == "en", "canonical language")
        require(a["oracle_id"] == b["oracle_id"] == meta["oracleRef"] == oracle, "Oracle identity")
        require(a["name"] == b["name"] == meta["name"] == binding["name"], "name")
        require(a["layout"] == b["layout"] == meta["layout"], "layout")
        faces = projection(b)
        require(faces == projection(a) and [f["name"] for f in faces] == meta["faceNames"], "faces")
        formats = ("standard", "pioneer", "modern", "legacy", "vintage")
        require(all(a["legalities"][f] == b["legalities"][f] for f in formats), "format legality")
        definition = semantic_hash(["native-definition-v1", oracle, b["name"], faces])
        rules = semantic_hash(["native-rules-text-v1", oracle, [[f["name"], f["oracle_text"]] for f in faces]])
        refs = binding["sourceRefs"]
        require((definition, rules) == (refs["definitionRevisionRef"], refs["rulesTextRef"]), "native semantic hashes")
        rows.append(dict(oracleRef=oracle, printing=key, definitionHash=definition, rulesHash=rules))
    return dict(schema="native-published-card-witness-v1", artifactHash=terms["artifactHash"],
                oracleSnapshotHash=candidate["descriptor"]["snapshot"]["sha256"], cards=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("plan", "publication", "candidate", "oracle"):
        parser.add_argument(arg, type=Path)
    args = parser.parse_args()
    result = witness(load(args.plan), load(args.publication), load(args.candidate), args.oracle)
    json.dump(result, sys.stdout, indent=2)
    print()


if __name__ == "__main__":
    main()
