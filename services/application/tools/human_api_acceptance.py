"""Qualify human metadata/intent HTTP composition using real managed caller identities.

Privileged admitted/recipe/participation fixtures qualify storage and authorization only.
They are explicitly not evidence of native gameplay, external supply evidence or hosted AI.
"""
import json
import uuid
from typing import Callable, TypedDict
from disposable_supabase import DisposableSupabase
from qualification_http import object_json

Request = Callable[[str, str | None, dict | None], tuple[int, object]]
Expect = Callable[[str, tuple[int, object], int], dict]


class Fixture(TypedDict):
    """Source-qualified-shape inputs retained by the owning managed acceptance harness."""
    format_id: str
    profile_id: str
    match_id: str
    release: str
    legality: str
    definition: str
    printing: str


def qualify_human_api(realm: DisposableSupabase, request: Request, expect: Expect,
                      owner_token: str, other_token: str, owner: str, peer: str,
                      fixture: Fixture, fresh_identity: Callable[[], str]) -> None:
    """Exercise real HTTP→live Auth→SQL/RLS without canned backend responses."""
    def get(label: str, path: str, token: str | None = owner_token, status: int = 200) -> dict:
        return expect('human-' + label, request(path, token, None), status)

    def post(label: str, path: str, body: dict, token: str = owner_token, status: int = 202) -> dict:
        return expect('human-' + label, request(path, token, body), status)

    def resource(payload: dict) -> str:
        return str(uuid.UUID(object_json(payload['data'])['resourceId']))

    def sql_json(statement: str) -> dict:
        return object_json(json.loads(realm.sql(statement)))

    def literal(value: object) -> str:
        # Test-only SQL constants/UUIDs; never a production query interface.
        return "'" + json.dumps(value, ensure_ascii=False).replace("'", "''") + "'::jsonb"

    mid = fixture['match_id']
    listing = object_json(get('match-list', '/v1/matches')['data'])
    if not any(object_json(row)['id'] == mid for row in listing['items']):
        raise RuntimeError('Owned match missing from participant page')
    detail = object_json(get('match-detail', '/v1/matches/' + mid)['data'])
    if object_json(detail['match'])['id'] != mid or detail['accountId'] != owner or 'result' not in detail:
        raise RuntimeError('Match context/required nullable result differs')
    for suffix in ['games', 'seats', 'decisions', 'ante']:
        get('match-' + suffix, '/v1/matches/' + mid + '/' + suffix)
    seats = object_json(get('seat-page-bound', '/v1/matches/' + mid + '/seats?limit=1')['data'])
    if len(seats['items']) != 1 or seats['nextCursor'] is None:
        raise RuntimeError('Match seat page was not bounded')
    get('seat-page-after', '/v1/matches/' + mid + '/seats?after=' + str(uuid.UUID(seats['nextCursor'])) + '&limit=1')
    get('match-anonymous', '/v1/matches/' + mid, None, 401)
    get('match-unavailable', '/v1/matches/' + str(uuid.uuid4()), status=404)
    get('competitive-history', '/v1/competitive-results')
    get('daily-win-history', '/v1/daily-wins')
    get('match-alias-path', '/v1/matches/%7B' + mid + '%7D', status=400)

    # Fresh manual room, qualified only as a normalized SQL table profile.
    created = sql_json("SELECT app.create_room('" + owner + "','" + fixture['format_id'] + "','" + fixture['profile_id'] + "','open',false,'public','" + str(uuid.uuid4()) + "')")
    room = resource(created)
    path = '/v1/rooms/' + room + '/ai'
    get('ai-empty-host-status', path)
    add = {'requestId': str(uuid.uuid4()), 'expectedRevision': '0'}
    post('ai-add-unknown', path + '/disposition', add, status=200)
    added = post('ai-add', path, add)
    slot = resource(added)
    if post('ai-add-exact-replay', path, add) != added:
        raise RuntimeError('AI admission replay changed slot identity')
    post('ai-add-recovery', path + '/disposition', add, status=200)
    post('ai-add-stranger', path, dict(add, requestId=str(uuid.uuid4())), other_token, 409)
    post('ai-add-forged-sponsor', path, dict(add, sponsorId=peer), status=400)
    get('ai-status-stranger', path, other_token, 404)
    bootstrap = {'requestId': str(uuid.uuid4()), 'expectedRevision': '1', 'deckRevisionId': None}
    bootstrap_path = '/v1/ai-slots/' + slot + '/bootstrap'
    booted = post('ai-bootstrap-intent', bootstrap_path, bootstrap)
    if set(object_json(booted['data'])) != {'resourceId', 'epoch'}:
        raise RuntimeError('Public bootstrap exposed private start identity')
    recovered = post('ai-bootstrap-recovery', bootstrap_path + '/disposition', bootstrap, status=200)
    if object_json(recovered['data']) != {'state': 'accepted', 'response': booted}:
        raise RuntimeError('Bootstrap exact redacted recovery differs')
    status = object_json(get('ai-pending-status', path)['data'])
    if object_json(object_json(status['items'][0])['binding'])['observedState'] != 'pending':
        raise RuntimeError('Bootstrap intent was presented as running')
    removal = {'requestId': str(uuid.uuid4()), 'expectedRevision': '2'}
    removed_path = '/v1/ai-slots/' + slot + '/remove'
    post('ai-remove', removed_path, removal)
    post('ai-remove-recovery', removed_path + '/disposition', removal, status=200)
    stopped = object_json(object_json(get('ai-retired-stop-pending', path)['data'])['items'][0])
    if stopped['isRetired'] is not True or stopped['stopPending'] is not True or stopped['binding'] is not None:
        raise RuntimeError('Requested AI stop was presented as completed')
    unready = {'requestId': str(uuid.uuid4()), 'expectedRevision': '3'}
    post('unready-noop', '/v1/rooms/' + room + '/unready', unready)
    post('unready-exact-recovery', '/v1/rooms/' + room + '/unready/disposition', unready, status=200)

    # The same synthetic two-card unlimited-basic source supports Collection admission.
    deck_input = {'name': 'Collection SQL fixture', 'mode': 'collection', 'formatId': fixture['format_id'],
                  'entries': [{'definitionId': fixture['definition'], 'printingId': fixture['printing'], 'section': 'main', 'quantity': '2'}]}
    revisions = []
    for label, token in [('owner', owner_token), ('peer', other_token)]:
        imported = post('ante-' + label + '-library', '/v1/decks/import', dict(deck_input, requestId=str(uuid.uuid4())), token)
        revisions.append(str(uuid.UUID(object_json(imported['data'])['revisionId'])))

    def reserve_collection() -> str:
        """Canonical admission fixture, not actual execution-release/deck validation evidence."""
        lobby = resource(sql_json("SELECT app.create_room('" + owner + "','" + fixture['format_id'] + "','" + fixture['profile_id'] + "','collection',false,'public','" + str(uuid.uuid4()) + "')"))
        def basis() -> str:
            return realm.sql("SELECT revision::text FROM app.room WHERE id='" + lobby + "'")
        for actor, revision in zip([owner, peer], revisions, strict=True):
            if actor == peer:
                joined = sql_json("SELECT app.join_room('" + actor + "','" + lobby + "',NULL,'" + str(uuid.uuid4()) + "')")
                if joined['status'] != 'success':
                    raise RuntimeError('Collection fixture join failed')
            selected = sql_json("SELECT app.select_room_deck('" + actor + "','" + lobby + "'," + basis() + ",'" + revision + "','" + str(uuid.uuid4()) + "')")
            if selected['status'] != 'success':
                raise RuntimeError('Collection fixture deck selection failed')
        countdown = sql_json("SELECT app.start_room_countdown('" + owner + "','" + lobby + "'," + basis() + ",'fixture:sql-only','" + str(uuid.uuid4()) + "')")
        if countdown['status'] != 'success':
            raise RuntimeError('Collection fixture countdown failed')
        realm.sql("UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='" + lobby + "'")
        started = sql_json("SELECT app.start_room_match('" + lobby + "'," + basis() + ",'" + fixture['release'] + "','" + fixture['legality'] + "','fixture:sql-only',(SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:sql-only') ORDER BY seat_index) FROM app.room_slot WHERE room_id='" + lobby + "' AND retired_at IS NULL),'" + str(uuid.uuid4()) + "')")
        if started['status'] != 'success':
            raise RuntimeError('Collection fixture admission failed')
        return resource(started)

    collection = reserve_collection()
    coin = str(uuid.UUID(realm.sql("SELECT id FROM app.asset_definition WHERE currency_key='coin'")))
    offer = {'requestId': str(uuid.uuid4()), 'expectedTermsRevision': '0', 'assets': [{'assetId': coin, 'quantity': '100'}]}
    ante_path = '/v1/matches/' + collection + '/ante'
    post('ante-unknown', ante_path + '/disposition', offer, status=200)
    offered = post('ante-human-offer', ante_path, offer)
    commitment = resource(offered)
    post('ante-original-recovery', ante_path + '/disposition', offer, status=200)
    second = post('ante-peer-offer', ante_path, dict(offer, requestId=str(uuid.uuid4()), expectedTermsRevision='1'), other_token)
    terms_hash = object_json(second['data'])['termsHash']
    terms = object_json(get('ante-redacted-terms', ante_path)['data'])
    if object_json(terms['agreement'])['termsHash'] != terms_hash or any(key in json.dumps(terms) for key in ['holdId', 'exposureId', 'sourceStakeId', 'valuationEvidence']):
        raise RuntimeError('Ante safe display/hash differs')
    acceptance = {'requestId': str(uuid.uuid4()), 'expectedTermsRevision': '2', 'termsHash': terms_hash}
    partial = post('ante-partial-consent', '/v1/ante/' + commitment + '/accept', acceptance)
    if object_json(partial['data'])['state'] != 'offered':
        raise RuntimeError('Early ante consent was presented as committed')
    final = post('ante-whole-roster-commit', '/v1/ante/' + commitment + '/accept', dict(acceptance, requestId=str(uuid.uuid4())), other_token)
    if object_json(final['data'])['state'] != 'committed':
        raise RuntimeError('All-player agreement did not commit')
    post('ante-consent-recovery', '/v1/ante/' + commitment + '/accept/disposition', acceptance, status=200)
    cancellation = {'requestId': str(uuid.uuid4()), 'expectedTermsRevision': '2'}
    post('ante-committed-cancel-denied', '/v1/ante/' + commitment + '/cancel', cancellation, status=409)
    cancellable = reserve_collection()
    cancel_offer = post('ante-cancelable-offer', '/v1/matches/' + cancellable + '/ante', dict(offer, requestId=str(uuid.uuid4())))
    cancel_id = resource(cancel_offer)
    cancel_intent = {'requestId': str(uuid.uuid4()), 'expectedTermsRevision': '1'}
    post('ante-offered-cancel', '/v1/ante/' + cancel_id + '/cancel', cancel_intent)
    post('ante-cancel-recovery', '/v1/ante/' + cancel_id + '/cancel/disposition', cancel_intent, status=200)

    # Frozen supply evidence is deliberately synthetic; no external product/source is qualified.
    candidates = []
    for _ in range(5):
        set_id, product, recipe, slot, printing, asset = [str(uuid.uuid4()) for _ in range(6)]
        realm.sql("INSERT INTO app.card_set(id,external_set_ref,name) VALUES('" + set_id + "','" + set_id + "','Supply SQL fixture'); INSERT INTO app.booster_product(id,set_id,name,external_product_ref) VALUES('" + product + "','" + set_id + "','Supply SQL fixture','" + product + "'); INSERT INTO app.booster_recipe(id,product_id,revision_ref,evidence_ref) VALUES('" + recipe + "','" + product + "','fixture','fixture:sql-only'); INSERT INTO app.booster_slot(id,recipe_id,slot_key,draws) VALUES('" + slot + "','" + recipe + "','fixture',1); INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES('" + printing + "','" + fixture['definition'] + "','" + set_id + "','" + printing + "','rare','nonfoil','fixture:sql-only'); INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES('" + slot + "','" + printing + "',1); INSERT INTO app.asset_definition(id,kind,printing_id) VALUES('" + asset + "','card','" + printing + "'); SELECT app.qualify_booster_recipe('" + recipe + "','sha256:" + 'a' * 64 + "'); SELECT app.qualify_booster_opening('" + recipe + "'," + literal({'version':'pack-opening-v1','groups':[]}) + ",'sha256:'||app.auth_json_hash(" + literal({'version':'pack-opening-v1','groups':[]}) + "))")
        candidates.append({'setId':set_id,'products':[{'recipeId':recipe,'weight':'1','normalQuantity':'100','modifierN':'1','modifierD':'1','modifierRef':'sha256:'+'a'*64}]})
    season = str(uuid.UUID(realm.sql("INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state) SELECT extract(year FROM d)::bigint,extract(quarter FROM d)::bigint,d AT TIME ZONE 'UTC',(d+interval '3 months') AT TIME ZONE 'UTC','fixture','planned' FROM(SELECT date_trunc('quarter',clock_timestamp() AT TIME ZONE 'UTC')+interval '3 months' d) q RETURNING id")))
    manifest = {'version':'supply-ballot-v1','supplyCoefficient':'60','sourceRef':'sha256:'+'a'*64,'tieSeed':'a'*64,'candidates':candidates}
    if sql_json("SELECT app.open_supply_ballot('" + season + "'," + literal(manifest) + ")")['status'] != 'success':
        raise RuntimeError('Ballot fixture opening failed')
    ballot_path = '/v1/seasons/' + season + '/ballot'
    approvals = {'requestId': str(uuid.uuid4()), 'setIds': [candidates[1]['setId'], candidates[0]['setId']]}
    # Other composed native fixtures may already qualify the shared owner. Keep
    # this negative control independent of their legitimate participation rows.
    ineligible_token=fresh_identity()
    ineligible=post('ballot-register-ineligible', '/v1/account', {'username':'NoGames'+uuid.uuid4().hex[:12]},ineligible_token,status=200)
    ineligible_id=str(uuid.UUID(object_json(ineligible['data'])['accountId']))
    if realm.sql("SELECT count(*) FROM app.game_participation WHERE account_id='"+ineligible_id+"' AND real_game")!='0':
        raise RuntimeError('Ballot negative control already has qualifying participation')
    post('ballot-ineligible', ballot_path, approvals, ineligible_token,status=409)
    # Privileged legacy relational completion, matching isolated SQL fixtures. Not a native game.
    legacy_match, legacy_seat, legacy_game = [str(uuid.uuid4()) for _ in range(3)]
    realm.sql("INSERT INTO app.match(id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision) VALUES('" + legacy_match + "','" + fixture['format_id'] + "','" + fixture['profile_id'] + "','" + fixture['legality'] + "','open',false,'bo1','completed',0); INSERT INTO app.match_seat(id,match_id,account_id,seat_index,controller_kind) VALUES('" + legacy_seat + "','" + legacy_match + "','" + owner + "',0,'human'); INSERT INTO app.game(id,match_id,game_index,state,accepted_revision) VALUES('" + legacy_game + "','" + legacy_match + "',0,'completed',0); INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision) VALUES('" + legacy_game + "','" + legacy_seat + "','" + owner + "',3,true,'fixture:relational-only',clock_timestamp(),0)")
    approvals['requestId'] = str(uuid.uuid4())
    voted = post('ballot-replace-ordered', ballot_path, approvals)
    post('ballot-recover-ordered', ballot_path + '/disposition', approvals, status=200)
    post('ballot-order-conflict', ballot_path + '/disposition', dict(approvals, setIds=list(reversed(approvals['setIds']))), status=409)
    own = object_json(get('ballot-frozen-own-page', ballot_path + '?limit=2')['data'])
    if own['publication'] is not None or len(own['items']) != 2 or any(object_json(row)['finalApprovals'] is not None for row in own['items']):
        raise RuntimeError('Live ballot exposed a tally or lost bounds')
    foreign = object_json(get('ballot-own-isolation', ballot_path, other_token)['data'])
    if foreign['approvedSetIds'] != [] or voted['status'] != 'success':
        raise RuntimeError('Ballot ownership differs')
    post('ballot-clear', ballot_path, {'requestId':str(uuid.uuid4()),'setIds':[]})
