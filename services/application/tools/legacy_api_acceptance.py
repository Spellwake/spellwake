"""Current real HTTP checks for existing catalogue, store and social surfaces.

All catalogue/admission inputs are isolated synthetic fixtures. No feed, payment,
provider account or native gameplay is acquired or qualified here.
"""
import urllib.parse
import uuid
import base64
import hashlib
import secrets
from typing import Callable
from human_api_acceptance import Request, Expect, Fixture
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def qualify_legacy_api(realm: DisposableSupabase, request: Request, expect: Expect,
                       owner_token: str, other_token: str, owner: str, peer: str,
                       fresh_identity: Callable[[], str],
                       fixture: Fixture) -> None:
    """Use existing actual JWTs and the same owned HTTP process and SQL realm."""
    def get(label: str, path: str, token: str | None = owner_token, status: int = 200) -> dict:
        return expect('legacy-' + label, request(path, token, None), status)

    def post(label: str, path: str, body: dict, token: str | None = owner_token, status: int = 202) -> dict:
        return expect('legacy-' + label, request(path, token, body), status)

    def data(payload: dict) -> dict:
        if payload.get('status') != 'success':
            raise RuntimeError('Legacy HTTP expected a standard success envelope')
        value = object_json(payload['data'])
        if 'accountId' in value and value['accountId'] != owner:
            raise RuntimeError('Legacy HTTP projection changed the caller identity')
        return value

    # Fresh fixture data may be empty, but each maintained caller projection must
    # actually execute through Auth/RLS and retain required nullable pagination.
    pages = [('/v1/inventory', 'inventory'), ('/v1/cosmetics', 'cosmetics'),
             ('/v1/formats', 'formats'), ('/v1/purchases', 'purchases'),
             ('/v1/trades', 'trades'), ('/v1/market/listings', 'listings'),
             ('/v1/packs/products', 'products'), ('/v1/predictions/markets', 'markets'),
             ('/v1/predictions/orders', 'orders'), ('/v1/predictions/positions', 'positions'),
             ('/v1/sets', 'sets'), ('/v1/cards', 'cards')]
    for path, label in pages:
        page = data(get(label, path + '?limit=1'))
        if not isinstance(page.get('items'), list) or len(page['items']) > 1 or 'nextCursor' not in page:
            raise RuntimeError('Legacy HTTP bounded page shape differs')
        get(label + '-anonymous', path, None, 401)
        get(label + '-invalid-limit', path + '?limit=0', status=400)
    data(get('session', '/v1/session'))
    get('session-anonymous', '/v1/session', None, 401)
    definition = str(uuid.UUID(fixture['definition']))
    printing = str(uuid.UUID(fixture['printing']))
    name = realm.sql("SELECT name FROM app.card_definition WHERE id='" + definition + "'")
    for path, label in [('/v1/cards/' + definition + '/printings?limit=1', 'definition-printings'),
                        ('/v1/cards/printings?name=' + urllib.parse.quote(name, safe='') + '&limit=1', 'named-printings')]:
        page = data(get(label, path))
        if not isinstance(page.get('items'), list) or len(page['items']) != 1 or page['items'][0]['definitionId'] != definition:
            raise RuntimeError('Legacy printing projection lost its exact definition')
    labels = data(post('catalogue-labels', '/v1/catalogue/resolve',
                       {'definitionIds': [definition], 'printingIds': [printing]}, status=200))
    if [row['id'] for row in labels['cards']] != [definition] or [row['id'] for row in labels['printings']] != [printing]:
        raise RuntimeError('Legacy catalogue resolver changed requested identities')
    # Foreign/missing detail reads must establish canonical absence, not return a
    # fabricated empty object. These do not qualify positive purchase/opening flows.
    for path, label in [('/v1/trades/', 'trade'), ('/v1/market/listings/', 'listing'),
                        ('/v1/purchases/', 'purchase'), ('/v1/packs/openings/', 'opening'),
                        ('/v1/predictions/markets/', 'market')]:
        get(label + '-missing', path + str(uuid.uuid4()), status=404)

    # Use ordinary friendship commands to establish contact, then exercise DMs,
    # exact persisted recovery, recipient read state and a changed-terms conflict.
    friendship = {'kind': 'friendship', 'command': {'requestId': str(uuid.uuid4()), 'otherAccountId': peer, 'action': 'request'}}
    offered = post('friend-request', '/v1/social/commands', friendship)
    recovered = post('friend-request-recover', '/v1/social/commands/disposition', friendship, status=200)
    if object_json(recovered['data']) != {'state': 'accepted', 'response': offered}:
        raise RuntimeError('Legacy social command did not recover exact accepted terms')
    post('friend-accept', '/v1/social/commands', {'kind': 'friendship', 'command': {'requestId': str(uuid.uuid4()), 'otherAccountId': owner, 'action': 'accept'}}, other_token)
    opened = post('dm-open', '/v1/social/commands', {'kind': 'open_dm', 'command': {'requestId': str(uuid.uuid4()), 'otherAccountId': peer}})
    conversation = str(uuid.UUID(object_json(opened['data'])['resourceId']))
    sent = post('dm-send-peer', '/v1/social/commands', {'kind': 'send_dm', 'command': {'requestId': str(uuid.uuid4()), 'conversationId': conversation, 'body': 'Owned real HTTP fixture message'}}, other_token)
    message = str(uuid.UUID(object_json(sent['data'])['resourceId']))
    page = data(get('dm-page', '/v1/social/conversations/' + conversation + '/messages?limit=1'))
    if page['conversationId'] != conversation or [row['id'] for row in page['items']] != [message]:
        raise RuntimeError('Legacy DM history changed admitted conversation/message')
    read = {'requestId': str(uuid.uuid4()), 'conversationId': conversation, 'messageIds': [message]}
    accepted = post('dm-mark-read', '/v1/social/read', read)
    recovered = post('dm-read-recover', '/v1/social/read/disposition', read, status=200)
    if object_json(recovered['data']) != {'state': 'accepted', 'response': accepted}:
        raise RuntimeError('Legacy read command did not recover exact accepted terms')
    status = data(get('social-status', '/v1/social/status?personId=' + peer))
    if status['personId'] != peer or status['unreadMessages'] != '0':
        raise RuntimeError('Legacy social status/read state differs')
    data(get('social-threads', '/v1/social/threads?limit=1'))
    username = realm.sql("SELECT username FROM app.account WHERE id='" + str(uuid.UUID(peer)) + "'")
    people = data(get('social-directory', '/v1/social/people?query=' + urllib.parse.quote(username, safe='') + '&limit=1'))
    if [row['id'] for row in people['items']] != [peer]:
        raise RuntimeError('Legacy social directory changed the searched peer')
    changed = dict(friendship, command=dict(friendship['command'], action='remove'))
    post('social-recovery-terms-conflict', '/v1/social/commands/disposition', changed, status=409)

    # A separately authenticated identity exercises real HTTP registration and
    # deletion without changing the continuing owner/peer fixture accounts.
    fresh = fresh_identity()
    session = object_json(get('fresh-session', '/v1/session', fresh)['data'])
    if session['state'] != 'needs_registration' or session['accountId'] is not None:
        raise RuntimeError('Fresh actual identity already had an application account')
    username = 'Api' + uuid.uuid4().hex[:16]
    registration = {'username': username}
    registered = post('fresh-registration', '/v1/account', registration, fresh, 200)
    account = str(uuid.UUID(object_json(registered['data'])['accountId']))
    if post('registration-repeat', '/v1/account', registration, fresh, 200) != registered:
        raise RuntimeError('Actual registration did not preserve one account')
    active = object_json(get('registered-session', '/v1/session', fresh)['data'])
    if active['state'] != 'active' or active['accountId'] != account:
        raise RuntimeError('Registered session lost its actual identity')
    key = secrets.token_bytes(32)
    intent = {'requestId': str(uuid.uuid4()), 'statusKeyHash': hashlib.sha256(key).hexdigest()}
    accepted = post('deletion-request', '/v1/account/deletion', intent, fresh)
    if object_json(accepted['data']) != {'resourceId': account}:
        raise RuntimeError('Deletion request changed its account')
    if post('deletion-request-replay', '/v1/account/deletion', intent, fresh) != accepted:
        raise RuntimeError('Deletion request did not recover its exact original acceptance')
    observer = {'accountId': account, 'requestId': intent['requestId'],
                'statusKey': base64.urlsafe_b64encode(key).decode().rstrip('=')}
    pending = post('deletion-observe-no-bearer', '/v1/account/deletion/observe', observer, None, 200)
    if object_json(pending['data']) != {'phase': 'pending', 'accountId': account, 'requestId': intent['requestId']}:
        raise RuntimeError('Observer inferred deletion completion without cleanup')
    wrong = dict(observer, statusKey=base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip('='))
    post('deletion-observer-wrong-capability', '/v1/account/deletion/observe', wrong, None, 404)
    post('deletion-pending-new-registration-denied', '/v1/account', {'username': username}, fresh, 409)
