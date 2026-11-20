"""Restore a hash-checked publisher plan through its canonical private SQL operations."""
import hashlib
import json
from pathlib import Path
from uuid import UUID

from qualify_published_card_sources import file_hash, require, semantic_hash


def quote(value):
    return "'" + value.replace("'", "''") + "'"


def batches(stage):
    body = bytearray(b"[")
    size = 0
    prefix = b'{"state":"supported","metadata":'
    with Path(stage).open("rb") as stream:
        for line in stream:
            if not line.startswith(prefix):
                require(json.loads(line)["state"] == "unresolved", "unknown stage row")
                continue
            require(line.endswith(b"}\n"), "unterminated staged record")
            record = line[len(prefix):-2]
            if size == 500 or len(body) + len(record) + 2 > 384 * 1024:
                yield bytes(body) + b"]"
                body = bytearray(b"[")
                size = 0
            if size:
                body.extend(b",")
            body.extend(record)
            size += 1
    if size:
        yield bytes(body) + b"]"


def restore_catalogue(realm, plan_path, artifact_hash):
    plan = json.loads(plan_path.read_bytes())
    terms, stage = plan["terms"], plan["stage"]
    require(terms["artifactHash"] == artifact_hash == file_hash(stage["source"]["artifactPath"]), "foreign bulk input")
    require(terms["metadataHash"] == file_hash(stage["source"]["metadataPath"]), "changed source metadata")
    require(terms["stageHash"] == file_hash(stage["recordsPath"]), "changed records stage")
    def execute(statements):
        return realm.sql("SET standard_conforming_strings=on; SET ROLE service_role;\n" + "\n".join(statements))
    pub = str(UUID(execute(["SELECT app.begin_catalogue_publication(" + quote(json.dumps(terms, separators=(",", ":"))) + "::jsonb);"])))
    statements = []
    count = 0
    for body, expected in zip(batches(stage["recordsPath"]), terms["batchHashes"], strict=True):
        require("sha256:" + hashlib.sha256(body).hexdigest() == expected, "publisher batch digest differs")
        count += 1
        statements.append(f"SELECT app.append_catalogue_batch('{pub}',{count},{quote(body.decode())});")
        if len(statements) == 8:
            require(len([UUID(value) for value in execute(statements).splitlines()]) == len(statements), "missing batch receipt")
            statements.clear()
    if statements:
        require(len([UUID(value) for value in execute(statements).splitlines()]) == len(statements), "missing final batch receipt")
    require(count == terms["batchCount"], "catalogue batch count")
    sql = f"SELECT app.finish_catalogue_publication('{pub}');"
    require(execute([sql]) == execute([sql]) == pub, "publication finalization or retry")
    return dict(publicationId=pub, sourceHash=artifact_hash, stageHash=terms["stageHash"], batches=count,
                definitionCount=terms["definitionCount"], printings=terms["printingCount"])


def selected_card(realm, publication, release_id, candidate, oracle):
    pub = str(UUID(publication["publicationId"]))
    binding, = [item for item in candidate["bindings"] if item["oracleRef"] == oracle]
    printing = str(UUID(binding["sourcePrinting"]))
    # The original printing and revision must both come from this committed publication.
    row = json.loads(realm.sql(f"""
      SELECT jsonb_build_object('definition',d.id,'revision',r.id,'printing',p.id,
        'definitionRevisionRef',r.revision_ref,'rulesTextRef',r.rules_text_ref)
      FROM app.card_definition d JOIN app.card_definition_revision r ON r.definition_id=d.id
      JOIN app.printing p ON p.definition_id=d.id
      WHERE d.oracle_ref='{oracle}' AND r.catalogue_receipt_id='{pub}' AND p.external_printing_ref='{printing}'
        AND p.catalogue_receipt_id='{pub}' AND app.catalogue_source_visible(r.catalogue_receipt_id)
      ORDER BY (p.finish='nonfoil') DESC,p.id LIMIT 1"""))
    refs = binding["sourceRefs"]
    require(row["definitionRevisionRef"] == "catalogue:" + pub, "foreign definition revision")
    require(row["rulesTextRef"] == f"prices/scryfall/{publication['sourceHash'][7:]}.jsonl.gz#oracle={oracle}", "foreign rules locator")
    row["programRef"] = semantic_hash(["native-published-program-v1", candidate["programIdentity"], oracle, release_id,
        row["definition"], row["revision"], row["definitionRevisionRef"], row["rulesTextRef"],
        refs["definitionRevisionRef"], refs["rulesTextRef"], refs["programRef"]])
    return row
