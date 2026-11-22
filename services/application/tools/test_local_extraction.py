"""Exercise real local Storage -> native extraction -> PostgREST observations.

The owner seeds a synthetic historical manifest, never an accepted live sample.
Only UUID-owned fixtures are removed; no database/project reset is performed.
"""

from datetime import datetime, timedelta, timezone
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.error
import urllib.request
import uuid
from typing import Callable

from test_local_storage import BASE, NoRedirect, ROOT

CONTAINER = "supabase_db_spellwake-application-local"


def literal(value: str) -> str:
    """Quote fixture-only SQL values without shell interpolation."""
    return "'" + value.replace("'", "''") + "'"


def sql(statement: str) -> str:
    """Use only the explicitly named owned local database container."""
    result = subprocess.run(
        ["docker", "exec", "-i", CONTAINER, "psql", "-U", "postgres", "-d", "postgres",
         "-X", "-qAt", "-v", "ON_ERROR_STOP=1"],
        input=statement, capture_output=True, text=True,
    )
    if result.returncode:
        raise RuntimeError("Local extraction fixture SQL failed: " + result.stderr[-2000:])
    return result.stdout.strip()


def qualify(durable: Callable[[str, dict[str, str]], str] | None = None,
            settle: Callable[[list[str], dict[str, str]], None] | None = None) -> None:
    """Seed, execute and remove an explicitly synthetic extraction fixture."""
    if 'project_id = "spellwake-application-local"' not in (ROOT / "supabase/config.toml").read_text():
        raise RuntimeError("Refusing an unrecognized local project")
    state = json.loads((ROOT / "target/local-supabase-status.json").read_text())
    if state.get("API_URL") != BASE or not state.get("SECRET_KEY", "").startswith("sb_secret_"):
        raise RuntimeError("Refusing a nonlocal or unsupported credential target")
    if sql("SELECT current_database()") != "postgres":
        raise RuntimeError("Unexpected local database")
    # extract(slot) discovers every eligible market. This qualification therefore
    # requires an empty application, rather than borrowing a populated project.
    if sql("SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM app.prediction_market)+(SELECT count(*) FROM app.price_observation)") != "0":
        raise RuntimeError("Extraction qualification requires an empty owned application")
    now = datetime.now(timezone.utc)
    slot = (now - timedelta(days=1)).date().isoformat()
    if sql("SELECT app.read_established_price_slot(" + literal(slot) + "::date) IS NULL") != "t":
        raise RuntimeError("Refusing to replace an existing source slot")
    before = sql("SELECT count(*) FROM app.price_observation")
    identifiers = [str(uuid.uuid4()) for _ in range(13)]
    actor, subject, set_id, definition, present, absent, foil, source, external, missing_external, *markets = identifiers
    # Four markets: present, absent, unavailable finish, and a later terminal.
    markets.append(str(uuid.uuid4()))
    q = literal
    acquired = slot + "T12:10:00Z"
    updated = slot + "T11:00:00Z"
    uri = "https://data.scryfall.io/all-cards/all-cards-" + slot.replace("-", "") + "110000.jsonl.gz"
    card = {"object": "card", "id": external, "finishes": ["nonfoil"], "prices": {"usd": "1.23", "usd_foil": None}}
    artifact = gzip.compress((json.dumps(card) + "\n").encode(), mtime=0)
    metadata = json.dumps({"object": "list", "has_more": False, "fixture_id": source, "data": [{
        "object": "bulk_data", "id": "922288cb-4bef-45e1-bb30-0c2bd3d3534f", "type": "all_cards",
        "updated_at": updated, "jsonl_download_uri": uri, "compressed_size": len(artifact),
    }]}).encode()
    digest, metadata_digest = hashlib.sha256(artifact).hexdigest(), hashlib.sha256(metadata).hexdigest()
    artifact_ref = "prices/scryfall/" + digest + ".jsonl.gz"
    metadata_ref = "prices/scryfall/metadata/" + metadata_digest + ".json"
    manifest = {"version": "scryfall-slot-v1", "slot": slot, "state": "valid", "source": "scryfall:all_cards",
                "hash": "sha256:" + digest, "metadataHash": "sha256:" + metadata_digest,
                "artifactRef": artifact_ref, "metadataRef": metadata_ref, "sourceUpdatedAt": updated,
                "acquiredAt": acquired, "recordCount": "1", "downloadUri": uri}
    opener = urllib.request.build_opener(NoRedirect())

    def request(path: str, method: str = "GET", body: bytes | None = None, kind: str = "application/json") -> object:
        headers = {"apikey": state["SECRET_KEY"], "Content-Type": kind, "x-upsert": "false"}
        try:
            with opener.open(urllib.request.Request(BASE + path, method=method, data=body, headers=headers), timeout=30) as response:
                payload = response.read(1048577)
                if len(payload) > 1048576:
                    raise RuntimeError("Oversized local fixture response")
                return json.loads(payload) if payload else None
        except urllib.error.HTTPError as error:
            raise RuntimeError("Local fixture HTTP failed: " + str(error.code)) from None

    bucket = request("/storage/v1/bucket/spellwake-evidence")
    if not isinstance(bucket, dict) or bucket.get("public") is not False:
        raise RuntimeError("Private local bucket required; run test_local_storage.py first")
    environment = os.environ.copy()
    environment.update({"SPELLWAKE_SUPABASE_URL": BASE, "SPELLWAKE_SUPABASE_SECRET_KEY": state["SECRET_KEY"],
                        "SPELLWAKE_PRICE_BUCKET": "spellwake-evidence", "SPELLWAKE_PRICE_STAGE": str(ROOT / "target/local-price-stage")})

    def activity(*arguments: str, success: bool = True) -> object:
        result = subprocess.run([str(ROOT / "target/release/price_collect"), *arguments], env=environment, capture_output=True, text=True)
        payload = json.loads(result.stdout)
        if (result.returncode == 0) != success or payload.get("status") != ("success" if success else "error"):
            raise RuntimeError("Unexpected native extraction activity result")
        return payload.get("data")

    uploaded: list[str] = []
    try:
        for key, content, kind in [(metadata_ref, metadata, "application/json"), (artifact_ref, artifact, "application/gzip")]:
            request("/storage/v1/object/spellwake-evidence/" + key, "POST", content, kind)
            # Only acknowledged creates belong to this run. A conflicting key
            # must never become ours to delete; unknown ACKs need inspection.
            uploaded.append(key)
        manifest_json = q(json.dumps(manifest)) + "::jsonb"
        statements = [
            "BEGIN; SET LOCAL session_replication_role=replica;",
            "INSERT INTO auth.users(id) VALUES(" + q(subject) + ");",
            "INSERT INTO app.account(id,auth_subject,username,sign_in_method,created_at,state) VALUES(" +
            ",".join(map(q, [actor, subject, "fixture_" + actor.replace("-", "")[:12], "email_code"])) + ",now(),'active');",
            "INSERT INTO app.card_set(id,external_set_ref,name) VALUES(" + ",".join(map(q, [set_id, set_id, "Synthetic fixture"])) + ");",
            "INSERT INTO app.card_definition(id,oracle_ref,name) VALUES(" + ",".join(map(q, [definition, definition, "Synthetic fixture"])) + ");",
            "INSERT INTO app.command_receipt(id,principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref) VALUES(" +
            q(source) + ",'collector:scryfall-v1'," + q(slot) + ",'price_slot'," + q(slot) +
            ",encode(sha256(convert_to((" + manifest_json + ")::text,'UTF8')),'hex'),'accepted',(" + manifest_json + ")::text);",
        ]
        for printing, reference, finish in [(present, external, "nonfoil"), (absent, missing_external, "nonfoil"), (foil, external, "foil")]:
            statements.append("INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES(" +
                              ",".join(map(q, [printing, definition, set_id, reference, "rare", finish, "fixture-only"])) + ");")
        for index, (market, printing) in enumerate(zip(markets, [present, absent, foil, present], strict=True)):
            terminal = ((now - timedelta(days=1)).date() + timedelta(days=2 if index == 3 else 0)).isoformat()
            statements.append("INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) VALUES(" +
                              ",".join(map(q, [market, actor, printing, "scryfall:usd_foil" if index == 2 else "scryfall:usd", "1.00", "at_deadline" if index == 3 else "reached_by"])) +
                              "," + q(terminal + "T13:00:00Z") + "," + q(slot + "T11:00:00Z") + "," + q(slot + "T12:00:00Z") +
                              ",'scryfall-daily-12-13-utc-v1'," + q(terminal) + ",'scryfall-sampled-gte-v1',100,1," + q("open" if index == 0 else "closed") + ");")
        statements.append("COMMIT;")
        sql("\n".join(statements))
        if durable is None:
            result = activity("extract", slot)
            if not isinstance(result, dict) or result.get("processed") != 3 or result.get("more") is not False:
                raise RuntimeError("Expected exactly three historical extractions")
        elif durable(slot, environment) != source:
            raise RuntimeError("Durable workflow returned another source manifest")
        actual = json.loads(sql("SELECT coalesce(jsonb_object_agg(market_id::text,jsonb_build_array(state,price_usd_decimal)),'{}') FROM app.price_observation WHERE manifest_id=" + q(source)))
        expected = {markets[0]: ["valid", "1.23"], markets[1]: ["missing", None], markets[2]: ["invalid", None]}
        if actual != expected:
            raise RuntimeError("Stored observations differ from exact selected-file evidence")
        retry = activity("extract", slot)
        if not isinstance(retry, dict) or retry.get("processed") != 0:
            raise RuntimeError("Extraction retry repeated completed work")
        fresh = datetime.fromisoformat(acquired.replace("Z", "+00:00")) >= datetime.now(timezone.utc) - timedelta(hours=24)
        if fresh:
            probe = activity("probe", slot, external)
            # Prove cache reuse without retained objects, rather than re-reading them.
            request("/storage/v1/object/spellwake-evidence", "DELETE", json.dumps({"prefixes": uploaded}).encode())
            if activity("probe", slot, external) != probe:
                raise RuntimeError("Established probe identity changed on retry")
        else:
            activity("probe", slot, external, success=False)
        if settle is not None:
            settle(markets, environment)
        print("Real local extraction passed: valid/missing/invalid, terminal exclusion, retry; probe " + ("creation/reuse" if fresh else "stale rejection") + ".")
    finally:
        # Explicit owner-only fixture teardown. Runtime RPCs never disable guards.
        sql("BEGIN; SET LOCAL session_replication_role=replica; " +
            "DELETE FROM app.ledger_posting WHERE operation_id IN (SELECT id FROM app.ledger_operation WHERE source_scope='prediction_settlement' AND source_ref IN (" + ",".join(map(q, markets)) + ")); " +
            "DELETE FROM app.price_observation WHERE manifest_id=" + q(source) + " OR market_id IN (" + ",".join(map(q, markets)) + "); " +
            "DELETE FROM app.prediction_market WHERE id IN (" + ",".join(map(q, markets)) + "); " +
            "DELETE FROM app.ledger_operation WHERE source_scope='prediction_settlement' AND source_ref IN (" + ",".join(map(q, markets)) + "); " +
            "DELETE FROM app.command_receipt WHERE id=" + q(source) + " OR (principal_key='collector:scryfall-v1' AND scope='price_probe' AND result_ref::jsonb->>'manifestId'=" + q(source) + "); " +
            "DELETE FROM app.printing WHERE id IN (" + ",".join(map(q, [present, absent, foil])) + "); " +
            "DELETE FROM app.card_definition WHERE id=" + q(definition) + "; DELETE FROM app.card_set WHERE id=" + q(set_id) + "; " +
            "DELETE FROM app.account WHERE id=" + q(actor) + "; DELETE FROM auth.users WHERE id=" + q(subject) + "; COMMIT;")
        if uploaded:
            request("/storage/v1/object/spellwake-evidence", "DELETE", json.dumps({"prefixes": uploaded}).encode())
            if sql("SELECT count(*) FROM storage.objects WHERE bucket_id='spellwake-evidence' AND name IN (" + ",".join(map(q, uploaded)) + ")") != "0":
                raise RuntimeError("Fixture objects survived cleanup")
        if sql("SELECT count(*) FROM app.price_observation") != before:
            raise RuntimeError("Fixture cleanup did not restore observation count")
        if sql("SELECT app.read_established_price_slot(" + q(slot) + "::date) IS NULL") != "t":
            raise RuntimeError("Synthetic source receipt survived cleanup")


def main() -> None:
    """Serialize with the other owned Storage qualification harness."""
    with (ROOT / "target/local-storage-test.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        qualify()


if __name__ == "__main__":
    main()
