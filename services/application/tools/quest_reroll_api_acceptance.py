"""Actual HTTP/Auth/PostgREST rerolls over explicitly synthetic enabled objectives."""
import json
import uuid

from disposable_supabase import DisposableSupabase
from human_api_acceptance import Request, Expect
from qualification_http import object_json


def qualify_quest_reroll_api(realm: DisposableSupabase, request: Request, expect: Expect,
                            owner_token: str, peer_token: str, owner: str, peer: str) -> None:
    """Check atomic replacement, original recovery, ownership and pending-state rules."""
    owner, peer = str(uuid.UUID(owner)), str(uuid.UUID(peer))
    original, candidate = str(uuid.uuid4()), str(uuid.uuid4())
    for definition, enabled in [(original, False), (candidate, True)]:
        realm.sql("INSERT INTO app.quest_definition_revision(id,definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref,reroll_enabled) VALUES('" + definition + "','" + definition + "','fixture:v1','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified'," + ('true' if enabled else 'false') + ")")
    targets = [str(uuid.uuid4()), str(uuid.uuid4())]
    for actor, target in zip([owner, peer], targets):
        realm.sql("INSERT INTO app.quest_assignment(id,account_id,definition_id,source_day,assignment_ref,progress) VALUES('" + target + "','" + actor + "','" + original + "','2026-01-01','fixture:" + target + "',1)")

    def effects() -> str:
        return realm.sql("SELECT jsonb_build_object('assignments',(SELECT jsonb_agg(to_jsonb(q) ORDER BY q.id) FROM app.quest_assignment q WHERE account_id IN('" + owner + "','" + peer + "')),'daily',(SELECT jsonb_agg(to_jsonb(d) ORDER BY d.id) FROM app.daily_quest_state d WHERE account_id IN('" + owner + "','" + peer + "')),'receipts',(SELECT jsonb_agg(jsonb_build_array(id,state,response_payload,normalized_source) ORDER BY id) FROM app.command_receipt WHERE account_id IN('" + owner + "','" + peer + "') AND scope='quest_reroll_intent_v1'))")

    def post(label: str, token: str, intent: dict, lookup: bool = False, status: int = 202) -> dict:
        return expect('quest-reroll-' + label, request('/v1/quests/reroll' + ('/disposition' if lookup else ''), token, intent), status)

    intents = [{'requestId': str(uuid.uuid4()), 'assignmentId': target} for target in targets]
    accepted: list[dict] = []
    for label, actor, token, intent in zip(['owner', 'peer'], [owner, peer], [owner_token, peer_token], intents):
        if post(label + '-unknown', token, intent, True, 200) != {'status': 'success', 'data': {'state': 'unknown', 'response': None}}:
            raise RuntimeError('Unknown reroll lookup changed its protocol')
        result = post(label + '-execute', token, intent)
        data = object_json(result['data'])
        replacement = str(uuid.UUID(data['resourceId']))
        if data != {'resourceId': replacement, 'replacedId': intent['assignmentId']} or replacement == intent['assignmentId']:
            raise RuntimeError('Reroll acknowledgment substituted its original assignment')
        stored = object_json(json.loads(realm.sql("SELECT jsonb_build_object('owner',q.account_id,'definition',q.definition_id,'progress',q.progress::text,'oldReplaced',old.replaced_by_id,'used',d.rerolls_used::text) FROM app.quest_assignment q JOIN app.quest_assignment old ON old.id='" + intent['assignmentId'] + "' JOIN app.daily_quest_state d ON d.account_id=q.account_id AND d.utc_day=q.source_day WHERE q.id='" + replacement + "'")))
        if stored != {'owner': actor, 'definition': candidate, 'progress': '0', 'oldReplaced': replacement, 'used': '1'}:
            raise RuntimeError('HTTP reroll did not atomically store its replacement/allowance')
        before = effects()
        if post(label + '-retry', token, intent) != result:
            raise RuntimeError('Exact reroll retry changed its accepted result')
        if post(label + '-recover', token, intent, True, 200) != {'status': 'success', 'data': {'state': 'accepted', 'response': result}} or effects() != before:
            raise RuntimeError('Reroll recovery repeated selection or changed state')
        for lookup in [False, True]:
            post(label + '-conflict-' + str(lookup), token, dict(intent, assignmentId=str(uuid.uuid4())), lookup, 409)
        if effects() != before:
            raise RuntimeError('Conflicting request changed its stored reroll state')
        accepted.append(result)
    before = effects()
    denied = post('foreign-assignment', peer_token, {'requestId': str(uuid.uuid4()), 'assignmentId': targets[0]}, status=409)
    if denied != {'status': 'fail', 'data': {'assignment': 'unavailable'}}:
        raise RuntimeError('Foreign reroll did not reject ownership')
    # This rejection has a durable failure receipt; it must not replace either target.
    if realm.sql("SELECT count(*) FROM app.quest_assignment WHERE account_id IN('" + owner + "','" + peer + "') AND definition_id='" + candidate + "'") != '2':
        raise RuntimeError('Foreign rejection created a replacement')
    realm.sql("UPDATE app.quest_definition_revision SET reroll_enabled=false WHERE id='" + candidate + "'")
    before = effects()
    for label, token, intent, result in zip(['owner', 'peer'], [owner_token, peer_token], intents, accepted):
        if post(label + '-disabled-pool-retry', token, intent) != result:
            raise RuntimeError('Pool change invalidated historical accepted retry')
    if effects() != before:
        raise RuntimeError('Historical retries mutated the disabled pool/allowance')
    realm.sql("UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id='" + peer + "'")
    if post('pending-recover', peer_token, intents[1], True, 200) != {'status': 'success', 'data': {'state': 'accepted', 'response': accepted[1]}}:
        raise RuntimeError('Pending account could not recover its committed reroll')
    post('pending-new', peer_token, {'requestId': str(uuid.uuid4()), 'assignmentId': str(uuid.uuid4())}, status=409)
