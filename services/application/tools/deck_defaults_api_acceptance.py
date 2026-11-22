"""Actual caller defaults and saved deck writes after a verified synthetic purchase."""
import uuid
from human_api_acceptance import Request, Expect, Fixture
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def qualify_deck_defaults_api(realm: DisposableSupabase, request: Request, expect: Expect,
                              owner_token: str, peer_token: str, owner: str,
                              paid_back: str, fixture: Fixture) -> None:
    """Prove copied defaults, immutable revisions and complete-snapshot recovery."""
    def post(label: str, path: str, body: dict, token: str = owner_token, status: int = 202) -> dict:
        return expect('saved-' + label, request(path, token, body), status)

    def get(label: str, path: str) -> dict:
        payload = expect('saved-' + label, request(path, owner_token, None), 200)
        if payload.get('status') != 'success':
            raise RuntimeError('Saved deck HTTP expected standard success')
        return object_json(payload['data'])

    def resource(payload: dict) -> str:
        return str(uuid.UUID(object_json(payload['data'])['resourceId']))

    def recover(label: str, path: str, original: dict, accepted: dict) -> None:
        result = post(label, path, original, status=200)
        if object_json(result['data']) != {'state': 'accepted', 'response': accepted}:
            raise RuntimeError('Saved/default receipt did not retain its complete original input')

    account = get('original-defaults', '/v1/account')
    if account['accountId'] != owner:
        raise RuntimeError('Account defaults changed their authenticated owner')
    original = object_json(account['defaults'])
    selected = dict(original, backId=str(uuid.UUID(paid_back)))
    change = {'requestId': str(uuid.uuid4()), 'expected': original, 'appearance': selected}
    changed = post('defaults-change', '/v1/account/defaults', change)
    recover('defaults-recover', '/v1/account/defaults/disposition', change, changed)
    after = get('changed-defaults', '/v1/account')
    # appearanceRevision invalidates entitlement changes. Defaults are a separate
    # observed-value snapshot and changing them does not create/revoke a grant.
    if after['defaults'] != selected or after['appearanceRevision'] != account['appearanceRevision']:
        raise RuntimeError('Defaults did not atomically adopt the entitled appearance')

    created = post('deck-create-from-defaults', '/v1/decks', {'requestId': str(uuid.uuid4()), 'name': 'API copied defaults', 'mode': 'open', 'formatId': fixture['format_id']})
    deck = resource(created)
    detail_path = '/v1/decks/' + deck
    detail = get('new-deck-defaults', detail_path)
    if object_json(detail['deck'])['appearance'] != selected or detail['revision'] is not None:
        raise RuntimeError('New deck did not copy current defaults into its own settings')
    restore = {'requestId': str(uuid.uuid4()), 'expected': selected, 'appearance': original}
    restored = post('defaults-restore', '/v1/account/defaults', restore)
    recover('defaults-restore-recover', '/v1/account/defaults/disposition', restore, restored)
    recover('old-defaults-after-later-change', '/v1/account/defaults/disposition', change, changed)
    if object_json(get('existing-deck-keeps-copy', detail_path)['deck'])['appearance'] != selected:
        raise RuntimeError('Changing account defaults rewrote an existing deck')

    entry = {'definitionId': fixture['definition'], 'printingId': fixture['printing'], 'section': 'main', 'quantity': '2'}
    save = {'requestId': str(uuid.uuid4()), 'deckId': deck, 'expectedRevisionId': None, 'entries': [entry]}
    revision = post('deck-save', detail_path + '/revisions', save)
    first_id = resource(revision)
    recover('deck-save-recover', '/v1/decks/commands/disposition', {'kind': 'save', 'command': save}, revision)
    current = get('first-revision', detail_path)
    if object_json(current['revision'])['id'] != first_id or object_json(current['revision'])['entries'] != [entry]:
        raise RuntimeError('First saved revision lost its exact contents')
    second = dict(save, requestId=str(uuid.uuid4()), expectedRevisionId=first_id, entries=[dict(entry, quantity='3')])
    advanced = post('deck-save-next', detail_path + '/revisions', second)
    second_id = resource(advanced)
    if post('deck-save-original-repeat', detail_path + '/revisions', save) != revision:
        raise RuntimeError('Old content request did not preserve its original accepted revision')
    recover('old-revision-after-new-head', '/v1/decks/commands/disposition', {'kind': 'save', 'command': save}, revision)
    current = get('head-kept-after-replay', detail_path)
    if object_json(current['revision'])['id'] != second_id or realm.sql("SELECT count(*) FROM app.deck_revision WHERE deck_id='" + deck + "'") != '2':
        raise RuntimeError('Content replay created another revision or reactivated the old head')

    summary = object_json(current['deck'])
    baseline = {'name': summary['name'], 'mode': summary['mode'], 'formatId': object_json(summary['format'])['id'], 'appearance': summary['appearance']}
    settings = {'requestId': str(uuid.uuid4()), 'deckId': deck, 'expected': baseline,
                'settings': dict(baseline, name='API verified settings', appearance=original)}
    edited = post('deck-settings', detail_path + '/settings', settings)
    recover('deck-settings-recover', '/v1/decks/commands/disposition', {'kind': 'settings', 'command': settings}, edited)
    if post('deck-settings-repeat', detail_path + '/settings', settings) != edited:
        raise RuntimeError('Settings replay changed its original acknowledgment')
    actual = object_json(get('settings-final', detail_path)['deck'])
    if actual['name'] != 'API verified settings' or actual['appearance'] != original or actual['currentRevisionId'] != second_id:
        raise RuntimeError('Settings mutation changed content or lost its entitled selection')
    foreign = dict(second, requestId=str(uuid.uuid4()), expectedRevisionId=second_id)
    post('existing-deck-foreign-save-denied', detail_path + '/revisions', foreign, peer_token, 409)
    if realm.sql("SELECT count(*) FROM app.deck_revision WHERE deck_id='" + deck + "'") != '2':
        raise RuntimeError('Foreign save created a revision on another account deck')
    stale = dict(restore, requestId=str(uuid.uuid4()))
    post('defaults-stale-denied', '/v1/account/defaults', stale, status=409)
    if get('defaults-final', '/v1/account')['defaults'] != original:
        raise RuntimeError('Stale defaults command changed the current account appearance')
