"""Configured API provider boundaries with actual local storage and synthetic keys.

Stripe HTTP is the existing finite loopback fixture. Patreon start/status and an
unissued completion do not exchange a code or contact Patreon. Signed Patreon
notifications are retained/routed, not treated as paid membership evidence.
"""
import base64
import hashlib
import hmac
import json
import secrets
import threading
import time
import urllib.parse
import urllib.request
import uuid
from typing import Callable
from disposable_supabase import DisposableSupabase, ROOT
from human_api_acceptance import Request, Expect, Fixture
from qualification_http import make_opener, request_json, object_json
from qualify_stripe import Provider, KEY, WEBHOOK, VERSION

RawRequest = Callable[[str, bytes, dict[str, str]], tuple[int, object]]


def qualify_provider_api(realm: DisposableSupabase, request: Request, expect: Expect,
                         raw: RawRequest, configure: Callable[[dict[str, str]], None],
                         owner_token: str, peer_token: str, owner: str, peer: str, fixture: Fixture) -> str:
    """Exercise configured handlers, exact signatures and canonical benefit changes."""
    provider = Provider()
    thread = threading.Thread(target=provider.serve_forever, daemon=True)
    thread.start()
    try:
        cosmetic = str(uuid.uuid4())
        realm.sql("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free,display_name) VALUES('" + cosmetic + "','back','test-only:http-provider','active',false,'Synthetic HTTP provider cosmetic')")
        opener = make_opener()
        for bucket in ['payment-evidence', 'patreon-evidence']:
            status, _ = request_json(opener, urllib.request.Request(realm.status['API_URL'] + '/storage/v1/bucket',
                headers={'Content-Type': 'application/json', 'apikey': realm.status['SECRET_KEY']},
                data=json.dumps({'id': bucket, 'name': bucket, 'public': False}).encode()))
            if status != 200:
                raise RuntimeError('Owned private provider evidence bucket creation failed')
        patreon = object_json(json.loads((ROOT / 'tests/fixtures/patreon-source.json').read_text()))
        configure(dict(SPELLWAKE_STRIPE_SECRET_KEY=KEY, SPELLWAKE_STRIPE_WEBHOOK_SECRET=WEBHOOK,
            SPELLWAKE_STRIPE_ACCOUNT_ID='acct_synthetic', SPELLWAKE_STRIPE_PRICES=json.dumps({cosmetic: 'price_synthetic'}),
            SPELLWAKE_STRIPE_SUCCESS_URL='https://app.example/return', SPELLWAKE_STRIPE_CANCEL_URL='https://app.example/cancel',
            SPELLWAKE_PAYMENT_EVIDENCE_BUCKET='payment-evidence', SPELLWAKE_STRIPE_ALLOW_LOCAL_FIXTURE='1',
            SPELLWAKE_STRIPE_FIXTURE_BASE_URL=provider.base,
            SPELLWAKE_PATREON_CAMPAIGN_ID='123', SPELLWAKE_PATREON_CREATOR_ACCESS_TOKEN='patreon_test_creator_synthetic',
            SPELLWAKE_PATREON_CLIENT_ID='client_test', SPELLWAKE_PATREON_CLIENT_SECRET='patreon_test_client_secret',
            SPELLWAKE_PATREON_REDIRECT_URI='https://spellwake.example.invalid/auth/patreon',
            SPELLWAKE_PATREON_WEBHOOK_SECRET=patreon['secret'], SPELLWAKE_PATREON_EVIDENCE_BUCKET='patreon-evidence'))

        def post(label: str, path: str, body: dict, token: str | None = owner_token, status: int = 200) -> dict:
            return expect('provider-' + label, request(path, token, body), status)

        def counts() -> str:
            """Compare stable financial/evidence rows, excluding observation clocks."""
            return realm.sql("SELECT jsonb_build_object("
                "'purchases',(SELECT jsonb_agg(jsonb_build_array(id,state,stripe_payment_ref,stripe_checkout_ref) ORDER BY id) FROM app.purchase),"
                "'grants',(SELECT jsonb_agg(jsonb_build_array(id,state,purchase_id,provider_event_id) ORDER BY id) FROM app.cosmetic_grant),"
                "'events',(SELECT jsonb_agg(jsonb_build_array(id,provider,external_event_ref,processing_state,raw_evidence_state) ORDER BY id) FROM app.provider_event),"
                "'postings',(SELECT jsonb_agg(jsonb_build_array(id,inventory_asset_id,direction,quantity::text) ORDER BY id) FROM app.ledger_posting))")

        purchase_input = {'requestId': str(uuid.uuid4()), 'cosmeticId': cosmetic}
        reserved = post('purchase-reserve', '/v1/purchases', purchase_input, status=202)
        purchase = str(uuid.UUID(object_json(reserved['data'])['resourceId']))
        if post('purchase-reserve-repeat', '/v1/purchases', purchase_input, status=202) != reserved:
            raise RuntimeError('Configured checkout reservation changed its original identity')
        identifier = uuid.UUID(purchase).hex
        if len(provider.rows) != 1 or len(provider.keys) != 1 or provider.errors:
            raise RuntimeError('Checkout retry changed provider creation terms')
        expect('provider-purchase-existing-foreign-private', request('/v1/purchases/' + purchase, peer_token, None), 404)
        pending = object_json(expect('provider-purchase-pending', request('/v1/purchases/' + purchase, owner_token, None), 200)['data'])
        if pending['purchaseId'] != purchase or pending['state'] != 'pending':
            raise RuntimeError('Reserved purchase was presented as paid')

        def deliver(body: bytes) -> tuple[int, object]:
            stamp = str(int(time.time()))
            signature = hmac.new(WEBHOOK.encode(), stamp.encode() + b'.' + body, hashlib.sha256).hexdigest()
            return raw('/v1/webhooks/stripe', body, {'Stripe-Signature': 't=' + stamp + ',v1=' + signature})

        for state, expected in [('paid', 'paid'), ('lost', 'reversed'), ('won', 'paid'), ('refunded', 'refunded'), ('failed_refund', 'paid')]:
            provider.rows[identifier]['state'] = state
            event = dict(id='evt_' + state + '_' + identifier, object='event', api_version=VERSION, livemode=False,
                created=int(time.time()), type='checkout.session.completed',
                data={'object': {'id': 'cs_test_' + identifier, 'object': 'checkout.session'}})
            body = json.dumps(event).encode()
            expect('provider-stripe-' + state, deliver(body), 200)
            before = counts()
            expect('provider-stripe-' + state + '-redelivery', deliver(body), 200)
            if counts() != before or realm.sql("SELECT state FROM app.purchase WHERE id='" + purchase + "'") != expected:
                raise RuntimeError('Verified provider redelivery duplicated economic effects')
            active = int(realm.sql("SELECT count(*) FROM app.cosmetic_grant WHERE purchase_id='" + purchase + "' AND state='active'"))
            if active != (1 if expected == 'paid' else 0):
                raise RuntimeError('Current payment state did not control cosmetic access')
        grants = object_json(json.loads(realm.sql("SELECT jsonb_build_object('total',count(*),'active',count(*) FILTER(WHERE state='active'),'events',count(DISTINCT provider_event_id)) FROM app.cosmetic_grant WHERE purchase_id='" + purchase + "'")))
        if grants != {'total': 3, 'active': 1, 'events': 3}:
            raise RuntimeError('Reversal and restoration did not create independent verified grants')
        def payment_state(purchase_id: str, state: str, label: str) -> None:
            key = uuid.UUID(purchase_id).hex
            provider.rows[key]['state'] = state
            if label == 'first-source-reversed':
                # A new refund after the earlier failed refund needs distinct
                # source and returned-funds identities; do not rewrite history.
                provider.rows[key]['refund_ref'] = 're_projection_' + key
                provider.rows[key]['refund_failure_ref'] = 'txn_projection_' + key
            body = json.dumps(dict(id='evt_' + label.replace('-', '_') + '_' + key, object='event', api_version=VERSION, livemode=False,
                created=int(time.time()), type='checkout.session.completed',
                data={'object': {'id': 'cs_test_' + key, 'object': 'checkout.session'}})).encode()
            expect('provider-projection-stripe-' + label, deliver(body), 200)

        from benefit_projection_acceptance import qualify_benefit_projections
        qualify_benefit_projections(realm, request, expect, owner_token, peer_token, owner, peer,
            cosmetic, purchase, fixture, payment_state)
        if provider.errors:
            raise RuntimeError('Controlled Stripe rejected an application request')
        before = counts()
        expect('provider-stripe-invalid-signature', raw('/v1/webhooks/stripe', b'{}', {'Stripe-Signature': 't=0,v1=' + '0' * 64}), 400)
        if counts() != before:
            raise RuntimeError('Invalid Stripe signature created financial evidence')

        flow = {'requestId': str(uuid.uuid4()), 'stateKey': base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip('=')}
        start = post('patreon-start', '/v1/account/patreon/start', flow, status=202)
        data = object_json(start['data'])
        url = urllib.parse.urlsplit(data['authorizationUrl'])
        if data['accountId'] != owner or data['requestId'] != flow['requestId'] or url.scheme != 'https' or url.netloc != 'www.patreon.com':
            raise RuntimeError('Patreon start changed its caller or official authorization origin')
        if post('patreon-start-repeat', '/v1/account/patreon/start', flow, status=202) != start:
            raise RuntimeError('Patreon start retry changed the original issued flow')
        status = object_json(post('patreon-issued-status', '/v1/account/patreon/status', flow)['data'])
        if status['accountId'] != owner or status['requestId'] != flow['requestId'] or status['phase'] != 'issued':
            raise RuntimeError('Patreon status lost the original issued flow')
        foreign = object_json(post('patreon-foreign-flow-private', '/v1/account/patreon/status', flow, peer_token)['data'])
        if foreign['accountId'] != peer or foreign['phase'] != 'unknown' or foreign['associationId'] is not None:
            raise RuntimeError('Patreon status disclosed another account flow')
        unknown = dict(flow, requestId=str(uuid.uuid4()), code='synthetic_unissued_code')
        completed = object_json(post('patreon-unissued-completion', '/v1/account/patreon/complete', unknown)['data'])
        if completed['phase'] != 'unknown' or completed['associationId'] is not None:
            raise RuntimeError('An unissued OAuth code fabricated an association')
        # Do not complete the issued flow: its token exchange uses official
        # Patreon, outside this local API qualification and authorization scope.
        body = patreon['webhookBody'].encode()
        grants_before = realm.sql('SELECT count(*) FROM app.cosmetic_grant')
        retained_before = int(realm.sql("SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND raw_evidence_state='retained'"))
        for label in ['notification', 'redelivery-occurrence']:
            expect('provider-patreon-' + label, raw('/v1/webhooks/patreon', body, {'X-Patreon-Signature': patreon['signature']}), 200)
        if int(realm.sql("SELECT count(*) FROM app.provider_event WHERE provider='patreon' AND raw_evidence_state='retained'")) != retained_before + 2 or realm.sql('SELECT count(*) FROM app.cosmetic_grant') != grants_before:
            raise RuntimeError('Signed notification lost occurrence evidence or fabricated paid benefits')
        before = counts()
        expect('provider-patreon-original-bytes-required', raw('/v1/webhooks/patreon', body + b' ', {'X-Patreon-Signature': patreon['signature']}), 400)
        if counts() != before:
            raise RuntimeError('Invalid Patreon signature created retained evidence')
        return cosmetic
    finally:
        provider.shutdown()
        provider.server_close()
        thread.join(timeout=5)
        if thread.is_alive():
            raise RuntimeError('Owned provider fixture did not terminate')
