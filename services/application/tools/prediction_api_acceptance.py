"""Actual prediction HTTP transactions over explicitly synthetic retained evidence.

The historical daily receipt fixture bypasses ordinary triggers for that isolated
insert, as the existing database publication tests do. It is not an acquired Scryfall feed
or collector acceptance. All public reads/publication/matching/cancellation use
the actual HTTP process, current Auth and guarded canonical database functions.
"""
import json
import uuid
from typing import Callable
from human_api_acceptance import Request, Expect
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def qualify_prediction_api(realm: DisposableSupabase, request: Request, expect: Expect,
                           owner_token: str, peer_token: str, owner: str, peer: str,
                           printing: str,
                           prepare_runtime: Callable[[str, str], None] | None = None) -> None:
    """Check publication, matched fixed backing and cancellation of unfilled units."""
    printing = str(uuid.UUID(printing))
    actors = "('" + owner + "','" + peer + "')"

    def post(label: str, body: dict, token: str = owner_token, lookup: bool = False, status: int = 202) -> dict:
        path = '/v1/predictions/commands' + ('/disposition' if lookup else '')
        return expect('prediction-' + label, request(path, token, body), status)

    def get(label: str, path: str, token: str = owner_token, status: int = 200) -> dict:
        return expect('prediction-' + label, request(path, token, None), status)

    def resource(payload: dict) -> str:
        if payload.get('status') != 'success':
            raise RuntimeError('Prediction HTTP expected standard success')
        return str(uuid.UUID(object_json(payload['data'])['resourceId']))

    def literal(value: object) -> str:
        return "'" + json.dumps(value, ensure_ascii=False).replace("'", "''") + "'::jsonb"

    def effects() -> str:
        return realm.sql("SELECT jsonb_build_object("
            "'orders',(SELECT jsonb_agg(jsonb_build_array(id,state,quantity::text,filled_quantity::text,hold_id) ORDER BY id) FROM app.prediction_order),"
            "'pairs',(SELECT jsonb_agg(jsonb_build_array(id,quantity::text,yes_actual_cost_coins::text,no_actual_cost_coins::text,fixed_payout_coins::text,match_operation_id) ORDER BY id) FROM app.prediction_pair),"
            "'markets',(SELECT jsonb_agg(jsonb_build_array(id,state,next_order_sequence::text,orders_revision::text) ORDER BY id) FROM app.prediction_market),"
            "'inventories',(SELECT jsonb_agg(jsonb_build_array(i.id,i.owned_quantity::text,v.held::text,i.revision::text) ORDER BY i.id) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_availability v ON v.inventory_asset_id=i.id WHERE o.account_id IN " + actors + "),"
            "'operations',(SELECT jsonb_agg(jsonb_build_array(id,state) ORDER BY id) FROM app.ledger_operation WHERE actor_account_id IN " + actors + "),"
            "'postings',(SELECT jsonb_agg(jsonb_build_array(p.id,p.direction,p.quantity::text) ORDER BY p.id) FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id IN " + actors + "))")

    def recover(label: str, body: dict, accepted: dict, token: str = owner_token) -> None:
        before = effects()
        result = post(label, body, token, True, 200)
        if object_json(result['data']) != {'state': 'accepted', 'response': accepted} or effects() != before:
            raise RuntimeError('Prediction disposition changed its original result or financial state')

    def repeat(label: str, body: dict, accepted: dict, token: str = owner_token) -> None:
        before = effects()
        if post(label, body, token) != accepted or effects() != before:
            raise RuntimeError('Prediction retry published, matched or moved held coins twice')

    def held(actor: str) -> int:
        return int(realm.sql("SELECT v.held::text FROM app.asset_availability v JOIN app.inventory_asset i ON i.id=v.inventory_asset_id JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE o.account_id='" + actor + "' AND d.currency_key='coin'"))

    source_path = '/v1/predictions/printings/' + printing + '/source'
    unavailable = get('source-missing', source_path, status=409)
    if unavailable != {'status': 'fail', 'data': {'price': 'unavailable'}}:
        raise RuntimeError('Missing synthetic price was not distinguished from a source printing')
    # Retain the same normalized historical manifest shape as database tests.
    # The session-local bypass is explicitly scoped to inserting this synthetic
    # already-retained receipt; it never qualifies real collector/source behavior.
    slot = realm.sql("SELECT ((clock_timestamp() AT TIME ZONE 'UTC'-interval '12 hours')::date)::text")
    manifest = object_json(json.loads(realm.sql("SELECT jsonb_build_object('version','scryfall-slot-v1','slot','" + slot + "','state','valid','source','scryfall:all_cards','hash','sha256:'||repeat('a',64),'artifactRef','prices/scryfall/'||repeat('a',64)||'.jsonl.gz','recordCount','1','downloadUri','https://data.scryfall.io/all-cards/all-cards-20261031212149.jsonl.gz','acquiredAt',least(clock_timestamp(),app.price_slot_start('" + slot + "'::date)+interval '10 minutes'),'sourceUpdatedAt',app.price_slot_start('" + slot + "'::date)-interval '1 hour')")))
    encoded = literal(manifest)
    realm.sql("BEGIN; SET LOCAL session_replication_role=replica; INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref) VALUES('collector:scryfall-v1','" + slot + "','price_slot','" + slot + "',encode(sha256(convert_to(" + encoded + "::text,'UTF8')),'hex'),'accepted'," + encoded + "::text); COMMIT;")
    manifest_id = str(uuid.UUID(realm.sql("SELECT id FROM app.command_receipt WHERE principal_key='collector:scryfall-v1' AND scope='price_slot' AND request_ref='" + slot + "'")))
    card = {'object': 'card', 'id': printing, 'finishes': ['nonfoil'],
            'prices': {'usd': '2.37', 'usd_foil': None, 'usd_etched': None}}
    probe = str(uuid.UUID(realm.sql("SELECT app.record_price_probe('" + manifest_id + "'," + literal(card) + ")")))
    source = object_json(get('source-qualified-shape', source_path)['data'])
    if source['accountId'] != owner or object_json(source['source'])['probeId'] != probe or object_json(source['source'])['currentUsd'] != '2.37':
        raise RuntimeError('Price source projection changed its caller or exact probe')
    if any(key in object_json(source['source']) for key in ['artifactRef', 'manifestId', 'prices', 'evidence', 'hash']):
        raise RuntimeError('Price source projection exposed private retained evidence')
    before = effects()
    # The owned API intentionally has no Temporal service. Establish its safe
    # dependency failure rather than claiming source extraction has executed.
    unavailable = expect('prediction-prepare-runtime-unavailable', request(source_path, owner_token, {}), 503)
    if unavailable.get('status') != 'error' or effects() != before:
        raise RuntimeError('Unavailable source-preparation runtime changed prediction state')
    if prepare_runtime is not None:
        prepare_runtime(source_path, slot)
        if effects() != before:
            raise RuntimeError('Queued preparation changed prediction or financial state')

    command = object_json(json.loads(realm.sql("SELECT jsonb_build_object('requestId','" + str(uuid.uuid4()) + "','printingId','" + printing + "','probeId','" + probe + "','targetUsd','10.00','condition','reached_by','tradingClosesAt',clock_timestamp()+interval '1 hour','observationStartsAt',date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'+interval '2 days 12 hours','deadline',date_trunc('day',clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'+interval '2 days 13 hours')")))
    publication = {'kind': 'publish_market', 'command': command}
    published = post('publish', publication)
    market = resource(published)
    recover('publish-recover', publication, published)
    repeat('publish-repeat', publication, published)
    detail = object_json(get('market-exact', '/v1/predictions/markets/' + market)['data'])
    terms = object_json(object_json(detail['market'])['terms'])
    if detail['accountId'] != owner or terms['id'] != market or terms['printingId'] != printing or terms['payoutCoins'] != '100':
        raise RuntimeError('Published market lost its source or fixed payout')

    owner_before, peer_before = held(owner), held(peer)
    yes = {'kind': 'place_order', 'command': {'requestId': str(uuid.uuid4()), 'marketId': market, 'side': 'yes', 'limitCost': '60', 'quantity': '3'}}
    offered = post('yes-offer', yes)
    yes_id = resource(offered)
    if held(owner) != owner_before + 180:
        raise RuntimeError('Prediction did not hold the full unfilled bid')
    recover('yes-offer-recover', yes, offered)
    repeat('yes-offer-repeat', yes, offered)
    no = {'kind': 'place_order', 'command': {'requestId': str(uuid.uuid4()), 'marketId': market, 'side': 'no', 'limitCost': '40', 'quantity': '1'}}
    matched = post('no-match', no, peer_token)
    recover('no-match-recover', no, matched, peer_token)
    repeat('no-match-repeat', no, matched, peer_token)
    pair = object_json(json.loads(realm.sql("SELECT jsonb_build_object('quantity',quantity::text,'yes',yes_actual_cost_coins::text,'no',no_actual_cost_coins::text,'payout',fixed_payout_coins::text) FROM app.prediction_pair WHERE market_id='" + market + "'")))
    if pair != {'quantity': '1', 'yes': '60', 'no': '40', 'payout': '100'} or held(peer) != peer_before + 40:
        raise RuntimeError('Matched Yes/No contract lost exact fixed backing')
    cancellation = {'kind': 'cancel_order', 'command': {'requestId': str(uuid.uuid4()), 'orderId': yes_id}}
    foreign = dict(cancellation, command=dict(cancellation['command'], requestId=str(uuid.uuid4())))
    before = effects()
    post('cancel-foreign-active-existing', foreign, peer_token, status=409)
    if effects() != before:
        raise RuntimeError('Foreign cancellation changed another player active order')
    canceled = post('cancel-unfilled', cancellation)
    recover('cancel-unfilled-recover', cancellation, canceled)
    repeat('cancel-unfilled-repeat', cancellation, canceled)
    if held(owner) != owner_before + 60 or held(peer) != peer_before + 40:
        raise RuntimeError('Cancellation released a filled position or retained unfilled units')
    positions = object_json(get('own-position', '/v1/predictions/positions?marketId=' + market)['data'])
    if positions['accountId'] != owner or len(positions['items']) != 1:
        raise RuntimeError('Position page lost its original owner or matched unit')
    position = object_json(positions['items'][0])
    if any(position[key] != value for key, value in {'marketId': market, 'orderId': yes_id, 'side': 'yes', 'quantity': '1', 'actualCost': '60', 'fixedPayout': '100'}.items()):
        raise RuntimeError('Position page changed its sealed contract terms')
    changed = dict(yes, command=dict(yes['command'], quantity='4'))
    before = effects()
    post('lookup-changed-quantity', changed, lookup=True, status=409)
    if effects() != before:
        raise RuntimeError('Conflicting prediction lookup changed financial effects')
