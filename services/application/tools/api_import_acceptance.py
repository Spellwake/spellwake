"""Exercise the Rust import HTTP boundary against the caller's owned managed realm."""
import json
import base64
import hashlib
import secrets
import socket
import time
import re
import urllib.parse
import urllib.request
import uuid
from typing import Callable

from disposable_supabase import DisposableSupabase, ROOT, private_write
from qualification_http import make_opener, request_json_with_headers, object_json


def qualify_import_api(realm: DisposableSupabase, owner_token: str, other_token: str,
                       owner: str, defaults: list[str], fresh_identity: Callable[[], str],
                       *, temporal_preparation: bool = False, native_receipt_fixture: dict | None = None) -> dict:
    """Use actual managed JWTs and SQL; no canned backend responses qualify this path."""
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    base = 'http://127.0.0.1:' + str(port)
    api_arguments = [str(ROOT / 'target/debug/application_api')]
    api_environment = {
        'SPELLWAKE_SUPABASE_URL': realm.status['API_URL'],
        'SPELLWAKE_SUPABASE_PUBLISHABLE_KEY': realm.status['PUBLISHABLE_KEY'],
        'SPELLWAKE_SUPABASE_SECRET_KEY': realm.status['SECRET_KEY'],
        'SPELLWAKE_AUTH_ISSUER': realm.status['API_URL'] + '/auth/v1',
        'SPELLWAKE_FREE_BACK_ID': defaults[0], 'SPELLWAKE_FREE_SLEEVE_ID': defaults[1],
        'SPELLWAKE_FREE_DICE_ID': defaults[2], 'SPELLWAKE_API_BIND': '127.0.0.1:' + str(port),
        'SPELLWAKE_ROOM_INVITATION_KEY': secrets.token_urlsafe(32),
    }
    process = realm.start_background(api_arguments, api_environment)
    opener = make_opener()
    checks: list[dict] = []
    progress_path = realm.stage / 'api-http-case-progress.jsonl'
    private_write(progress_path, b'')
    # Record requested registered path templates, never concrete account/resource
    # identifiers, query values, credentials or bodies. Observations establish
    # actual HTTP statuses, not independent semantic acceptance of each endpoint.
    router_source = (ROOT / 'src/api.rs').read_text()
    templates = re.findall(r'\.route\(\s*"([^"\n]+)"', router_source)
    if len(templates) != router_source.count('.route(') or len(set(templates)) != len(templates):
        raise RuntimeError('Route-template coverage requires source reconciliation')
    patterns = [(template, re.compile('^' + '/'.join(
        '[^/]+' if part.startswith('{') and part.endswith('}') else re.escape(part)
        for part in template.split('/')) + '$')) for template in templates]
    patterns.sort(key=lambda item: item[0].count('{'))
    observations: dict[tuple[str, str, int], int] = {}

    def dispatch(path: str, token: str | None, data: bytes | None, extra_headers: dict[str, str]) -> tuple[int, object, dict[str, str]]:
        headers = {'Content-Type': 'application/json', **extra_headers}
        if token is not None:
            headers['Authorization'] = 'Bearer ' + token
        outgoing = urllib.request.Request(base + path, headers=headers, data=data)
        response = request_json_with_headers(opener, outgoing)
        requested_path = urllib.parse.urlsplit(path).path
        route = next((template for template, pattern in patterns if pattern.fullmatch(requested_path)), None)
        if route is None:
            raise RuntimeError('Qualification requested an unaccounted route template')
        key = (outgoing.get_method(), route, response[0])
        observations[key] = observations.get(key, 0) + 1
        return response

    def request_with_headers(path: str, token: str | None, data: dict | None = None) -> tuple[int, object, dict[str, str]]:
        return dispatch(path, token, None if data is None else json.dumps(data).encode(), {})

    def request_raw(path: str, data: bytes, headers: dict[str, str]) -> tuple[int, object]:
        status, payload, _ = dispatch(path, None, data, headers)
        return status, payload

    def request(path: str, token: str | None, data: dict | None = None) -> tuple[int, object]:
        status, payload, _ = request_with_headers(path, token, data)
        return status, payload

    def expect(case: str, response: tuple[int, object], status: int) -> dict:
        actual, payload = response
        checks.append({'case': case, 'status': actual, 'passed': actual == status})
        # Status progress survives a later semantic assertion failure. No bearer,
        # headers, request bodies or concrete URLs enter this diagnostic journal.
        with progress_path.open('ab') as journal:
            journal.write(json.dumps({'scope':'status only; later semantic assertions may still fail',
                'case':case,'status':actual,'passed':actual == status}).encode() + b'\n')
        if actual != status:
            private_write(realm.stage / 'private-api-http-failure.json', json.dumps({
                'case':case,'actualStatus':actual,'expectedStatus':status,'payload':payload}).encode())
            raise RuntimeError('Owned API HTTP case failed: ' + case)
        return object_json(payload)

    def wait_ready() -> None:
        deadline = time.monotonic() + 15
        while True:
            if process.poll() is not None:
                raise RuntimeError('Owned application exited before readiness')
            try:
                if request('/health', None)[0] == 200:
                    break
            except OSError:
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError('Owned API readiness deadline exceeded')
            time.sleep(0.1)

    def restart_api(with_key: bool) -> None:
        nonlocal process
        process.terminate()
        process.wait(timeout=5)
        environment = dict(api_environment)
        if not with_key:
            environment.pop('SPELLWAKE_ROOM_INVITATION_KEY')
        process = realm.start_background(api_arguments, environment)
        wait_ready()

    wait_ready()
    native_receipts: dict = {'executed':False}
    if native_receipt_fixture is not None:
        from native_receipt_api_acceptance import qualify_native_receipt_api
        native_receipts=qualify_native_receipt_api(realm,request,request_with_headers,expect,owner_token,other_token,fresh_identity,native_receipt_fixture)
    format_id = str(uuid.uuid4())
    realm.sql("INSERT INTO app.format(id,key,name) VALUES('" + format_id + "','" + format_id + "','API fixture')")
    # Empty saved-library content is allowed; this is not a native admission or
    # playable-deck qualification. The transaction must still seal one revision.
    intent = {'requestId': str(uuid.uuid4()), 'name': 'Owned API import', 'mode': 'open',
              'formatId': format_id, 'entries': []}
    before = realm.sql("SELECT count(*) FROM app.deck WHERE account_id='" + str(uuid.UUID(owner)) + "'")
    unknown = expect('import-unknown', request('/v1/decks/import/disposition', owner_token, intent), 200)
    if object_json(unknown['data']) != {'state': 'unknown', 'response': None}:
        raise RuntimeError('Unknown import receipt shape differs')
    accepted = expect('import-accepted', request('/v1/decks/import', owner_token, intent), 202)
    ack = object_json(accepted['data'])
    deck = str(uuid.UUID(ack['resourceId']))
    revision = str(uuid.UUID(ack['revisionId']))
    if deck == revision or set(ack) != {'resourceId', 'revisionId'}:
        raise RuntimeError('Import did not confirm both separate IDs')
    if realm.sql("SELECT count(*) FROM app.deck d JOIN app.deck_revision r ON r.id=d.current_revision_id WHERE d.id='" + deck + "' AND r.id='" + revision + "' AND r.sealed_at IS NOT NULL AND d.account_id='" + owner + "'") != '1':
        raise RuntimeError('API acknowledgement does not match actual sealed database state')
    if expect('import-exact-replay', request('/v1/decks/import', owner_token, intent), 202) != accepted:
        raise RuntimeError('Import replay changed its confirmed IDs')
    disposition = expect('import-established-disposition', request('/v1/decks/import/disposition', owner_token, intent), 200)
    if object_json(disposition['data']) != {'state': 'accepted', 'response': accepted}:
        raise RuntimeError('Import disposition differs from established acceptance')
    changed = dict(intent, name='Conflicting name')
    expect('import-changed-terms', request('/v1/decks/import', owner_token, changed), 409)
    expect('import-changed-lookup', request('/v1/decks/import/disposition', owner_token, changed), 409)
    other = expect('other-principal-no-receipt', request('/v1/decks/import/disposition', other_token, intent), 200)
    if object_json(other['data'])['state'] != 'unknown':
        raise RuntimeError('Foreign principal read the owner import receipt')
    invalid = dict(intent, requestId=str(uuid.uuid4()), entries=[{'definitionId': str(uuid.uuid4()),
        'printingId': None, 'section': 'main', 'quantity': '1'}])
    expect('import-rejected-content', request('/v1/decks/import', owner_token, invalid), 409)
    if int(realm.sql("SELECT count(*) FROM app.deck WHERE account_id='" + owner + "'")) != int(before) + 1:
        raise RuntimeError('Rejected API import left partial library state')
    expect('import-no-bearer', request('/v1/decks/import', None, intent), 401)
    expect('import-caller-actor-rejected', request('/v1/decks/import', owner_token, dict(intent, actorId=owner)), 400)
    quests = expect('owner-quest-http', request('/v1/quests?limit=1', owner_token), 200)
    quest_data = object_json(quests['data'])
    if quest_data.get('accountId') != owner or len(quest_data.get('items', [])) != 1 or not isinstance(quest_data.get('utcDay'), str):
        raise RuntimeError('HTTP quest projection differs from the populated owner')
    foreign_quests = expect('stranger-quest-http', request('/v1/quests?limit=1', other_token), 200)
    if object_json(foreign_quests['data']).get('items') != []:
        raise RuntimeError('HTTP stranger read the owner assignment')
    # Low-level retained rank data qualifies only the read projection here;
    # canonical scoring/result consumption is independently exercised in SQL.
    season = str(uuid.uuid4())
    realm.sql("INSERT INTO app.season(id,year,quarter,start_at,end_at,policy_ref,state) VALUES('" + season + "',1979,1,'1979-01-01T00:00:00Z','1979-04-01T00:00:00Z','fixture','closed')")
    rank = str(uuid.uuid4())
    realm.sql("INSERT INTO app.rank(id,account_id,format_id,season_id,tier,rating_ref,revision,finalized,rating) VALUES('" + rank + "','" + owner + "','" + format_id + "','" + season + "','mythic','fixture:read-only',0,false,9007199254740993)")
    ranks = expect('owner-rank-http', request('/v1/ranks?formatId=' + format_id + '&seasonId=' + season, owner_token), 200)
    rank_items = object_json(ranks['data']).get('items')
    if not isinstance(rank_items, list) or len(rank_items) != 1 or object_json(rank_items[0]).get('id') != rank or object_json(rank_items[0]).get('rating') != '9007199254740993':
        raise RuntimeError('HTTP owner rank or exact integer differs')
    foreign_ranks = expect('stranger-rank-http', request('/v1/ranks?formatId=' + format_id, other_token), 200)
    if object_json(foreign_ranks['data']).get('items') != []:
        raise RuntimeError('HTTP stranger read the owner rank')
    calendar = expect('season-calendar-http', request('/v1/seasons?limit=100', owner_token), 200)
    if not any(object_json(item).get('id') == season and object_json(item).get('state') == 'closed' for item in object_json(calendar['data']).get('items', [])):
        raise RuntimeError('HTTP calendar omitted the stored quarter')
    expect('progression-actor-query-rejected', request('/v1/quests?accountId=' + owner, owner_token), 400)
    # Draft room creation/select/leave use actual session facades. This profile is
    # deliberately unqualified: draft metadata is not native match admission.
    profile = str(uuid.uuid4())
    realm.sql("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref) VALUES('" + profile + "','" + profile + "','fixture','fixture')")
    room_intent = {'kind': 'create', 'command': {'requestId': str(uuid.uuid4()),
        'formatId': format_id, 'profileId': profile, 'mode': 'open',
        'competitive': False, 'visibility': 'private'}}
    expect('room-unknown', request('/v1/rooms/commands/disposition', owner_token, room_intent), 200)
    created = expect('room-create', request('/v1/rooms/commands', owner_token, room_intent), 202)
    room = str(uuid.UUID(object_json(created['data'])['resourceId']))
    if realm.sql("SELECT count(*) FROM app.room WHERE id='" + room + "' AND host_id='" + owner + "' AND state='lobby'") != '1':
        raise RuntimeError('Room acknowledgment differs from database')
    if expect('room-create-replay', request('/v1/rooms/commands', owner_token, room_intent), 202) != created:
        raise RuntimeError('Room replay changed identity')
    choose = {'kind': 'select_deck', 'command': {'requestId': str(uuid.uuid4()),
        'roomId': room, 'expectedRevision': '0', 'deckRevisionId': revision}}
    expect('room-select', request('/v1/rooms/commands', owner_token, choose), 202)
    selected = expect('room-select-recover', request('/v1/rooms/commands/disposition', owner_token, choose), 200)
    if object_json(selected['data']).get('state') != 'accepted':
        raise RuntimeError('Room select numeric fingerprint differs')
    changed_choose = {'kind': 'select_deck', 'command': dict(choose['command'], expectedRevision='1')}
    expect('room-select-conflict', request('/v1/rooms/commands/disposition', owner_token, changed_choose), 409)
    visible = expect('room-owner-detail', request('/v1/rooms/' + room, owner_token), 200)
    slots = object_json(object_json(visible['data'])['slots']).get('items', [])
    if len(slots) != 1 or object_json(slots[0]).get('deckRevisionId') != revision:
        raise RuntimeError('Own selected revision missing from room detail')
    expect('room-foreign-private-hidden', request('/v1/rooms/' + room, other_token), 404)
    rooms = expect('room-owner-list', request('/v1/rooms?limit=100', owner_token), 200)
    if not any(object_json(item).get('id') == room for item in object_json(rooms['data']).get('items', [])):
        raise RuntimeError('Room list omitted owned private lobby')
    # Direct fixture visibility update qualifies the read policy, not a host configure endpoint.
    realm.sql("UPDATE app.room SET visibility='public' WHERE id='" + room + "'")
    public = expect('room-public-stranger-detail', request('/v1/rooms/' + room, other_token), 200)
    for item in object_json(object_json(public['data'])['slots']).get('items', []):
        if object_json(item).get('deckRevisionId') is not None:
            raise RuntimeError('Public room leaked opponent selected revision')
    expect('room-slot-query-authority-rejected', request('/v1/rooms/' + room + '?actorId=' + owner, owner_token), 400)
    host_config = expect('room-host-configuration-read', request('/v1/rooms/' + room + '/configuration', owner_token), 200)
    expect('room-nonhost-configuration-hidden', request('/v1/rooms/' + room + '/configuration', other_token), 404)
    # Normalized profile fixture exercises SQL configuration only, never native admission.
    configured_profile = str(uuid.uuid4())
    realm.sql("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref) VALUES('" + configured_profile + "','" + configured_profile + "','fixture','fixture',2,2,'fixture:sql-only')")
    configure = {'requestId': str(uuid.uuid4()), 'expectedRevision': '1',
        'profileId': configured_profile, 'structure': 'bo3',
        'teams': object_json(host_config['data'])['teams']}
    configuration_ack = expect('room-host-configure', request('/v1/rooms/' + room + '/configuration', owner_token, configure), 202)
    ack_data = object_json(configuration_ack['data'])
    if ack_data != {'resourceId': room, 'revision': '2'}:
        raise RuntimeError('Configuration acknowledgment lost revision or room')
    if expect('room-configuration-replay', request('/v1/rooms/' + room + '/configuration', owner_token, configure), 202) != configuration_ack:
        raise RuntimeError('Configuration replay differs')
    config_receipt = expect('room-configuration-disposition', request('/v1/rooms/' + room + '/configuration/disposition', owner_token, configure), 200)
    if object_json(config_receipt['data']) != {'state': 'accepted', 'response': configuration_ack}:
        raise RuntimeError('Configuration disposition differs')
    changed_config = dict(configure, structure='bo1')
    expect('room-configuration-changed-terms', request('/v1/rooms/' + room + '/configuration/disposition', owner_token, changed_config), 409)
    leave = {'kind': 'leave', 'command': {'requestId': str(uuid.uuid4()), 'roomId': room, 'expectedRevision': ack_data['revision']}}

    expect('room-leave', request('/v1/rooms/commands', owner_token, leave), 202)
    recovered = expect('room-closed-recover', request('/v1/rooms/commands/disposition', owner_token, leave), 200)
    if object_json(recovered['data']).get('state') != 'accepted' or realm.sql("SELECT state FROM app.room WHERE id='" + room + "'") != 'closed':
        raise RuntimeError('Closed room receipt recovery failed')
    stranger = expect('room-stranger-recover', request('/v1/rooms/commands/disposition', other_token, leave), 200)
    if object_json(stranger['data']).get('state') != 'unknown':
        raise RuntimeError('Other account saw room receipt')
    # Actual private invitation delivery and join use the separate owned key.
    private_intent = {'kind': 'create', 'command': {'requestId': str(uuid.uuid4()),
        'formatId': format_id, 'profileId': configured_profile, 'mode': 'open',
        'competitive': False, 'visibility': 'private'}}
    private_ack = expect('invitation-private-room-create', request('/v1/rooms/commands', owner_token, private_intent), 202)
    private_room = str(uuid.UUID(object_json(private_ack['data'])['resourceId']))
    issue = {'requestId': str(uuid.uuid4()), 'expectedRevision': '0'}
    issue_path = '/v1/rooms/' + private_room + '/invitations'
    expect('invitation-unknown', request(issue_path + '/disposition', owner_token, issue), 200)
    issued = expect('invitation-issue-and-deliver', request(issue_path, owner_token, issue), 202)
    issued_data = object_json(issued['data'])
    invitation = str(uuid.UUID(issued_data['resourceId']))
    raw_token = issued_data['token']
    if not isinstance(raw_token, str) or len(raw_token) != 43:
        raise RuntimeError('Issued capability is not canonical length')
    decoded = base64.urlsafe_b64decode(raw_token + '=')
    if len(decoded) != 32 or base64.urlsafe_b64encode(decoded).decode().rstrip('=') != raw_token:
        raise RuntimeError('Issued capability encoding is not canonical')
    stored_hash = realm.sql("SELECT token_hash FROM app.invitation WHERE id='" + invitation + "'")
    if stored_hash != hashlib.sha256(decoded).hexdigest():
        raise RuntimeError('Delivered capability does not match stored hash')
    receipt = realm.sql("SELECT response_payload::text FROM app.command_receipt WHERE principal_key='human:" + owner + "' AND scope='room_invite' AND request_ref='" + issue['requestId'] + "'")
    if raw_token in receipt:
        raise RuntimeError('Raw capability entered SQL receipt')
    joined_input = {'requestId': str(uuid.uuid4()), 'token': raw_token}
    join_path = '/v1/rooms/' + private_room + '/join'
    expect('invitation-private-null-denied', request(join_path, other_token, {'requestId': str(uuid.uuid4()), 'token': None}), 409)
    joined = expect('invitation-private-join', request(join_path, other_token, joined_input), 202)
    joined_data = object_json(joined['data'])
    if joined_data.get('resourceId') != private_room or not isinstance(joined_data.get('slotId'), str):
        raise RuntimeError('Joined acknowledgment does not bind room and slot')
    expect('invitation-join-recover', request(join_path + '/disposition', other_token, joined_input), 200)
    issuer_list = expect('invitation-issuer-safe-list', request('/v1/invitations?roomId=' + private_room, owner_token), 200)
    issuer_items = object_json(issuer_list['data']).get('items', [])
    if len(issuer_items) != 1 or set(object_json(issuer_items[0])) != {'id', 'roomId', 'state'}:
        raise RuntimeError('Invitation list is incomplete or contains private fields')
    guest_list = expect('invitation-other-issuer-isolated', request('/v1/invitations?roomId=' + private_room, other_token), 200)
    if object_json(guest_list['data']).get('items') != []:
        raise RuntimeError('Guest read issuer invitation inventory')
    revoke_path = '/v1/invitations/' + invitation + '/revoke'
    expect('invitation-foreign-revoke-denied', request(revoke_path, other_token, {'requestId': str(uuid.uuid4())}), 409)
    restart_api(True)
    recovered = expect('invitation-recovery-after-restart', request(issue_path + '/disposition', owner_token, issue), 200)
    if object_json(recovered['data']) != {'state': 'accepted', 'response': issued}:
        raise RuntimeError('Restart changed original delivered capability')
    revocation = {'requestId': str(uuid.uuid4())}
    expect('invitation-issuer-revoke', request(revoke_path, owner_token, revocation), 202)
    expect('invitation-revoke-disposition', request(revoke_path + '/disposition', owner_token, revocation), 200)
    if expect('invitation-revoked-existing-member-noop', request(join_path, other_token, {'requestId': str(uuid.uuid4()), 'token': None}), 202) != joined:
        raise RuntimeError('Revocation changed existing member slot')
    guest_view = expect('invitation-guest-current-room', request('/v1/rooms/' + private_room, other_token), 200)
    guest_basis = object_json(object_json(guest_view['data'])['room'])['revision']
    expect('invitation-guest-leave', request('/v1/rooms/commands', other_token, {'kind': 'leave', 'command': {'requestId': str(uuid.uuid4()), 'roomId': private_room, 'expectedRevision': guest_basis}}), 202)
    expect('invitation-revoked-new-join-denied', request(join_path, other_token, {'requestId': str(uuid.uuid4()), 'token': raw_token}), 409)
    historical = expect('invitation-original-join-recover-after-leave', request(join_path + '/disposition', other_token, joined_input), 200)
    if object_json(historical['data']) != {'state': 'accepted', 'response': joined}:
        raise RuntimeError('Original join receipt changed after leave')
    restart_api(False)
    missing_key_issue = {'requestId': str(uuid.uuid4()), 'expectedRevision': '2'}
    expect('invitation-missing-key-no-issue', request(issue_path, owner_token, missing_key_issue), 503)
    if realm.sql("SELECT count(*) FROM app.command_receipt WHERE principal_key='human:" + owner + "' AND scope='room_invite' AND request_ref='" + missing_key_issue['requestId'] + "'") != '0':
        raise RuntimeError('Missing key created a mutation receipt')
    expect('invitation-keyless-list', request('/v1/invitations?roomId=' + private_room, owner_token), 200)
    expect('invitation-keyless-revoke', request(revoke_path, owner_token, {'requestId': str(uuid.uuid4())}), 202)
    public_intent = {'kind': 'create', 'command': dict(private_intent['command'], requestId=str(uuid.uuid4()), visibility='public')}
    public_ack = expect('invitation-keyless-public-room', request('/v1/rooms/commands', owner_token, public_intent), 202)
    public_room = str(uuid.UUID(object_json(public_ack['data'])['resourceId']))
    expect('invitation-keyless-public-null-join', request('/v1/rooms/' + public_room + '/join', other_token, {'requestId': str(uuid.uuid4()), 'token': None}), 202)
    restart_api(True)
    canceled_receipt = expect('invitation-canceled-delivery-is-historical', request(issue_path + '/disposition', owner_token, issue), 200)
    if object_json(canceled_receipt['data']) != {'state': 'accepted', 'response': issued} or realm.sql("SELECT state FROM app.invitation WHERE id='" + invitation + "'") != 'canceled':
        raise RuntimeError('Historical capability recovery reactivated invitation')
    # Published configuration is an explicit SQL-only fixture, not engine qualification.
    queue_profile = str(uuid.uuid4())
    configuration = {'version': 'matchmaking-profile-v1', 'influence': 'none', 'planechase': False,
        'qualificationRef': 'sha256:' + '1' * 64,
        'configurations': [{'key': 'duel', 'formatId': format_id, 'seatCount': '2', 'structure': 'bo1', 'teams': [None, None]}]}
    realm.sql("INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES('" + queue_profile + "','" + queue_profile + "','fixture','fixture',2,2,'fixture:sql-only','" + json.dumps(configuration).replace("'", "''") + "'::jsonb)")
    profiles = expect('queue-published-profiles', request('/v1/table-profiles?limit=100', owner_token), 200)
    if not any(object_json(item).get('id') == queue_profile and object_json(item).get('hasMatchmaking') is True for item in object_json(profiles['data']).get('items', [])):
        raise RuntimeError('Published queue profile missing')
    configurations = expect('queue-profile-configurations', request('/v1/table-profiles/' + queue_profile + '/configurations?limit=1', owner_token), 200)
    config_data = object_json(configurations['data'])
    if config_data.get('nextIndex') != '0' or set(object_json(config_data['items'][0])) != {'index', 'key', 'formatId', 'seatCount', 'structure'}:
        raise RuntimeError('Configuration page leaked private source or lost ordinal')
    expect('queue-profile-after-end', request('/v1/table-profiles/' + queue_profile + '/configurations?afterIndex=0&limit=1', owner_token), 200)
    consent = {'requestId': str(uuid.uuid4()), 'mode': 'open', 'competitive': False,
        'alternatives': [{'deckRevisionId': revision, 'choices': [{'profileId': queue_profile,
            'configurationKey': 'duel', 'maximumAiSeats': '0', 'maximumSponsoredAiSeats': '0'}]}]}
    expect('queue-unknown', request('/v1/queue/disposition', owner_token, consent), 200)
    enqueued = expect('queue-original-consent', request('/v1/queue', owner_token, consent), 202)
    queue_ack = object_json(enqueued['data'])
    queue_entry = str(uuid.UUID(queue_ack['resourceId']))
    queue_intent = str(uuid.UUID(queue_ack['intentId']))
    if set(queue_ack) != {'resourceId', 'intentId'}:
        raise RuntimeError('Queue acknowledgment lost an identity')
    if expect('queue-exact-replay', request('/v1/queue', owner_token, consent), 202) != enqueued:
        raise RuntimeError('Queue replay created a new intent')
    status = expect('queue-owner-status', request('/v1/queue?intentId=' + queue_intent, owner_token), 200)
    status_items = object_json(status['data']).get('items', [])
    if len(status_items) != 1 or object_json(status_items[0]).get('state') != 'queued':
        raise RuntimeError('Queue status differs from actual intake')
    original = expect('queue-owner-original-intent', request('/v1/queue/intents/' + queue_intent, owner_token), 200)
    original_data = object_json(original['data'])
    if original_data.get('alternatives') != consent['alternatives'] or original_data.get('requestId') != consent['requestId'] or 'qualificationRef' in json.dumps(original_data):
        raise RuntimeError('Original queue intent lost consent or leaked trusted metadata')
    expect('queue-foreign-original-denied', request('/v1/queue/intents/' + queue_intent, other_token), 404)
    foreign_status = expect('queue-foreign-status-isolated', request('/v1/queue?intentId=' + queue_intent, other_token), 200)
    if object_json(foreign_status['data']).get('items') != []:
        raise RuntimeError('Foreign queue status leaked')
    expect('queue-foreign-deck-rejected', request('/v1/queue', other_token, dict(consent, requestId=str(uuid.uuid4()))), 409)
    other_import = expect('queue-other-library-import', request('/v1/decks/import', other_token, dict(intent, requestId=str(uuid.uuid4()), name='Other queue library')), 202)
    other_revision = str(uuid.UUID(object_json(other_import['data'])['revisionId']))
    other_consent = {'requestId': str(uuid.uuid4()), 'mode': 'open', 'competitive': False,
        'alternatives': [{'deckRevisionId': other_revision, 'choices': consent['alternatives'][0]['choices']}]}
    other_enqueued = expect('queue-other-original-consent', request('/v1/queue', other_token, other_consent), 202)
    other_entry = str(uuid.UUID(object_json(other_enqueued['data'])['resourceId']))
    # Explicit trusted SQL matcher fixture only; no human HTTP endpoint can choose this roster.
    selected = {'profileId': queue_profile, 'configurationKey': 'duel', 'hostEntryId': queue_entry,
        'humanSeats': [{'queueEntryId': queue_entry, 'seatIndex': '0'}, {'queueEntryId': other_entry, 'seatIndex': '1'}], 'aiSeats': []}
    claimed = object_json(json.loads(realm.sql("SELECT app.claim_matchmaking('" + json.dumps(selected).replace("'", "''") + "'::jsonb,'" + str(uuid.uuid4()) + "')")))
    if claimed.get('status') != 'success':
        raise RuntimeError('Trusted SQL-only matcher fixture failed')
    claimed_room = str(uuid.UUID(object_json(claimed['data'])['resourceId']))
    claimed_status = expect('queue-claimed-status', request('/v1/queue?intentId=' + queue_intent, owner_token), 200)
    if object_json(object_json(claimed_status['data'])['items'][0]).get('claimedRoomId') != claimed_room:
        raise RuntimeError('Queue status lost trusted claim')
    cancellation = {'requestId': str(uuid.uuid4())}
    cancel_path = '/v1/queue/' + queue_entry + '/cancel'
    canceled = expect('queue-whole-intent-cancel', request(cancel_path, owner_token, cancellation), 202)
    if object_json(canceled['data']) != {'resourceId': queue_entry} or realm.sql("SELECT state FROM app.room WHERE id='" + claimed_room + "'") != 'closed':
        raise RuntimeError('Queue cancellation did not close claimed room')
    if realm.sql("SELECT count(*) FROM app.queue_entry WHERE claimed_room_id='" + claimed_room + "' AND state='canceled'") != '2':
        raise RuntimeError('Queue cancellation did not release both intents')
    expect('queue-cancel-disposition', request(cancel_path + '/disposition', owner_token, cancellation), 200)
    established = expect('queue-original-recover-after-cancel', request('/v1/queue/disposition', owner_token, consent), 200)
    if object_json(established['data']) != {'state': 'accepted', 'response': enqueued}:
        raise RuntimeError('Cancellation changed original enqueue receipt')
    # Canonical SQL admission fixture with explicit synthetic qualification/artifact refs.
    # This checks real HTTP/Auth/chat persistence, not native rules or playable decks.
    peer_account = str(uuid.UUID(realm.sql("SELECT d.account_id FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id='" + other_revision + "'")))
    definition, card_set, printing, body_revision, release, legality = [str(uuid.uuid4()) for _ in range(6)]
    realm.sql("INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('" + definition + "','" + definition + "','SQL chat fixture'); INSERT INTO app.card_set(id,external_set_ref,name) VALUES('" + card_set + "','" + card_set + "','SQL chat fixture'); INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES('" + printing + "','" + definition + "','" + card_set + "','" + printing + "','common','nonfoil','fixture:sql-only'); INSERT INTO app.card_definition_revision(id,definition_id,revision_ref,rules_text_ref) VALUES('" + body_revision + "','" + definition + "','fixture','fixture:sql-only'); INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref) VALUES('" + release + "','" + release + "','fixture','fixture','fixture','fixture:sql-only'); INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land) VALUES('" + release + "','" + body_revision + "','fixture:sql-only',true); INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at) VALUES('" + legality + "','fixture:sql-only',clock_timestamp(),clock_timestamp()); INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES('" + definition + "','" + format_id + "','" + legality + "','legal')")
    chat_import = dict(intent, requestId=str(uuid.uuid4()), name='Chat source fixture', entries=[{'definitionId':definition,'printingId':printing,'section':'main','quantity':'2'}])
    chat_owner_deck = expect('chat-owner-source-library',request('/v1/decks/import',owner_token,chat_import),202)
    chat_peer_deck = expect('chat-peer-source-library',request('/v1/decks/import',other_token,dict(chat_import,requestId=str(uuid.uuid4()))),202)
    chat_owner_revision = str(uuid.UUID(object_json(chat_owner_deck['data'])['revisionId']))
    chat_peer_revision = str(uuid.UUID(object_json(chat_peer_deck['data'])['revisionId']))
    created = object_json(json.loads(realm.sql("SELECT app.create_room('" + owner + "','" + format_id + "','" + queue_profile + "','open',false,'public','" + str(uuid.uuid4()) + "')")))
    chat_room = str(uuid.UUID(object_json(created['data'])['resourceId']))
    def room_basis() -> str:
        return realm.sql("SELECT revision::text FROM app.room WHERE id='" + chat_room + "'")
    for player, selected_revision, joining in [(owner,chat_owner_revision,False),(peer_account,chat_peer_revision,True)]:
        if joining:
            joined_fixture = object_json(json.loads(realm.sql("SELECT app.join_room('" + player + "','" + chat_room + "',NULL,'" + str(uuid.uuid4()) + "')")))
            if joined_fixture['status'] != 'success':
                raise RuntimeError('Canonical chat fixture join failed')
        selected_fixture = object_json(json.loads(realm.sql("SELECT app.select_room_deck('" + player + "','" + chat_room + "'," + room_basis() + ",'" + selected_revision + "','" + str(uuid.uuid4()) + "')")))
        if selected_fixture['status'] != 'success':
            raise RuntimeError('Canonical chat fixture selection failed')
    countdown = object_json(json.loads(realm.sql("SELECT app.start_room_countdown('" + owner + "','" + chat_room + "'," + room_basis() + ",'fixture:sql-only','" + str(uuid.uuid4()) + "')")))
    if countdown['status'] != 'success':
        raise RuntimeError('Canonical chat fixture countdown failed')
    realm.sql("UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='" + chat_room + "'")
    started = object_json(json.loads(realm.sql("SELECT app.start_room_match('" + chat_room + "'," + room_basis() + ",'" + release + "','" + legality + "','fixture:sql-only',(SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:sql-only') ORDER BY seat_index) FROM app.room_slot WHERE room_id='" + chat_room + "' AND retired_at IS NULL),'" + str(uuid.uuid4()) + "')")))
    if started['status'] != 'success':
        raise RuntimeError('Canonical chat fixture admission failed')
    match_id = str(uuid.UUID(object_json(started['data'])['resourceId']))
    chat_path = '/v1/matches/' + match_id + '/chat'
    empty_chat = expect('chat-admitted-human-read',request(chat_path,owner_token),200)
    if object_json(empty_chat['data']).get('canSend') is not True or object_json(empty_chat['data']).get('items') != []:
        raise RuntimeError('Admitted chat projection differs')
    chat_send = {'requestId':str(uuid.uuid4()),'body':'Hello 🎲'}
    expect('chat-unknown-receipt',request(chat_path+'/messages/disposition',owner_token,chat_send),200)
    sent_chat = expect('chat-human-send',request(chat_path+'/messages',owner_token,chat_send),202)
    if expect('chat-exact-replay',request(chat_path+'/messages',owner_token,chat_send),202) != sent_chat:
        raise RuntimeError('Chat replay differs')
    expect('chat-conflicting-replay',request(chat_path+'/messages',owner_token,dict(chat_send,body='changed')),409)
    chat_history = expect('chat-peer-history',request(chat_path+'?limit=1',other_token),200)
    history_data = object_json(chat_history['data'])
    history_item = object_json(history_data['items'][0])
    if history_item.get('senderId') != owner or history_item.get('body') != chat_send['body'] or 'admissionReceiptId' in json.dumps(history_data):
        raise RuntimeError('Chat history lost identity or exposed private context')
    expect('chat-anonymous-denied',request(chat_path,None),401)
    expect('chat-invalid-cursor',request(chat_path+'?beforeId='+str(uuid.uuid4()),owner_token),400)
    # Structural owned producer fixtures isolate recipient projection/acknowledgment semantics.
    notification = str(uuid.uuid4())
    realm.sql("INSERT INTO app.notification(id,recipient_id,source_kind,source_ref,body) VALUES('" + notification + "','" + owner + "','trade_offer','" + str(uuid.uuid4()) + "','Trade offer received')")
    notice = expect('notifications-own-page',request('/v1/notifications',owner_token),200)
    if not any(object_json(item).get('id') == notification for item in object_json(notice['data'])['items']):
        raise RuntimeError('Own notification missing')
    notice_foreign = expect('notifications-foreign-page',request('/v1/notifications',other_token),200)
    if any(object_json(item).get('id') == notification for item in object_json(notice_foreign['data'])['items']):
        raise RuntimeError('Foreign notification exposed')
    mark = {'requestId':str(uuid.uuid4()),'notificationIds':[notification]}
    expect('notifications-unknown',request('/v1/notifications/read/disposition',owner_token,mark),200)
    expect('notifications-foreign-ack-denied',request('/v1/notifications/read',other_token,dict(mark,requestId=str(uuid.uuid4()))),409)
    marked = expect('notifications-owned-ack',request('/v1/notifications/read',owner_token,mark),202)
    first_read = realm.sql("SELECT read_at::text FROM app.notification WHERE id='" + notification + "'")
    if not first_read:
        raise RuntimeError('Notification acknowledgment not persisted')
    if expect('notifications-exact-replay',request('/v1/notifications/read',owner_token,mark),202) != marked:
        raise RuntimeError('Notification replay differs')
    recovered_notice = expect('notifications-recovery',request('/v1/notifications/read/disposition',owner_token,mark),200)
    if object_json(recovered_notice['data']) != {'state':'accepted','response':marked} or realm.sql("SELECT read_at::text FROM app.notification WHERE id='" + notification + "'") != first_read:
        raise RuntimeError('Notification recovery changed first read time')
    expect('notifications-anonymous-denied',request('/v1/notifications',None),401)
    # Crop reference HTTP qualification only. The existing synthetic printing ref is not loadable-art evidence.
    profile_path = '/v1/account/profile'
    current_profile = expect('profile-own-reference-read',request(profile_path,owner_token),200)
    if object_json(current_profile['data']) != {'accountId':owner,'cardArtProfile':None,'supporter':False}:
        raise RuntimeError('Initial own crop projection differs')
    crop = {'printingId':printing,'cropX':0.10000000000000002,'cropY':0.3333333333333333,'scale':1.0000000000000002}
    crop_intent = {'requestId':str(uuid.uuid4()),'expected':None,'cardArtProfile':crop}
    expect('profile-art-unknown',request(profile_path+'/disposition',owner_token,crop_intent),200)
    crop_accepted = expect('profile-art-set',request(profile_path,owner_token,crop_intent),202)
    if object_json(crop_accepted['data']) != {'resourceId':owner}:
        raise RuntimeError('Profile edit acknowledgment not owner-bound')
    own_crop = expect('profile-art-own-round-trip',request(profile_path,owner_token),200)
    if object_json(own_crop['data']).get('cardArtProfile') != crop:
        raise RuntimeError('Crop float round trip differs')
    peer_view = expect('profile-art-active-public-view',request('/v1/players/'+owner+'/profile',other_token),200)
    public_crop = object_json(peer_view['data'])
    if public_crop.get('viewerAccountId') != peer_account or public_crop.get('cardArtProfile') != crop or set(public_crop) != {'viewerAccountId','person','cardArtProfile','supporter'}:
        raise RuntimeError('Public crop context or safe allowlist differs')
    if expect('profile-art-exact-replay',request(profile_path,owner_token,crop_intent),202) != crop_accepted:
        raise RuntimeError('Crop replay changed acknowledgment')
    recovered_crop = expect('profile-art-recover',request(profile_path+'/disposition',owner_token,crop_intent),200)
    if object_json(recovered_crop['data']) != {'state':'accepted','response':crop_accepted}:
        raise RuntimeError('Crop receipt recovery differs')
    expect('profile-art-stale-edit',request(profile_path,owner_token,dict(crop_intent,requestId=str(uuid.uuid4()))),409)
    changed_crop = dict(crop_intent,cardArtProfile=dict(crop,cropX=0.5))
    expect('profile-art-conflicting-lookup',request(profile_path+'/disposition',owner_token,changed_crop),409)
    extreme_crop = dict(crop,scale=float.fromhex('0x1.fffffffffffffp+1023'))
    expect('profile-art-max-finite-edit',request(profile_path,owner_token,{'requestId':str(uuid.uuid4()),'expected':crop,'cardArtProfile':extreme_crop}),202)
    extreme_read = expect('profile-art-max-finite-round-trip',request(profile_path,owner_token),200)
    if object_json(extreme_read['data']).get('cardArtProfile') != extreme_crop:
        raise RuntimeError('Maximum finite crop failed actual HTTP round trip')
    realm.sql("UPDATE app.printing SET image_ref=NULL WHERE id='" + printing + "'")
    expect('profile-art-clear-unavailable-image',request(profile_path,owner_token,{'requestId':str(uuid.uuid4()),'expected':extreme_crop,'cardArtProfile':None}),202)
    expect('profile-art-missing-image-rejected',request(profile_path,owner_token,dict(crop_intent,requestId=str(uuid.uuid4()))),409)
    expect('profile-art-malformed-nullable',request(profile_path,owner_token,{'requestId':str(uuid.uuid4()),'cardArtProfile':None}),400)
    expect('profile-art-no-selected-owner',request(profile_path+'?owner='+peer_account,owner_token),400)
    expect('profile-art-invalid-public-path',request('/v1/players/%FF/profile',owner_token),400)
    expect('profile-art-anonymous-denied',request(profile_path,None),401)
    # Public identifier spellings are exact; opaque names remain ordinary text.
    canonical_id = '11111111-1111-1111-1111-111111111111'
    canonical_import = dict(intent,requestId=canonical_id,name='AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA')
    exact_ids = expect('canonical-non-rfc-request',request('/v1/decks/import',owner_token,canonical_import),202)
    exact_receipt = expect('canonical-original-receipt',request('/v1/decks/import/disposition',owner_token,canonical_import),200)
    if object_json(exact_receipt['data']) != {'state':'accepted','response':exact_ids}:
        raise RuntimeError('Canonical request identity did not retain its exact receipt')
    before_aliases = realm.sql("SELECT count(*) FROM app.deck WHERE account_id='" + owner + "'")
    for label, spelling in [('upper','AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA'),('simple','aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa'),('urn','urn:uuid:'+canonical_id),('braces','{'+canonical_id+'}')]:
        expect('canonical-reject-body-'+label,request('/v1/decks/import',owner_token,dict(canonical_import,requestId=spelling)),400)
    if realm.sql("SELECT count(*) FROM app.deck WHERE account_id='" + owner + "'") != before_aliases:
        raise RuntimeError('Invalid identifier created a deck or command effect')
    expect('canonical-reject-query',request('/v1/decks?after=urn:uuid:'+canonical_id,owner_token),400)
    expect('canonical-reject-utf8-path',request('/v1/decks/%FF',owner_token),400)
    empty_deck = expect('canonical-empty-deck-create',request('/v1/decks',owner_token,{'requestId':str(uuid.uuid4()),'name':'Explicit empty revision','mode':'open','formatId':format_id}),202)
    empty_deck_id = str(uuid.UUID(object_json(empty_deck['data'])['resourceId']))
    empty_detail = expect('canonical-empty-deck-detail',request('/v1/decks/'+empty_deck_id,owner_token),200)
    detail_data = object_json(empty_detail['data'])
    detail_deck = object_json(detail_data['deck'])
    if 'currentRevisionId' not in detail_deck or detail_deck['currentRevisionId'] is not None or detail_data.get('revision') is not None:
        raise RuntimeError('Empty deck nullable revision was omitted or changed')
    expect('canonical-reject-owned-path-alias',request('/v1/decks/%7B'+empty_deck_id+'%7D',owner_token),400)
    final_library = expect('canonical-omitted-page-cursor',request('/v1/decks',owner_token),200)
    library_items = object_json(final_library['data'])['items']
    if not any(object_json(item).get('name') == canonical_import['name'] for item in library_items):
        raise RuntimeError('UUID-looking deck name was rewritten or rejected')
    # Actual original caller JWT through Rust HTTP, with canonical SQL effect witnesses.
    enum_requests = [str(uuid.uuid4()) for _ in range(5)]
    enum_ids = ",".join("'" + value + "'" for value in enum_requests)
    enum_snapshot = ("SELECT jsonb_build_object('decks',(SELECT count(*) FROM app.deck WHERE account_id='" + owner + "'),"
        "'rooms',(SELECT count(*) FROM app.room WHERE host_id='" + owner + "'),"
        "'receipts',(SELECT count(*) FROM app.command_receipt WHERE account_id='" + owner + "' AND request_ref IN(" + enum_ids + ")))" )
    # The exact original canonical room intent remains a successful replay control.
    expect('enum-room-canonical-control', request('/v1/rooms/commands', owner_token, room_intent),202)
    before_enums = realm.sql(enum_snapshot)
    witnesses: list[tuple[str,str,dict]] = []
    for request_id, malformed_mode, suffix in zip(enum_requests[:2], ({'open': None}, ['open']), ('tagged-object', 'array')):
        witnesses.append(('enum-deck-mode-' + suffix, '/v1/decks',
            {'requestId':request_id,'name':'Enum boundary fixture','mode':malformed_mode,'formatId':format_id}))
    for request_id, malformed_visibility, suffix in zip(enum_requests[2:4], ({'private':None},['private']),('tagged-object','array')):
        witnesses.append(('enum-room-visibility-' + suffix,'/v1/rooms/commands',
            {'kind':'create','command':dict(room_intent['command'],requestId=request_id,visibility=malformed_visibility)}))
    witnesses.append(('enum-room-kind-tagged-object','/v1/rooms/commands',
        {'kind':{'create':None},'command':dict(room_intent['command'],requestId=enum_requests[4])}))
    for label,path,intent in witnesses:
        status,payload,headers = request_with_headers(path,owner_token,intent)
        rejected = expect(label,(status,payload),400)
        if rejected.get('status') != 'fail' or not isinstance(rejected.get('data'),dict):
            raise RuntimeError('Malformed enum did not return the standard fail envelope')
        if headers.get('cache-control') != 'no-store':
            raise RuntimeError('Malformed enum response omitted no-store')
        checks[-1]['noStore'] = True
    if realm.sql(enum_snapshot) != before_enums:
        raise RuntimeError('Malformed enum created a deck, room or canonical command receipt')
    # Retain valid original terms on existing resources: only the unsupported
    # URI query differs from ordinary execute/disposition/detail requests.
    current_account=expect('query-canonical-account-control',request('/v1/account',owner_token),200)
    account_defaults=object_json(object_json(current_account['data'])['defaults'])
    settings={'name':detail_deck['name'],'mode':detail_deck['mode'],
        'formatId':object_json(detail_deck['format'])['id'],'appearance':detail_deck['appearance']}
    query_ids=[str(uuid.uuid4()) for _ in range(6)]
    create_query={'requestId':query_ids[0],'name':'Query boundary fixture','mode':'open','formatId':format_id}
    witnesses=[
        ('query-deck-create','/v1/decks',create_query),
        ('query-deck-save','/v1/decks/'+empty_deck_id+'/revisions',{'requestId':query_ids[1],'deckId':empty_deck_id,'expectedRevisionId':None,'entries':[]}),
        ('query-deck-settings','/v1/decks/'+empty_deck_id+'/settings',{'requestId':query_ids[2],'deckId':empty_deck_id,'expected':settings,'settings':settings}),
        ('query-deck-disposition','/v1/decks/commands/disposition',{'kind':'create','command':dict(create_query,requestId=query_ids[3])}),
        ('query-deck-detail','/v1/decks/'+empty_deck_id,None),
        ('query-defaults-change','/v1/account/defaults',{'requestId':query_ids[4],'expected':account_defaults,'appearance':account_defaults}),
        ('query-defaults-disposition','/v1/account/defaults/disposition',{'requestId':query_ids[5],'expected':account_defaults,'appearance':account_defaults}),
    ]
    refs=','.join("'"+identity+"'" for identity in query_ids)
    snapshot=("SELECT jsonb_build_object('decks',(SELECT count(*) FROM app.deck WHERE account_id='"+owner+"'),"
        "'revisions',(SELECT count(*) FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE d.account_id='"+owner+"'),"
        "'receipts',(SELECT count(*) FROM app.command_receipt WHERE account_id='"+owner+"' AND request_ref IN("+refs+")),"
        "'profile',(SELECT to_jsonb(p) FROM app.profile p WHERE p.account_id='"+owner+"'))")
    original_snapshot=realm.sql(snapshot)
    for label,path,terms in witnesses:
        status,payload,headers=request_with_headers(path+'?unexpected=1',owner_token,terms)
        rejected=expect(label,(status,payload),400)
        if rejected != {'status':'fail','data':{'query':'unsupported'}} or headers.get('cache-control')!='no-store':
            raise RuntimeError('Unsupported query did not preserve private standard failure')
        checks[-1]['noStore']=True
    if realm.sql(snapshot)!=original_snapshot:
        raise RuntimeError('Unsupported URI queries changed decks, revisions, defaults or receipts')
    from human_api_acceptance import qualify_human_api
    qualify_human_api(realm, request, expect, owner_token, other_token, owner, peer_account,
        {'format_id':format_id,'profile_id':queue_profile,'match_id':match_id,'release':release,
         'legality':legality,'definition':definition,'printing':printing},fresh_identity)
    from legacy_api_acceptance import qualify_legacy_api
    qualify_legacy_api(realm, request, expect, owner_token, other_token, owner, peer_account, fresh_identity,
        {'format_id':format_id,'profile_id':queue_profile,'match_id':match_id,'release':release,
         'legality':legality,'definition':definition,'printing':printing})
    from economy_api_acceptance import qualify_economy_api
    prediction_printing = qualify_economy_api(realm, request, expect, owner_token, other_token, owner, peer_account, fresh_identity)
    def configure(settings: dict[str, str]) -> None:
        api_environment.update(settings)
        restart_api(True)

    temporal_result: dict = {'executed': False}
    def prepare_runtime(path: str, slot: str) -> None:
        from temporal_api_acceptance import qualify_temporal_preparation
        temporal_result.update(qualify_temporal_preparation(realm, request, expect, configure,
            owner_token, other_token, owner, peer_account, path, prediction_printing, slot))
        temporal_result['executed'] = True

    from prediction_api_acceptance import qualify_prediction_api
    qualify_prediction_api(realm, request, expect, owner_token, other_token, owner, peer_account,
        prediction_printing, prepare_runtime if temporal_preparation else None)
    from provider_api_acceptance import qualify_provider_api
    paid_back = qualify_provider_api(realm, request, expect, request_raw, configure, owner_token, other_token, owner, peer_account,
        {'format_id':format_id,'profile_id':queue_profile,'match_id':match_id,'release':release,
         'legality':legality,'definition':definition,'printing':printing})
    from deck_defaults_api_acceptance import qualify_deck_defaults_api
    qualify_deck_defaults_api(realm, request, expect, owner_token, other_token, owner, paid_back,
        {'format_id':format_id,'profile_id':queue_profile,'match_id':match_id,'release':release,
         'legality':legality,'definition':definition,'printing':printing})
    from quest_reroll_api_acceptance import qualify_quest_reroll_api
    qualify_quest_reroll_api(realm, request, expect, owner_token, other_token, owner, peer_account)
    with progress_path.open('ab') as journal:
        journal.write(json.dumps({'complete':True,'checksCount':len(checks),'scope':'completed managed HTTP checks with source-coded semantic assertions'}).encode() + b'\n')
    return {'passed': True, 'checks': checks, 'scope': 'actual-rust-http-managed-jwt-postgrest-current-human-boundaries',
            'nativeAdmissionQualified': False,
            'nativeReceiptsHttp':native_receipts,
            'temporalPreparation': temporal_result,
            'operationObservations': [{'method': method, 'route': route, 'status': status, 'requests': count}
                for (method, route, status), count in sorted(observations.items())],
            'operationObservationScope': 'Requested literal route templates and actual statuses only; no concrete IDs/query/body/header data. A status observation is not semantic endpoint acceptance.'}
