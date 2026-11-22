"""Join the pinned processor run, isolated Oracle abilities and card-specific game receipts."""
import argparse
from contextlib import contextmanager
import csv
import gzip
import hashlib
import run_card_support_audit
import json
import re
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

FORMATS = ("commander", "standard", "pioneer", "modern", "legacy", "vintage")
NUMBER = re.compile(r"\b(?:one|two|three|four|five|six|seven|eight|nine|ten|\d+)\b")
MANA = re.compile(r"(?:\{(?:[WUBRGC]|\d+)\})+")
NAMED_GAMES = [
    ("sol_ring_and_llanowar_elves_pay_their_prices_and_produce_printed_mana",
     {"Sol Ring":"Paid one generic mana, resolved, tapped for two colorless mana and rejected a second activation while tapped",
      "Llanowar Elves":"Paid green, resolved, rejected immediate tap for mana, then tapped for green on a later turn"}),
    ("paid_source_boosts_stack_and_expire_and_a_campus_can_pay_to_scry",
     {"Shivan Dragon":"Paid two separate red activations, checked 5→6→7 power and expiry back to 5",
      "Quandrix Campus":"Rejected underpayment, accepted the printed activation and presented the scry decision"}),
    ("named_ordinary_permanents_settle_their_full_entry_and_death_triggers",
     {n:"Paid cast and specific printed trigger outcome checked" for n in ("Cathedral Sanctifier","Cloudkin Seer","Feral Prowler","Ironshell Beetle","Attended Knight","Wailing Ghoul")}),
]


def read_table(path, delimiter=","):
    with path.open(newline="", encoding="utf-8") as file:
        return list(csv.DictReader(file, delimiter=delimiter))


@contextmanager
def publish(path):
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="", dir=path.parent, delete=False) as file:
        temporary = Path(file.name)
        try:
            yield file
            file.close()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)


def write_table(path, rows):
    with publish(path) as file:
        writer = csv.DictWriter(file, fieldnames=rows[0])
        writer.writeheader()
        writer.writerows(rows)


def load_registry(path):
    result = json.loads(path.read_bytes())
    assert result["status"] == "success", path
    result = result["data"]
    cards = {c["oracleId"]: c for c in result["cards"]}
    assert result["requiredRecords"] == len(cards) == len(result["cards"]) == 32155
    assert all(c["required"] for c in cards.values())
    assert len(admitted(cards)) == result["requiredPartialBindings"]
    return result, cards


def admitted(rows):
    return {oid for oid, row in rows.items() if row["binding"]["status"] != "blocked"}


def strip_reminder(text):
    parts = []
    depth = 0
    for char in text:
        if char == "(":
            depth += 1
        elif char == ")" and depth:
            depth -= 1
        elif not depth:
            parts.append(char)
    return "".join(parts)


def template(text, name):
    # Keep targets, effects, conditions, types and all printed paragraphs intact.
    text = strip_reminder(text.replace(name, "CARDNAME"))
    text = MANA.sub("{mana}", text)
    return NUMBER.sub("N", " ".join(text.split()))


def types(face):
    return face["type_line"].partition(" — ")[0].split()


def family(clause, face):
    line = clause.casefold()
    if re.search(r"\b(?:enters|enter the battlefield)\b", line) and re.search(r"\b(?:when|whenever)\b", line):
        return "Enters-the-battlefield triggers"
    if re.search(r"\b(?:when|whenever)\b.*\b(?:dies|leaves the battlefield)\b", line):
        return "Death and leaves-the-battlefield triggers"
    if "enters tapped" in line or ("enters" in line and " counter" in line and " with " in line):
        return "Entry conditions and replacement effects"
    if re.search(r"^[+−–-]?(?:\d+|x):", line):
        return "Loyalty abilities"
    if re.search(r":\s*(?:add |.*\badd one mana)", line):
        return "Activated mana production and costs"
    if line.startswith(("equip ", "enchant ", "equipped ", "enchanted ")):
        return "Auras and Equipment"
    if "choose one" in line or line.startswith("•"):
        return "Modal choices and their effects"
    if re.search(r"\b(?:when|whenever|at the beginning|at the end of combat)\b", line):
        return "Other triggered abilities"
    if ":" in line:
        return "Activated abilities and costs"
    if "any target" in line:
        return "Effects that allow any target"
    if "search your library" in line:
        return "Searches and their selection filters"
    if any(t in types(face) for t in ("Instant", "Sorcery")):
        return "Instant and sorcery effects and target restrictions"
    return "Static abilities, keywords and other permanent text"


def structures(raw, diagnostic):
    gaps = []
    if raw["layout"] != "normal":
        gaps.append(f"Layout: {raw['layout']}")
    for face in raw.get("card_faces", [raw]):
        mana = face.get("mana_cost", "")
        if mana and not MANA.fullmatch(mana):
            gaps.append("Variable, hybrid, snow or alternative mana symbols")
        if "Planeswalker" in types(face):
            gaps.append("Planeswalker characteristics and loyalty")
        if "Creature" in types(face) and any(
            not re.fullmatch(r"-?\d+", face.get(k, "") or "") for k in ("power", "toughness")
        ):
            gaps.append("Variable printed power or toughness")
    if diagnostic == "requires-other-subtype":
        gaps.append("Subtype vocabulary or mapping")
    return list(dict.fromkeys(gaps))


def prepare_review(folder, catalogue, current, identities):
    review = []
    with gzip.open(catalogue, "rb") as file:
        for line_num, line in enumerate(file,1):
            raw = json.loads(line)
            oid = raw.get("oracle_id")
            if oid not in identities:
                continue
            name = raw["name"]
            faces = raw.get("card_faces", [raw])
            review_faces = [{
                "face_index": index, **{key: face[key] for key in
                ("name","type_line","mana_cost","oracle_text","power","toughness","loyalty","defense","color_indicator","colors")
                if key in face}
            } for index,face in enumerate(faces)]
            review.append({
                "oracle_id": oid, "name": name, "layout": raw["layout"], "printing_id": raw["id"],
                "catalogue_line": line_num, "catalogue_sha256": current["sourceSha256"],
                "source_line_sha256": hashlib.sha256(line).hexdigest(),
                "color_identity": raw.get("color_identity",[]),
                "associated_parts": raw.get("all_parts", []), "faces": review_faces,
            })
    assert len(review) == len({r["oracle_id"] for r in review}) == len(identities)
    with publish(folder/"ai-review-input.jsonl") as output:
        for card in sorted(review,key=lambda r:r["oracle_id"]):
            output.write(json.dumps(card,ensure_ascii=False,separators=(",",":"))+"\n")


def write_summary(folder, stats, clause_rank):
    counts = stats["status"]
    game_log = "qualifying-games.log" if (folder / "qualifying-games.log").is_file() else "verified-games.log"
    ledger = "rule-ledger/" if (folder / "rule-ledger").is_dir() else "../card-support-audit-v1/rule-ledger/"
    pilot = "agent-pilot-report.md" if (folder / "agent-pilot-report.md").is_file() else "../card-support-audit-v1/agent-pilot-report.md"
    ranked = sorted(clause_rank,key=lambda r:(-r["cards_with_only_this_unmatched_pattern"],-r["all_cards_with_this_unmatched_pattern"],r["type"],r["remaining_ability_pattern"]))
    lines = [
        "# Spellwake card support on the October 29 Oracle snapshot", "",
        "This report follows 32,155 distinct Oracle identities legal or restricted in Commander, Standard, Pioneer, Modern, Legacy or Vintage. The snapshot contained 39,196 records before format selection. Each required identity appears once. All counts refer to the local rules engine.", "",
        "| Whole-card status | Cards | Evidence |", "|---|---:|---|",
        f"| Fully supported | {counts['Fully supported']:,} | A source-specific game exercised the complete printed behavior covered by the audit. |",
        f"| Partially supported | {counts['Partially supported']:,} | An executable definition, an earlier scoped implementation, or an independently compiled printed ability exists. The row identifies the remaining work or testing. |",
        f"| Unsupported | {counts['Unsupported']:,} | The whole card was rejected and the audit verified no isolated printed paragraph or earlier executable definition. Shared game machinery may still help implement it. |", "",
        f"The current processor can load **{stats['current_executable']:,} cards**. Of the partially supported cards, {stats['partially_supported_with_current_program']:,} have a current definition and {stats['partially_supported_without_current_program']:,} still fail whole-card compilation. {stats['previous_profile_only']} cards execute in an earlier restricted pool only. [All card results](cards.csv) · [Exact executable definitions](programs.jsonl)", "",
        "## Work completed", "",
        f"The processor grew from {stats['current_executable_before']:,} to {stats['current_executable']:,} executable identities. All previous identities remain. Each addition passed an actual paid cast and resolution or land play. Named games cover conditional land entry, color selection, repeatable boosts, cleanup and printed mana output. [Newly executable cards](newly-executable.csv) · [Gameplay results]({game_log})", "",
        "| Compiler change | Newly executable cards |", "|---|---:|",
        *[f"| {name} | {total:,} |" for name,total in sorted(stats['newly_executable_by_change'].items(), key=lambda entry:-entry[1])], "",
        "Giant Growth now changes its target’s power and toughness by exactly +3/+3 until cleanup, when the change expires. Sol Ring costs one, taps for two colorless mana, and rejects a second activation while tapped. Sengir Vampire’s flying compiles by itself. Its damage-linked death trigger still prevents the card from loading. Lightning Bolt has a creature-only implementation in an older pool. Its printed target domain remains open.", "",
        "## Remaining work by potential card impact", "",
        "These counts use rejected full cards. The first column of counts requires a single distinct unmatched paragraph shape and no separately recognized layout, stat, subtype or cost blocker. Numeric values and ordinary mana costs are normalized. A card can still require integration or have an undetected dependency. Only a successful processor rerun establishes an actual unlock.", "",
        "| Remaining printed pattern | Only unmatched pattern | All affected |", "|---|---:|---:|",
    ]
    for r in ranked[:10]:
        text=r["remaining_ability_pattern"].replace("|"," / ")
        lines.append(f"| {text} | {r['cards_with_only_this_unmatched_pattern']:,} | {r['all_cards_with_this_unmatched_pattern']:,} |")
    lines += ["", "[All ranked ability patterns](next-unlocks.csv) · [Complete recurring card shapes](remaining-templates.csv) · [Broader rules work](remaining-gaps.csv)", "",
        "## How to read the evidence", "",
        "The card CSV includes original faces and text, confirmed shared paragraphs, unresolved paragraphs, processor diagnostics, older restricted definitions, and the line containing its compiled engine instructions when available. For multi-face cards the paragraph probe keeps the chosen face’s name, cost, type and stats and assesses it as a normal face. Passing this probe gives evidence for that paragraph in isolation. The actual layout and cross-face behavior still need a whole-card game.", "",
        "Full support here is a positive result for the card’s printed mechanics in the tested scenarios. Global rules coverage, multiplayer permutations, deployment and independent Forge comparison are separate evidence. Missing tests on a compiled card are labeled explicitly. Earlier comprehensive-rules reports used a different complete-card qualification field, which the registry alone cannot evaluate.", "",
        "## Card and rule lookup", "",
        f"The dated ledger preserves every face and paragraph, its original text offsets, rule references, implementation evidence, remaining requirements and affected cards. The dated run indexes 61,454 printed paragraphs and 3,321 Comprehensive Rules entries. A requirement may represent a printed paragraph, a cost or a shared game operation. It also lists rules whose implementation is still unassessed. [Read the map]({ledger}README.md) · [Requirement status]({ledger}requirement-status.csv) · [Rules and evidence]({ledger}rule-evidence.csv)", "",
        "## Supporting-agent inventory", "",
        f"Every required card has a review packet with its exact original faces, printing ID, source-line hash, and the pinned catalogue hash. This includes the cards already passing. Agent proposals and their scenarios must be reconciled with engine execution before a support classification changes. [All 32,155 review packets](ai-review-input.jsonl) · [Measured model controls and review examples]({pilot})", "",
    ]
    if (folder/"agent-100-reviews.csv").is_file():
        lines.extend([
            "The first 100 reviews pair each proposed behavior and game scenario with the card’s measured support and its source text. The independent review checks where the agent linked a rule to an engine operation with different behavior. [First 100 card reviews](agent-100-reviews.csv) · [Independent quality findings](agent-100-quality.md) · [Model settings and usage by call](agent-first-100-usage.json)", "",
        ])
    lines += [
        "## Reproduction", "",
        "The baseline and updated CLI results preserve the same Scryfall archive hash and creature vocabulary. Run the card-programs CLI with the retained catalogue metadata, gzip, vocabulary and the keyword-gameplay-current profile. The replay harness runs the CLI and card-specific games against one source tree. It seals the source, input and output hashes. The report join checks the sealed evidence and verifies identities, complete source coverage, and prior definitions. [Replay harness](../../../scripts/run_card_support_audit.py) · [Run receipt](run-receipt.json). [Baseline](before.json) · [Updated processor output](after.json) · [Totals](summary.json)", "",
    ]
    if (folder/"verification.md").is_file():
        lines.extend(["[Workspace checks and retained evidence](verification.md)", ""])
    with publish(folder/"report.md") as file:
        file.write("\n".join(lines))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--catalogue", type=Path, required=True)
    parser.add_argument("--older-bindings", type=Path, required=True)
    parser.add_argument("--prepare-review-only", action="store_true")
    args = parser.parse_args()
    folder = args.directory
    base, before = load_registry(folder / "before.json")
    current, after = load_registry(folder / "after.json")
    assert before.keys() == after.keys()
    old, new = admitted(before), admitted(after)
    _, intermediate = load_registry(folder / "before-mana.json")
    intermediate_ids = admitted(intermediate)
    assert old <= intermediate_ids <= new
    gained = new-old
    assert old <= new
    assert base["creatureVocabularyId"] == current["creatureVocabularyId"]
    assert base["sourceSha256"] == current["sourceSha256"] == hashlib.sha256(args.catalogue.read_bytes()).hexdigest()
    prepare_review(folder, args.catalogue, current, after)
    if args.prepare_review_only:
        return
    receipt = run_card_support_audit.verify(folder)
    card_games = {}
    if "named-games.json" in receipt["artifacts"]:
        results = json.loads((folder / "named-games.json").read_text())
        confirmed = run_card_support_audit.named_evidence(results, (folder / "qualifying-games.log").read_text())
        card_games = {game["oracle_id"]: game for game in confirmed}
    for oid in old:
        assert before[oid]["binding"]["semanticSha256"] == after[oid]["binding"]["semanticSha256"]
    special = defaultdict(set)
    for registry in json.loads(args.older_bindings.read_bytes()):
        assert registry["source_sha256"] == current["sourceSha256"]
        for entry in registry["cards"]:
            if entry["required"] and entry["binding"]["status"] == "partial":
                special[entry["oracle_id"]].add(entry["binding"]["scope"])
    assert special.keys() <= after.keys()
    ordinary = {r["oracle_id"]: r for r in read_table(folder / "ordinary-cards.tsv", "\t")}
    spells = {r["oracle_id"]: r for r in read_table(folder / "ordinary-spells.tsv", "\t")}
    assert not ordinary.keys() & spells.keys()
    assert spells.keys() <= new
    for oid, row in ordinary.items():
        assert row["catalogue_sha256"] == current["sourceSha256"]
        if row["status"] in ("verified", "partial"):
            assert oid in new and row["semantic_sha256"] == after[oid]["binding"]["semanticSha256"]
        assert row["status"] != "failed"
    program_line = {}
    with (folder/"programs.jsonl").open() as file:
        for line_num, line in enumerate(file,1):
            program = json.loads(line)
            assert program["oracle_id"] not in program_line
            program_line[program["oracle_id"]] = line_num
    assert program_line.keys() == new
    # The aggregate smoke test enumerates the actual difference with the frozen predecessor.
    log_file = "qualifying-games.log" if card_games else "verified-games.log"
    log = (folder/log_file).read_text()
    assert "every_current_binding_casts_and_reaches_its_source_zone ... ok" in log
    assert "0 failed" in log
    _, frozen = load_registry(folder.parent / "keyword-wave-v6/registry.json")
    new_since_frozen = new-admitted(frozen)
    named = {}
    for test, details in NAMED_GAMES:
        assert test + " ... ok" in log
        named.update({name: f"{message} [{log_file}: {test}]" for name,message in details.items()})
    probes = defaultdict(list)
    for row in read_table(folder / "clauses.tsv", "\t"):
        probes[row["oracle_id"]].append(row)
    rows = []
    added = []
    categories = defaultdict(list)
    only = defaultdict(list)
    whole_shapes = defaultdict(list)
    clause_hits = defaultdict(list)
    sole_clauses = defaultdict(list)
    with gzip.open(args.catalogue, "rt") as file:
        for line_num, line in enumerate(file, 1):
            raw = json.loads(line)
            oid = raw.get("oracle_id")
            legal = [fmt for fmt in FORMATS if raw["legalities"][fmt] in ("legal", "restricted")]
            if not legal:
                assert oid not in after
                continue
            assert oid in after and raw["name"] == after[oid]["name"]
            name = raw["name"]
            binding = after[oid]["binding"]
            diagnostic = binding.get("reason", "")
            faces = raw.get("card_faces", [raw])
            audit = ordinary.get(oid, {})
            support = "Partially supported"
            evidence = audit.get("evidence_or_reason", "") if audit.get("status") in ("verified", "partial") else ""
            checks = ""
            shared, residual = [], []
            gaps = set()
            if oid in spells or audit.get("status") == "verified":
                support = "Fully supported"
                evidence = spells[oid]["evidence"] if oid in spells else evidence
                checks = "Complete printed behavior passed its card-specific game"
            elif oid in new and name == "Sol Ring" and raw["type_line"] == "Artifact" and raw["mana_cost"] == "{1}" and raw["oracle_text"] == "{T}: Add {C}{C}.":
                support = "Fully supported"
                evidence = named[name]
                checks = "Paid cast and complete printed mana ability passed in the card-specific game"
            elif oid in new:
                if not evidence:
                    if name in named:
                        evidence = named[name]
                    elif oid in new_since_frozen:
                        evidence = ("Land play and battlefield entry" if "Land" in types(faces[0]) and name!="Zoetic Cavern" else "Paid cast, resolution and expected zone") + f" [{log_file}: every_current_binding_casts_and_reaches_its_source_zone]"
                    else:
                        evidence = "Source-guarded executable definition; predecessor game suites are indexed by profile"
                checks = "Executable definition; complete printed behavior still needs verification"
                if audit.get("status") == "partial":
                    checks = evidence
                elif audit:
                    checks += ". Audit scope: " + audit["evidence_or_reason"]
            else:
                structural_gaps = structures(raw, diagnostic)
                gaps.update(structural_gaps)
                unresolved = set()
                context = {face["name"]: face for face in faces}
                for clause in probes[oid]:
                    face = context[clause["face_name"]]
                    value = clause["clause"]
                    label = f"{face['name']}: {value}"
                    if clause["status"] == "compiled":
                        shared.append(label if raw["layout"] == "normal" else label + " [isolated as a normal face]")
                    elif value:
                        residual.append(label)
                        gaps.add(family(value, face))
                        unresolved.add((face["type_line"].partition(" — ")[0], template(value,face["name"])))
                if not shared and oid not in special:
                    support = "Unsupported"
                if oid in special:
                    evidence = "Earlier executable scope: " + ", ".join(sorted(special[oid]))
                elif shared:
                    evidence = ("Reminder-stripped Oracle paragraphs compile and validate using the original face’s name, cost, types and stats"
                                + (". Original layout and cross-face behavior remain unverified" if raw["layout"] != "normal" else ""))
                checks = "Current processor rejected the complete card"
                if not gaps:
                    gaps.add("Assemble recognized abilities or represent remaining face characteristics")
                if not shared and not residual:
                    checks += ". No stand-alone printed ability verified for this face"
                if residual:
                    checks += ". Outstanding text includes " + residual[0]
                if structural_gaps:
                    checks += ". Additional work: " + ", ".join(structural_gaps)
                for key in unresolved:
                    clause_hits[key].append(name)
                if len(unresolved) == 1 and not structural_gaps:
                    sole_clauses[next(iter(unresolved))].append(name)
                groups = sorted(gaps)
                for gap in groups:
                    categories[gap].append(name)
                if len(groups) == 1:
                    only[groups[0]].append(name)
                whole = " // ".join(face["type_line"].partition(" — ")[0] + " | " + (template(face.get("oracle_text",""),face["name"]) or "(empty rules box)") for face in faces)
                whole_shapes[(raw["layout"], whole)].append(name)
            printed_faces = json.dumps([{k: f[k] for k in ("name", "type_line", "mana_cost", "oracle_text", "power", "toughness", "loyalty", "defense", "color_indicator") if k in f} for f in faces], ensure_ascii=False)
            game = card_games.get(oid)
            if game and game["printed_faces_sha256"] == hashlib.sha256(printed_faces.encode()).hexdigest():
                if oid not in new or game["name"] != name:
                    raise ValueError("Named gameplay does not match the executable card: " + name)
                evidence = game["evidence"] + " [qualifying-games.log: " + ", ".join(game["tests"]) + "]"
                if game["scope"] == "complete":
                    support = "Fully supported"
                    checks = "Complete printed behavior passed the named gameplay checks"
                else:
                    support = "Partially supported"
                    checks = game["evidence"]
            if oid in gained:
                if oid not in intermediate_ids and any(
                    re.search(r"(?m)^\{T\}: Add\b", face.get("oracle_text", "")) for face in faces
                ):
                    change = "Tap-for-mana permanents"
                elif "Land" in types(faces[0]):
                    change = "Lands"
                elif any(t in types(faces[0]) for t in ("Instant","Sorcery")):
                    change = "Instants and sorceries"
                elif any(line.startswith(("When ","Whenever ")) for line in faces[0].get("oracle_text","").splitlines()):
                    change = "Triggered permanents"
                elif "Enchantment" in types(faces[0]) and ":" not in faces[0].get("oracle_text", ""):
                    change = "Static enchantments"
                else:
                    change = "Self-modifying activated abilities"
                added.append({"change": change, "name": name, "oracle_id": oid, "type": raw["type_line"], "support": support, "was_available_in_earlier_profile": bool(special.get(oid)), "oracle_text": " // ".join(f.get("oracle_text","") for f in faces)})
            rows.append({
                "name": name, "oracle_id": oid, "support": support,
                "current_processor": "Executable" if oid in new else "Blocked",
                "reason": checks, "behavior_evidence": evidence,
                "verified_shared_abilities": "\n".join(shared), "unverified_or_rejected_clauses": "\n".join(residual),
                "remaining_work_families": " | ".join(sorted(gaps)), "current_processor_diagnostic": diagnostic,
                "compiled_instructions": f"programs.jsonl line {program_line[oid]}" if oid in new else "",
                "earlier_profile_scopes": " | ".join(sorted(special.get(oid, []))),
                "newly_executable": oid in new and oid not in old, "formats": " | ".join(legal),
                "layout": raw["layout"], "type": raw["type_line"],
                "complete_printed_faces": printed_faces,
                "catalogue_line": line_num, "semantic_sha256": binding.get("semanticSha256", ""),
            })
    assert len(rows) == len({r["oracle_id"] for r in rows}) == len(after)
    assert len(added) == len(new-old)
    assert probes.keys() == after.keys()-new, (len(probes),len(after.keys()-new))
    sort = lambda rows: sorted(rows, key=lambda r: (r["name"].casefold(),r["oracle_id"]))
    write_table(folder / "cards.csv", sort(rows))
    write_table(folder / "newly-executable.csv", sort(added))
    gaps = [
        {"work_family": g, "only_identified_work_family": len(only[g]), "all_affected_cards": len(names),
         "examples_with_only_this_family": " | ".join(sorted(only[g])[:8])}
        for g,names in categories.items()
    ]
    write_table(folder / "remaining-gaps.csv", sorted(gaps, key=lambda r: (-r["only_identified_work_family"],r["work_family"])))
    clause_rank = [
        {"type": k[0], "remaining_ability_pattern": k[1], "cards_with_only_this_unmatched_pattern": len(sole_clauses[k]),
         "all_cards_with_this_unmatched_pattern": len(names), "examples_with_only_this_pattern": " | ".join(sorted(sole_clauses[k])[:10])}
        for k,names in clause_hits.items()
    ]
    write_table(folder / "next-unlocks.csv", sorted(clause_rank, key=lambda r: (-r["cards_with_only_this_unmatched_pattern"], -r["all_cards_with_this_unmatched_pattern"], r["type"], r["remaining_ability_pattern"])))
    patterns = [
        {"layout": layout, "whole_card_text_shape": shape, "blocked_cards": len(names),
         "examples": " | ".join(sorted(names)[:8])} for (layout,shape),names in whole_shapes.items()
    ]
    write_table(folder / "remaining-templates.csv", sorted(patterns,key=lambda r:(-r["blocked_cards"],r["layout"],r["whole_card_text_shape"])))
    stats = dict(required=len(rows), status=dict(Counter(r["support"] for r in rows)),
        current_executable_before=len(old),current_executable=len(new), newly_executable=len(added),
        newly_executable_by_change=dict(Counter(r["change"] for r in added)),
        newly_executable_types=dict(Counter(r["type"].split(" — ")[0] for r in added)),
        previous_profile_only=len(special.keys()-new), current_blocked=len(after)-len(new),
        partially_supported_with_current_program=sum(r["support"]=="Partially supported" and r["current_processor"]=="Executable" for r in rows),
        partially_supported_without_current_program=sum(r["support"]=="Partially supported" and r["current_processor"]=="Blocked" for r in rows),
        source_sha256=current["sourceSha256"],creature_vocabulary_id=current["creatureVocabularyId"])
    with publish(folder/"summary.json") as file:
        file.write(json.dumps(stats,indent=2)+"\n")
    write_summary(folder,stats,clause_rank)


if __name__ == "__main__":
    main()
