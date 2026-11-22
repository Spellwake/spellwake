"""Join original Oracle paragraphs to sealed, isolated processor probes."""

import collections
import csv


def clean_reminder(text):
    # Preserve positions in the originals; the separate key reproduces the compiler's reminder elision.
    depth = 0
    masked = []
    cleaned = []
    for c in text:
        if c == "(":
            depth += 1
            cleaned.append(" ")
            masked.append(" ")
        elif c == ")" and depth:
            depth -= 1
            masked.append(" ")
        elif depth:
            masked.append(" ")
        else:
            cleaned.append(c)
            masked.append(c)
    return "".join(masked), "".join(cleaned).strip()


def load_paragraph_probes(cards, path):
    """Index verified TSV rows by exact card, face and reminder-elided paragraph."""
    rows = collections.defaultdict(list)
    with path.open(encoding="utf-8", newline="") as stream:
        for number, item in enumerate(csv.DictReader(stream, delimiter="\t"), 2):
            oid = item["oracle_id"]
            if oid not in cards or item["status"] not in (
                "compiled",
                "unmatched",
                "empty",
            ):
                raise ValueError(
                    f"Unknown identity or probe verdict in {path.name} row {number}"
                )
            key = (oid, item["face_name"], item["clause"])
            rows[key].append(
                {
                    "file": "clauses.tsv",
                    "row": number,
                    "status": item["status"],
                    "context": item["context"],
                    "normalized_text": item["clause"],
                }
            )
    source = {}
    for oid, card in cards.items():
        face_names = collections.Counter(f["name"] for f in card["faces"])
        source[oid] = []
        for face in card["faces"]:
            paragraphs = face.get("oracle_text", "").split("\n")
            keys = [clean_reminder(text)[1] for text in paragraphs]
            occurrences = collections.Counter(keys)
            offset = 0
            face_receipts = []
            for index, (original, key) in enumerate(zip(paragraphs, keys, strict=True)):
                matches = rows.get((oid, face["name"], key), [])
                probe = None
                verdict = "unprobed"
                if matches and (
                    len(matches) != 1
                    or occurrences[key] != 1
                    or face_names[face["name"]] != 1
                ):
                    reason = "Multiple source paragraphs, faces or rows share this key; no unique probe can be assigned."
                elif not matches:
                    reason = "No isolated probe for this exact card, face and parent paragraph."
                elif not key:
                    probe = matches[0]
                    if probe["status"] != "empty":
                        raise ValueError(
                            f"Nonempty probe for blank source paragraph on {oid}"
                        )
                    reason = "The native probe recorded an empty face; it tested no executable paragraph."
                else:
                    probe = matches[0]
                    if probe["status"] == "compiled":
                        verdict = "compiled_in_isolation"
                        reason = "The processor admitted the parent paragraph in the recorded probe context. No individual clause or complete card behavior was independently proved."
                    elif probe["status"] == "unmatched":
                        verdict = "unmatched"
                        reason = "The processor rejected the parent paragraph in the recorded probe context. This does not distinguish a compiler pattern gap from missing native rules."
                    else:
                        raise ValueError(
                            f"Empty probe for nonempty source paragraph on {oid}"
                        )
                face_receipts.append(
                    {
                        "face_index": face["face_index"],
                        "face_name": face["name"],
                        "paragraph_index": index,
                        "paragraph_start": offset,
                        "paragraph_end": offset + len(original),
                        "original_paragraph": original,
                        "verdict": verdict,
                        "reason": reason,
                        "probe": probe,
                    }
                )
                offset += len(original) + 1
            source[oid].append(face_receipts)
    return source


def probe_for_unit(card, unit, probes):
    """Return the containing paragraph's verdict without promoting it to a clause verdict."""
    paragraphs = probes[card["oracle_id"]][unit["face"]]
    start, end = unit["start"], unit["end"]
    matched = [
        p
        for p in paragraphs
        if p["paragraph_start"] <= start <= end <= p["paragraph_end"]
    ]
    if len(matched) == 1:
        parent = matched[0]
        masked, _ = clean_reminder(parent["original_paragraph"])
        first = start - parent["paragraph_start"]
        last = end - parent["paragraph_start"]
        if not masked[first:last].strip():
            relation = "no_operative_text_in_unit"
        elif (masked[:first] + masked[last:]).strip():
            relation = "operative_subspan"
        else:
            relation = "all_operative_text_in_parent"
        return {
            "unit": unit["unit"],
            "unit_relation": relation,
            "scope": "Only the reminder-stripped parent paragraph was eligible for the recorded probe. The verdict does not establish a separate sentence, reminder-based permission, whole-face or gameplay verdict.",
            **parent,
        }
    # A whitespace-only face can produce a single review unit spanning several empty lines.
    face = card["faces"][unit["face"]]
    text = face.get("oracle_text", "")
    if text.strip():
        raise ValueError(
            f"Review unit is not contained in an original paragraph: {card['oracle_id']}"
        )
    return {
        "unit": unit["unit"],
        "face_index": unit["face"],
        "face_name": face["name"],
        "paragraph_index": None,
        "paragraph_start": start,
        "paragraph_end": end,
        "original_paragraph": text[start:end],
        "verdict": "unprobed",
        "unit_relation": "no_operative_text_in_unit",
        "scope": "No individual instruction was probed.",
        "reason": "The blank source unit spans multiple physical Oracle paragraphs.",
        "probe": None,
    }
