"""One connected local Stripe feature exercise using the existing owned realm.

Stripe is a controlled HTTP provider, not a live merchant. Supabase Auth, RPC,
Storage, application API and the Temporal recovery worker are actual processes.
"""
import hashlib
import hmac
import json
import os
from pathlib import Path
import shutil
import socket
import threading
import time
import urllib.parse
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from disposable_supabase import DisposableSupabase, ROOT, private_write
from qualification_http import make_opener, request_json, object_json

VERSION = '2026-10-28.endive'
KEY = 'sk_test_spellwake_synthetic_only'
WEBHOOK = 'whsec_spellwake_synthetic_only'


class Provider(ThreadingHTTPServer):
    """Finite synthetic purchase profiles; no cardholder data or external I/O."""

    def __init__(self) -> None:
        super().__init__(('127.0.0.1', 0), Handler)
        self.rows: dict[str, dict] = {}
        self.keys: dict[str, tuple[str, bytes]] = {}
        self.drop_next = False
        self.errors: list[str] = []
        self.base = f'http://127.0.0.1:{self.server_port}/'

    @staticmethod
    def price() -> dict:
        return dict(id='price_synthetic', object='price', active=True, livemode=False,
                    currency='usd', unit_amount=500, billing_scheme='per_unit', type='one_time',
                    recurring=None, custom_unit_amount=None, transform_quantity=None)

    def session(self, identifier: str) -> dict:
        row = self.rows[identifier]
        paid = row['state'] != 'pending'
        return dict(id='cs_test_' + identifier, object='checkout.session', status='complete' if paid else 'open',
                    payment_status='paid' if paid else 'unpaid', mode='payment', livemode=False,
                    payment_intent='pi_' + identifier if paid else None, client_reference_id=row['purchase'],
                    currency='usd', amount_total=500, url='https://checkout.stripe.com/c/pay/synthetic',
                    expires_at=row['expires'], metadata={'spellwake_purchase_id': row['purchase']})


class Handler(BaseHTTPRequestHandler):
    """Validate private headers, stable creation terms and provider lookups."""

    def log_message(self, *_: object) -> None:
        pass

    def do_GET(self) -> None:
        self.respond()

    def do_POST(self) -> None:
        self.respond()

    def respond(self) -> None:
        provider: Provider = self.server
        try:
            if self.headers.get('Authorization') != 'Bearer ' + KEY or self.headers.get('Stripe-Version') != VERSION or self.headers.get('apikey') or self.headers.get('Stripe-Account'):
                raise RuntimeError('Provider credential boundary differs')
            path, _, query = self.path.partition('?')
            args = urllib.parse.parse_qs(query)
            if path == '/v1/account':
                value = {'id': 'acct_synthetic', 'object': 'account'}
            elif path == '/v1/prices/price_synthetic':
                value = provider.price()
            elif path == '/v1/checkout/sessions' and self.command == 'POST':
                body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
                form = urllib.parse.parse_qs(body.decode())
                purchase = str(uuid.UUID(form['client_reference_id'][0]))
                identifier = uuid.UUID(purchase).hex
                key = self.headers.get('Idempotency-Key', '')
                if key != 'spellwake-purchase-' + purchase or form['line_items[0][price]'] != ['price_synthetic'] or form['line_items[0][quantity]'] != ['1'] or form['payment_intent_data[metadata][spellwake_purchase_id]'] != [purchase] or len(form) != 8:
                    raise RuntimeError('Provider creation terms differ')
                if key in provider.keys and provider.keys[key] != (identifier, body):
                    raise RuntimeError('Idempotency key parameters changed')
                provider.keys[key] = (identifier, body)
                provider.rows.setdefault(identifier, {'purchase': purchase, 'state': 'pending', 'expires': int(time.time()) + 3600})
                if provider.drop_next:
                    provider.drop_next = False
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.connection.close()
                    return
                value = provider.session(identifier)
            elif path == '/v1/checkout/sessions':
                value = {'object': 'list', 'has_more': False, 'data': [provider.session(identifier) for identifier in provider.rows]}
            elif path.startswith('/v1/checkout/sessions/cs_test_'):
                identifier = path.split('/')[4].removeprefix('cs_test_')
                value = ({'object': 'list', 'has_more': False, 'data': [{'id': 'li_synthetic', 'quantity': 1, 'price': provider.price()}]}
                         if path.endswith('/line_items') else provider.session(identifier))
            elif path.startswith('/v1/payment_intents/pi_'):
                identifier = path.rsplit('/', 1)[1].removeprefix('pi_')
                value = dict(id='pi_' + identifier, object='payment_intent', livemode=False, status='succeeded', currency='usd', amount=500, amount_received=500, latest_charge='ch_' + identifier, metadata={'spellwake_purchase_id': provider.rows[identifier]['purchase']})
            elif path.startswith('/v1/charges/ch_'):
                identifier = path.rsplit('/', 1)[1].removeprefix('ch_')
                value = dict(id='ch_' + identifier, object='charge', livemode=False, currency='usd', amount=500, amount_captured=500, amount_refunded=500 if provider.rows[identifier]['state'] == 'refunded' else 0, paid=True, captured=True, payment_intent='pi_' + identifier)
            elif path == '/v1/refunds':
                identifier = args['payment_intent'][0].removeprefix('pi_')
                state = provider.rows[identifier]['state']
                value = {'object': 'list', 'has_more': False, 'data': []}
                if state in ('refunded', 'failed_refund'):
                    value['data'] = [dict(id=provider.rows[identifier].get('refund_ref', 're_' + identifier), object='refund', payment_intent='pi_' + identifier, charge='ch_' + identifier, status='succeeded' if state == 'refunded' else 'failed', amount=500, currency='usd', failure_balance_transaction=provider.rows[identifier].get('refund_failure_ref', 'txn_return_' + identifier) if state == 'failed_refund' else None)]
            elif path.startswith('/v1/balance_transactions/'):
                reference = path.rsplit('/', 1)[1]
                matches = [key for key, row in provider.rows.items() if reference == row.get('refund_failure_ref', 'txn_return_' + key)]
                if len(matches) != 1:
                    raise RuntimeError('Unexpected fixture balance transaction')
                identifier = matches[0]
                value = dict(id=reference, object='balance_transaction', source=provider.rows[identifier].get('refund_ref', 're_' + identifier), type='refund_failure', amount=500, currency='usd')
            elif path == '/v1/disputes':
                identifier = args['payment_intent'][0].removeprefix('pi_')
                state = provider.rows[identifier]['state']
                data = []
                if state in ('lost', 'won'):
                    movements = [dict(id='txn_loss_' + identifier, amount=-500, currency='usd', created=1)]
                    if state == 'won':
                        movements.append(dict(id='txn_credit_' + identifier, amount=500, currency='usd', created=2))
                    data = [dict(id='du_' + identifier, object='dispute', livemode=False, payment_intent='pi_' + identifier, charge='ch_' + identifier, status=state, balance_transactions=movements)]
                value = {'object': 'list', 'has_more': False, 'data': data}
            else:
                raise RuntimeError('Unexpected provider path')
            data = json.dumps(value).encode()
            self.send_response(200)
        except (KeyError, ValueError, RuntimeError):
            provider.errors.append('Provider profile rejected an unexpected request')
            data = b'{}'
            self.send_response(400)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def port() -> int:
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def main() -> None:
    qualification = Path(os.environ['SPELLWAKE_CATALOGUE_QUALIFICATION_INPUT']).resolve(strict=True)
    realm = DisposableSupabase(storage=True, mail=True)
    provider = Provider()
    thread = threading.Thread(target=provider.serve_forever, daemon=True)
    thread.start()
    report: dict = {'passed': False, 'cleanupVerified': False, 'scope': 'controlled-stripe-real-local-api-auth-rpc-storage-temporal'}
    checks: list[str] = []
    # Keep completed checkpoint names on failures too; passed remains false.
    report['checks'] = checks
    repository = ROOT.parents[1]
    app = repository / 'apps/play'
    source_files = [*sorted((ROOT / 'src').rglob('*.rs')), *sorted((ROOT / 'supabase/migrations').glob('*.sql')),
                    ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'tools/qualify_stripe.py', ROOT / 'tools/disposable_supabase.py', ROOT / 'tools/qualification_http.py', ROOT / 'tools/loopback_docker.py']
    source_files += [*sorted((app / 'src').rglob('*.*')), *sorted((app / 'scripts').glob('*.ts')), app / 'vite.config.ts',
                    app / 'package.json', app / 'index.html', repository / 'package.json', repository / 'bun.lock',
                    repository / 'src-tauri/tauri.conf.json',
                    *sorted((repository / 'packages/application-contracts/src').rglob('*.ts')),
                    *sorted((repository / 'packages/ui/src').rglob('*.*')),
                    *sorted((repository / 'packages/deck-box/src').rglob('*.*')),
                    *sorted((repository / 'benchmarks/browser-rendering/src').rglob('*.*')),
                    *sorted((app / 'public').rglob('*.*'))]
    source_files += [repository / 'packages' / name / 'package.json' for name in ('application-contracts', 'ui', 'deck-box')]
    def source_hashes() -> dict[str, str]:
        return {str(path.relative_to(repository)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_files if path.is_file()}
    hashes = source_hashes()
    report["sourceHashes"] = hashes
    primary = None
    try:
        realm.command(['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--bin', 'application_api', '--bin', 'stripe_worker', '--bin', 'stripe_workflow', '--bin', 'catalogue_publish', '--bin', 'deletion_worker', '--bin', 'deletion_workflow'], timeout=300)
        realm.__enter__()
        base = realm.status['API_URL']
        public = realm.status['PUBLISHABLE_KEY']
        secret = realm.status['SECRET_KEY']
        opener = make_opener()

        def request(url: str, body: object = None, *, method: str | None = None, bearer: str | None = None, admin: bool = False) -> tuple[int, object]:
            headers = {'Content-Type': 'application/json'}
            if url.startswith(base + '/'):
                headers.update(apikey=secret if admin else public)
                headers.update({'Accept-Profile': 'app', 'Content-Profile': 'app'})
            if bearer:
                headers['Authorization'] = 'Bearer ' + bearer
            return request_json(opener, urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method), timeout=45)

        status, generated = request(base + '/auth/v1/admin/generate_link', {'type': 'magiclink', 'email': 'stripe@example.invalid'}, admin=True)
        props = object_json(object_json(generated).get('properties', generated))
        if status != 200:
            raise RuntimeError('Actual email-code generation failed')
        status, signed = request(base + '/auth/v1/verify', {'email': 'stripe@example.invalid', 'token': props['email_otp'], 'type': props['verification_type']})
        if status != 200:
            raise RuntimeError('Actual email-code redemption failed')
        bearer = object_json(signed)['access_token']
        free = [str(uuid.uuid4()) for _ in range(3)]
        cosmetic = str(uuid.uuid4())
        for identifier, kind in zip(free, ('back', 'sleeve', 'dice'), strict=True):
            realm.sql(f"INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES('{identifier}','{kind}','test-only:stripe','active',true)")
        realm.sql(f"INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free,display_name) VALUES('{cosmetic}','back','test-only:stripe-paid','active',false,'Owned qualification back')")
        status, _ = request(base + '/storage/v1/bucket', {'id': 'payment-evidence', 'name': 'payment-evidence', 'public': False}, admin=True)
        if status != 200:
            raise RuntimeError('Actual private evidence bucket creation failed')
        api_port = port()
        api = f'http://127.0.0.1:{api_port}'
        frontend_port = port()
        frontend = f'http://127.0.0.1:{frontend_port}'
        environment = dict(SPELLWAKE_SUPABASE_URL=base, SPELLWAKE_SUPABASE_PUBLISHABLE_KEY=public,
            SPELLWAKE_SUPABASE_SECRET_KEY=secret, SPELLWAKE_AUTH_ISSUER=base + '/auth/v1',
            SPELLWAKE_FREE_BACK_ID=free[0], SPELLWAKE_FREE_SLEEVE_ID=free[1], SPELLWAKE_FREE_DICE_ID=free[2],
            SPELLWAKE_API_BIND=f'127.0.0.1:{api_port}', SPELLWAKE_API_ORIGINS=frontend, SPELLWAKE_STRIPE_SECRET_KEY=KEY,
            SPELLWAKE_STRIPE_WEBHOOK_SECRET=WEBHOOK, SPELLWAKE_STRIPE_ACCOUNT_ID='acct_synthetic',
            SPELLWAKE_STRIPE_PRICES=json.dumps({cosmetic: 'price_synthetic'}), SPELLWAKE_STRIPE_SUCCESS_URL='https://app.example/return',
            SPELLWAKE_STRIPE_CANCEL_URL='https://app.example/cancel', SPELLWAKE_PAYMENT_EVIDENCE_BUCKET='payment-evidence',
            SPELLWAKE_STRIPE_ALLOW_LOCAL_FIXTURE='1', SPELLWAKE_STRIPE_FIXTURE_BASE_URL=provider.base)
        realm.start_background([str(ROOT / 'target/debug/application_api')], environment)

        def wait_until(predicate, message: str, seconds: int = 30) -> None:
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                try:
                    if predicate():
                        return
                except OSError:
                    pass
                time.sleep(0.1)
            raise RuntimeError(message)

        wait_until(lambda: request(api + '/health')[0] == 200, 'Actual application readiness failed')
        status, registered = request(api + '/v1/account', {'username': 'stripe_qualified'}, bearer=bearer)
        if status != 200 or object_json(registered).get('status') != 'success':
            raise RuntimeError('Actual application registration failed')
        account = str(uuid.UUID(object_json(object_json(registered)['data'])['accountId']))

        def reserve(drop: bool = False) -> tuple[str, dict]:
            provider.drop_next = drop
            body = {'requestId': str(uuid.uuid4()), 'cosmeticId': cosmetic}
            status, value = request(api + '/v1/purchases', body, bearer=bearer)
            if status != 202 or object_json(value).get('status') != 'success':
                raise RuntimeError('Actual purchase reservation failed')
            purchase = str(uuid.UUID(object_json(object_json(value)['data'])['resourceId']))
            if request(api + '/v1/purchases', body, bearer=bearer) != (status, value):
                raise RuntimeError('Actual reservation receipt replay differs')
            return purchase, body

        def state(purchase: str) -> str:
            return realm.sql(f"SELECT state FROM app.purchase WHERE id='{str(uuid.UUID(purchase))}'")

        def webhook(purchase: str, label: str) -> bytes:
            identifier = uuid.UUID(purchase).hex
            event = dict(id='evt_' + label, object='event', api_version=VERSION, livemode=False,
                created=int(time.time()), type='checkout.session.completed', data={'object': {'id': 'cs_test_' + identifier, 'object': 'checkout.session'}})
            return json.dumps(event).encode()

        def deliver(body: bytes, offset: int = 0) -> int:
            stamp = str(int(time.time()) + offset)
            signature = hmac.new(WEBHOOK.encode(), stamp.encode() + b'.' + body, hashlib.sha256).hexdigest()
            status, _ = request_json(opener, urllib.request.Request(api + '/v1/webhooks/stripe', data=body,
                headers={'Content-Type': 'application/json', 'Stripe-Signature': f't={stamp},v1={signature}'}), timeout=45)
            return status

        purchase, _ = reserve()
        identifier = uuid.UUID(purchase).hex
        if state(purchase) != 'pending':
            raise RuntimeError('Known checkout acknowledgment was not saved')
        checks.append('checkout-and-receipt-replay')
        for provider_state, expected in [('paid', 'paid'), ('lost', 'reversed'), ('won', 'paid'), ('refunded', 'refunded'), ('failed_refund', 'paid')]:
            provider.rows[identifier]['state'] = provider_state
            body = webhook(purchase, provider_state)
            if deliver(body) != 200 or state(purchase) != expected:
                raise RuntimeError('Connected payment transition differs: ' + provider_state)
            if deliver(body, 1) != 200 or state(purchase) != expected:
                raise RuntimeError('Fresh signature redelivery changed the economic result')
            checks.append('verified-' + provider_state)
        grants = json.loads(realm.sql(f"SELECT jsonb_build_object('total',count(*),'active',count(*) FILTER(WHERE state='active'),'events',count(DISTINCT provider_event_id)) FROM app.cosmetic_grant WHERE purchase_id='{purchase}'"))
        if grants != {'total': 3, 'active': 1, 'events': 3}:
            raise RuntimeError('Immutable restoration grants differ')
        status, view = request(api + '/v1/purchases/' + purchase, bearer=bearer)
        if status != 200 or object_json(object_json(view)['data']).get('state') != 'paid':
            raise RuntimeError('Actual safe owner purchase view failed')
        if request(base + '/rest/v1/purchase?select=checkout_terms', bearer=bearer)[0] not in (401, 403):
            raise RuntimeError('Owner read exposed private checkout terms')
        checks.append('owner-projection-and-private-column-denial')
        evidence = json.loads(realm.sql("SELECT jsonb_agg(jsonb_build_object('key',payload_ref,'hash',capture->>'bodyHash','state',raw_evidence_state)) FROM app.provider_event WHERE capture->>'kind'='webhook'"))
        if len(evidence) != 5 or any(row['state'] != 'retained' for row in evidence):
            raise RuntimeError('Actual webhook capture/evidence count differs')
        checks.append('private-storage-capture-before-upload-and-redelivery')

        expired_body = webhook(purchase, 'paid')
        # Keep the original authenticated bytes, including their original
        # created timestamp. This source is aged only in the owned fixture.
        expired_source = json.loads(realm.sql("SELECT jsonb_build_object('id',id,'key',payload_ref) FROM app.provider_event WHERE external_event_ref='evt_paid'"))
        raw_url = base + '/storage/v1/object/authenticated/payment-evidence/' + expired_source['key']
        with opener.open(urllib.request.Request(raw_url, headers={'apikey': secret}), timeout=10) as response:
            expired_body = response.read(512 * 1024 + 1)
        if hashlib.sha256(expired_body).hexdigest() != next(row['hash'] for row in evidence if row['key'] == expired_source['key']):
            raise RuntimeError('Actual retained webhook bytes differ')
        if request(base + '/storage/v1/object/payment-evidence', {'prefixes': [expired_source['key']]}, admin=True, method='DELETE')[0] != 200:
            raise RuntimeError('Owned original evidence deletion failed')
        expired_id = str(uuid.UUID(expired_source['id']))
        realm.sql(f"BEGIN; SET LOCAL session_replication_role=replica; UPDATE app.provider_event SET received_at=clock_timestamp()-interval '31 days',raw_evidence_state='expired',raw_deleted_at=clock_timestamp() WHERE id='{expired_id}'; COMMIT")
        # Simulate a previously admitted uploader finishing after deletion and
        # crashing before its confirm RPC. Publication uses the actual API.
        status, _ = request_json(opener, urllib.request.Request(base + '/storage/v1/object/payment-evidence/' + expired_source['key'], data=expired_body,
            headers={'apikey': secret, 'Content-Type': 'application/json', 'x-upsert': 'false'}))
        if status != 200:
            raise RuntimeError('Owned late-upload race fixture did not publish')

        unknown, _ = reserve(drop=True)
        if state(unknown) != 'creating' or realm.sql(f"SELECT stripe_checkout_ref IS NULL FROM app.purchase WHERE id='{unknown}'") != 't':
            raise RuntimeError('Dropped provider acknowledgment was guessed locally')
        provider.rows[uuid.UUID(unknown).hex]['state'] = 'paid'
        temporal = shutil.which('temporal')
        if temporal is None:
            raise RuntimeError('Temporal CLI unavailable')
        temporal_port = port()
        namespace = 'stripe-test-' + uuid.uuid4().hex
        private_write(realm.stage / 'empty.yaml', b'{}')
        isolated = ['--disable-config-env', '--disable-config-file', '--env-file', str(realm.stage / 'empty.yaml')]
        realm.start_background([temporal, 'server', 'start-dev', *isolated, '--ip', '127.0.0.1', '--port', str(temporal_port), '--namespace', namespace, '--headless', '--db-filename', str(realm.stage / 'temporal.db')], {})

        def temporal_ready() -> bool:
            try:
                realm.command([temporal, 'operator', 'cluster', 'health', *isolated, '--address', f'127.0.0.1:{temporal_port}', '--command-timeout', '2s'], timeout=4)
                return True
            except RuntimeError:
                return False

        wait_until(temporal_ready, 'Owned Temporal readiness failed')
        worker_env = environment | {'SPELLWAKE_TEMPORAL_ADDRESS': f'http://127.0.0.1:{temporal_port}', 'SPELLWAKE_TEMPORAL_NAMESPACE': namespace}
        realm.command([str(ROOT / 'target/debug/stripe_workflow'), 'install'], extra_environment=worker_env)
        realm.start_background([str(ROOT / 'target/debug/stripe_worker')], worker_env)
        wait_until(lambda: state(unknown) == 'paid', 'Real recovery worker did not resolve unknown checkout', seconds=100)
        wait_until(lambda: realm.sql(f"SELECT count(*) FROM storage.objects WHERE bucket_id='payment-evidence' AND name='{expired_source['key']}'") == '0', 'Real expiry worker did not remove recreated evidence', seconds=100)
        if deliver(expired_body, 1) != 200 or realm.sql(f"SELECT count(*) FROM storage.objects WHERE bucket_id='payment-evidence' AND name='{expired_source['key']}'") != '0':
            raise RuntimeError('Expired redelivery recreated raw evidence')
        checks.append('real-storage-late-upload-expiry-recovery-and-no-redelivery-extension')
        if len(provider.keys) != 2 or len(provider.rows) != 2:
            raise RuntimeError('Recovery created an additional checkout')
        checks.append('real-temporal-positive-unknown-checkout-recovery')

        # The same owned realm now exercises the complete source producer and
        # its actual owner-facing deck consumer, rather than another fixture per helper.
        status, _ = request(base + '/storage/v1/bucket', {'id': 'catalogue-evidence', 'name': 'catalogue-evidence',
            'public': False, 'file_size_limit': 2147483648,
            'allowed_mime_types': ['application/json', 'application/gzip', 'application/x-ndjson']}, admin=True)
        if status != 200:
            raise RuntimeError('Owned catalogue evidence bucket creation failed')
        catalogue_environment = dict(SPELLWAKE_SUPABASE_URL=base, SPELLWAKE_SUPABASE_SECRET_KEY=secret,
            SPELLWAKE_CATALOGUE_BUCKET='catalogue-evidence')
        catalogue_process = realm.start_background([str(ROOT / 'target/debug/catalogue_publish'), str(qualification), str(realm.stage)], catalogue_environment)
        catalogue_log = realm.stage / f'private-background-{realm.sequence}.log'
        wait_until(lambda: catalogue_process.poll() is not None, 'Owned catalogue publisher did not finish', seconds=1800)
        if catalogue_process.returncode != 0:
            raise RuntimeError('Owned catalogue publication failed; private diagnostics retained')
        catalogue_result = object_json(json.loads(catalogue_log.read_text().splitlines()[-1]))
        if catalogue_result.get('status') != 'success':
            raise RuntimeError('Owned catalogue publisher returned no accepted report')
        catalogue = object_json(catalogue_result['data'])
        publication = str(uuid.UUID(catalogue['publicationId']))
        counts = json.loads(realm.sql(f"SELECT jsonb_build_object('definitions',(SELECT count(*) FROM app.card_definition_revision WHERE catalogue_receipt_id='{publication}'),'sets',(SELECT count(*) FROM app.card_set WHERE catalogue_receipt_id='{publication}'),'printings',(SELECT count(*) FROM app.printing WHERE catalogue_receipt_id='{publication}'),'programs',(SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision r ON r.id=p.definition_revision_id WHERE r.catalogue_receipt_id='{publication}'))"))
        if counts['printings'] != catalogue['printingCount'] or counts['definitions'] < 1 or counts['programs'] != 0:
            raise RuntimeError('Published catalogue coverage or qualification boundary differs')
        catalogue_card = json.loads(realm.sql("SELECT jsonb_build_object('definitionId',d.id,'name',d.name,'printingId',p.id,'collectorNumber',p.collector_number,'finish',p.finish,'language',p.language,'setName',s.name,'setCode',s.code) FROM app.card_definition d JOIN app.printing p ON p.definition_id=d.id JOIN app.card_set s ON s.id=p.set_id WHERE d.name='Fury Sliver' ORDER BY p.id LIMIT 1"))
        if not catalogue_card:
            raise RuntimeError('Retained source has no actual deck-consumer card')
        checks.extend(['real-retained-source-private-storage-and-atomic-catalogue-publication', 'real-catalogue-coverage-without-executable-program-issuance'])

        # Freeze one actual browser build inside this owned realm, not shared dist outputs.
        browser_environment = dict(VITE_SUPABASE_URL=base, VITE_SUPABASE_PUBLISHABLE_KEY=public, VITE_APPLICATION_API_URL=api)
        dist = realm.stage / 'client-dist'
        realm.command(['bun', 'run', '--cwd', str(app), 'vite', 'build', '--mode', 'web', '--outDir', str(dist)], timeout=300, extra_environment=browser_environment)
        realm.start_background(['bun', 'run', '--cwd', str(app), 'vite', 'preview', '--mode', 'web', '--outDir', str(dist), '--port', str(frontend_port), '--strictPort'], browser_environment)
        def frontend_ready() -> bool:
            with opener.open(frontend + '/', timeout=2) as response:
                return response.status == 200
        wait_until(frontend_ready, 'Owned static browser build did not become ready')
        # Preview serves real static assets; the browser supplies the native strict policy.
        csp = json.loads((repository / 'src-tauri/tauri.conf.json').read_text())['app']['security']['csp']
        csp = csp.replace('connect-src ', f'connect-src {base} {api} ')
        browser_config = realm.stage / 'browser-config.json'
        private_write(browser_config, json.dumps(dict(app=frontend, api=api, auth=base, mail=f'http://127.0.0.1:{realm.mail_port}',
            email='browser@example.invalid', cosmetic=cosmetic, stage=str(realm.stage), csp=csp, catalogueCard=catalogue_card)).encode())
        browser_process = realm.start_background(['bun', str(app / 'scripts/qualify-stripe-browser.ts'), str(browser_config)], {})
        def browser_phase(name: str) -> bool:
            if browser_process.poll() is not None and browser_process.returncode != 0:
                raise RuntimeError('Actual browser consumer failed; diagnostics retained privately')
            return (realm.stage / f'browser-{name}.json').is_file()
        wait_until(lambda: browser_phase('registered'), 'Actual SMTP/browser registration did not complete', seconds=100)
        browser_account = str(uuid.UUID(realm.sql("SELECT id FROM app.account WHERE username='browser_qualified' AND auth_subject=(SELECT id FROM auth.users WHERE email='browser@example.invalid')")))
        # Actual social commands share this owned Auth/API/SQL environment with
        # account, commerce and deletion. No direct social-state inserts.
        wait_until(lambda: browser_phase('social-requested'), 'Actual browser did not request friendship', seconds=100)
        accepted = {'kind': 'friendship', 'command': {'requestId': str(uuid.uuid4()), 'otherAccountId': browser_account, 'action': 'accept'}}
        status, acceptance = request(api + '/v1/social/commands', accepted, bearer=bearer)
        if status != 202 or object_json(acceptance).get('status') != 'success':
            raise RuntimeError('Actual peer friendship acceptance failed')
        private_write(realm.stage / 'browser-social-accepted.json', b'{}')
        wait_until(lambda: browser_phase('social-delivered'), 'Actual browser did not recover its committed message', seconds=150)
        social_delivery = object_json(json.loads((realm.stage / 'browser-social-delivered.json').read_text()))
        social_command = object_json(social_delivery['command'])
        social_input = object_json(social_command['command'])
        conversation = str(uuid.UUID(social_input['conversationId']))
        send_request = str(uuid.UUID(social_input['requestId']))
        if social_delivery['sendPosts'] != 1 or social_command['kind'] != 'send_dm' or social_input['body'] != 'Owned social delivery':
            raise RuntimeError('Actual browser changed its social delivery terms')
        delivered_once = realm.sql(f"SELECT count(*) FROM app.message WHERE conversation_id='{conversation}' AND sender_id='{browser_account}' AND body='Owned social delivery'")
        send_receipts = realm.sql(f"SELECT count(*) FROM app.command_receipt WHERE account_id='{browser_account}' AND scope='chat' AND request_ref='{send_request}' AND state='accepted'")
        if delivered_once != '1' or send_receipts != '1':
            raise RuntimeError('Actual message/receipt does not prove one committed send')
        incoming = {'kind': 'send_dm', 'command': {'requestId': str(uuid.uuid4()), 'conversationId': conversation, 'body': 'Owned peer reply'}}
        status, incoming_ack = request(api + '/v1/social/commands', incoming, bearer=bearer)
        if status != 202 or object_json(incoming_ack).get('status') != 'success':
            raise RuntimeError('Actual peer reply failed')
        incoming_id = str(uuid.UUID(object_json(object_json(incoming_ack)['data'])['resourceId']))
        private_write(realm.stage / 'browser-social-incoming.json', b'{}')
        wait_until(lambda: browser_phase('social-result'), 'Actual browser did not retain social history/draft', seconds=100)
        wait_until(lambda: realm.sql(f"SELECT count(*) FROM app.notification WHERE recipient_id='{browser_account}' AND source_kind='dm_message' AND source_ref='{incoming_id}' AND read_at IS NOT NULL") == '1', 'Actual visible reply was not acknowledged')
        status, social_status = request(api + '/v1/social/status?personId=' + browser_account, bearer=bearer)
        peer_status = object_json(object_json(social_status).get('data', {}))
        if status != 200 or peer_status.get('accountId') != account or peer_status.get('personId') != browser_account or peer_status.get('unreadMessages') != '1':
            raise RuntimeError('Actual peer social status failed')
        report['social'] = {'oneSendPost': True, 'oneMessage': True, 'oneReceipt': True, 'visibleReplyRead': True, 'draftRetained': True}
        checks.extend(['real-browser-friendship-discovery-and-canonical-peer-acceptance', 'real-browser-social-lost-ack-reload-explicit-recovery-with-one-message', 'real-browser-visible-reply-exact-read-and-retained-social-draft'])
        format_id = str(uuid.uuid4())
        realm.sql(f"INSERT INTO app.format(id,key,name) VALUES('{format_id}','standard','Standard')")
        private_write(realm.stage / 'browser-library-ready.json', b'{}')
        wait_until(lambda: browser_phase('deck-saved'), 'Actual browser did not create/save its source-backed deck', seconds=180)
        deck_result = object_json(json.loads((realm.stage / 'browser-deck-saved.json').read_text()))
        deck_id = str(uuid.UUID(deck_result['deckId']))
        saved = json.loads(realm.sql(f"SELECT jsonb_build_object('owner',d.account_id,'entries',jsonb_agg(jsonb_build_object('definitionId',e.definition_id,'printingId',e.printing_id,'quantity',e.quantity::text,'section',e.section))) FROM app.deck d JOIN app.deck_entry e ON e.revision_id=d.current_revision_id WHERE d.id='{deck_id}' GROUP BY d.id"))
        if saved != {'owner': browser_account, 'entries': [{'definitionId': catalogue_card['definitionId'], 'printingId': catalogue_card['printingId'], 'quantity': '3', 'section': 'main'}]}:
            raise RuntimeError('Actual owned browser deck differs from its canonical saved content')
        checks.extend(['real-browser-canonical-create-source-printing-selection-and-save', 'real-browser-source-import-revision-recovery'])
        wait_until(lambda: browser_phase('reject-ready'), 'Actual browser did not select its checkout', seconds=100)
        realm.sql(f"UPDATE app.cosmetic SET state='inactive' WHERE id='{cosmetic}'")
        private_write(realm.stage / 'browser-reject-start.json', b'{}')
        wait_until(lambda: browser_phase('rejected'), 'Actual browser did not observe canonical rejection', seconds=100)
        rejected_request = str(uuid.UUID(json.loads((realm.stage / 'browser-rejected.json').read_text())['requestId']))
        if realm.sql(f"SELECT count(*) FROM app.command_receipt WHERE principal_key='human:{browser_account}' AND scope='stripe_checkout' AND request_ref='{rejected_request}' AND state='rejected'") != '1':
            raise RuntimeError('Rejected browser request has no immutable failed receipt')
        realm.sql(f"UPDATE app.cosmetic SET state='active' WHERE id='{cosmetic}'")
        private_write(realm.stage / 'browser-reactivated.json', b'{}')
        wait_until(lambda: browser_phase('purchase-ready'), 'Actual browser checkout recovery did not complete', seconds=100)
        browser_purchase = str(uuid.UUID(json.loads((realm.stage / 'browser-purchase-ready.json').read_text())['purchaseId']))
        if realm.sql(f"SELECT account_id FROM app.purchase WHERE id='{browser_purchase}'") != browser_account:
            raise RuntimeError('Browser purchase is not owned by the newly registered account')
        browser_identifier = uuid.UUID(browser_purchase).hex
        for provider_state, phase, expected in [('paid', 'paid', 'paid'), ('lost', 'reversed', 'reversed'), ('won', 'result', 'paid')]:
            provider.rows[browser_identifier]['state'] = provider_state
            if deliver(webhook(browser_purchase, 'browser_' + provider_state)) != 200 or state(browser_purchase) != expected:
                raise RuntimeError('Actual browser source transition differs')
            if provider_state == 'lost':
                private_write(realm.stage / 'browser-reverse.json', b'{}')
            elif provider_state == 'won':
                private_write(realm.stage / 'browser-restore.json', b'{}')
            wait_until(lambda: browser_phase(phase), 'Actual browser did not observe its verified purchase state', seconds=100)
        restored_browser_grants = json.loads(realm.sql(f"SELECT jsonb_build_object('total',count(*),'active',count(*) FILTER(WHERE state='active')) FROM app.cosmetic_grant WHERE purchase_id='{browser_purchase}'"))
        if restored_browser_grants != {'total': 2, 'active': 1}:
            raise RuntimeError('Actual purchase restoration did not create a new valid grant before deletion')
        wait_until(lambda: browser_phase('deletion-code-needed'), 'Browser did not reach deletion reauthentication', seconds=100)
        status, generated = request(base + '/auth/v1/admin/generate_link', {'type': 'magiclink', 'email': 'browser@example.invalid'}, admin=True)
        reauth = object_json(object_json(generated).get('properties', generated))
        if status != 200 or not isinstance(reauth.get('email_otp'), str):
            raise RuntimeError('Owned deletion reauthentication code unavailable')
        private_write(realm.stage / 'browser-deletion-code.json', json.dumps({'code': reauth['email_otp']}).encode())
        wait_until(lambda: browser_phase('deletion-ready'), 'Actual browser deletion request/reload did not recover', seconds=100)
        deletion_source = object_json(json.loads((realm.stage / 'browser-deletion-ready.json').read_text()))
        deletion_command = object_json(deletion_source['request'])
        deletion_request = str(uuid.UUID(deletion_command['requestId']))
        subject = str(uuid.UUID(realm.sql(f"SELECT auth_subject FROM app.account WHERE id='{browser_account}'")))
        before_retirement = json.loads(realm.sql(f"SELECT jsonb_build_object('state',state,'wallet',(SELECT sum(owned_quantity)::text FROM app.inventory_asset i JOIN app.asset_owner owner ON owner.id=i.owner_id WHERE owner.account_id='{browser_account}'),'requests',(SELECT count(*) FROM app.command_receipt WHERE account_id='{browser_account}' AND scope='account_delete' AND request_ref='{deletion_request}' AND state='accepted')) FROM app.account WHERE id='{browser_account}'"))
        if before_retirement != {'state': 'deletion_pending', 'wallet': '14500', 'requests': 1}:
            raise RuntimeError('Actual deletion admission/retained nonzero assets differs')
        # Both dedicated processes use the same owned Temporal service, distinct
        # queues/workflows. No fake finalizer or direct auth.users DELETE.
        realm.command([str(ROOT / 'target/debug/deletion_workflow'), 'install'], extra_environment=worker_env)
        deletion_worker = realm.start_background([str(ROOT / 'target/debug/deletion_worker')], worker_env)
        def deletion_finished() -> bool:
            if deletion_worker.poll() is not None:
                raise RuntimeError('Owned deletion worker exited before cleanup')
            return realm.sql(f"SELECT app.account_auth_cleanup_receipt('{browser_account}') IS NOT NULL") == 't'
        wait_until(deletion_finished, 'Actual deletion worker did not retire and canonically remove Auth target', seconds=150)
        deletion_proof = json.loads(realm.sql(f"SELECT jsonb_build_object('state',a.state,'authCleared',a.auth_subject IS NULL AND a.auth_cleanup_subject IS NULL,'users',(SELECT count(*) FROM auth.users WHERE id='{subject}'),'sessions',(SELECT count(*) FROM auth.sessions WHERE user_id='{subject}'),'inventory',(SELECT coalesce(sum(owned_quantity),0)::text FROM app.inventory_asset i JOIN app.asset_owner owner ON owner.id=i.owner_id WHERE owner.account_id=a.id),'profiles',(SELECT count(*) FROM app.profile WHERE account_id=a.id),'retirements',(SELECT count(*) FROM app.ledger_operation WHERE actor_account_id=a.id AND kind='retirement' AND state='committed'),'finalization',(SELECT count(*) FROM app.command_receipt WHERE account_id=a.id AND scope='account_retirement_v1' AND state='accepted'),'cleanupProof',app.account_auth_cleanup_receipt(a.id) IS NOT NULL,'deckName',(SELECT name FROM app.deck WHERE id='{deck_id}'),'liveGrants',(SELECT count(*) FROM app.cosmetic_grant WHERE account_id=a.id AND state='active')) FROM app.account a WHERE a.id='{browser_account}'"))
        expected_deletion = {'state':'deleted','authCleared':True,'users':0,'sessions':0,'inventory':'0','profiles':0,'retirements':1,'finalization':1,'cleanupProof':True,'deckName':'Deleted deck','liveGrants':0}
        if deletion_proof != expected_deletion:
            raise RuntimeError('Actual retirement, canonical cleanup or minimization differs')
        report['deletion'] = deletion_proof
        status, retained_history = request(api + f'/v1/social/conversations/{conversation}/messages', bearer=bearer)
        retained = object_json(object_json(retained_history).get('data', {}))
        retained_person = object_json(retained.get('person', {}))
        if status != 200 or retained_person.get('username') != 'Deleted player' or retained_person.get('isDeleted') is not True or retained_person.get('canContact') is not False:
            raise RuntimeError('Surviving peer history did not anonymize the retired player')
        retired_messages = [object_json(item) for item in retained.get('items', []) if object_json(item).get('senderId') == browser_account]
        if len(retired_messages) != 1 or retired_messages[0].get('body') != 'Owned social delivery' or retired_messages[0].get('author') != 'Deleted player':
            raise RuntimeError('Retired message body/anonymous author was not preserved')
        checks.append('real-peer-retained-history-anonymous-after-canonical-retirement')
        private_write(realm.stage / 'browser-deletion-worker-settled.json', b'{}')
        wait_until(lambda: browser_phase('deletion-result'), 'Actual browser did not verify terminal deletion/local erasure', seconds=100)
        realm.command([str(ROOT / 'target/debug/deletion_workflow'), 'install'], extra_environment=worker_env)
        history_bytes = realm.command([temporal, 'workflow', 'show', '--workflow-id', 'spellwake-deletion-cleanup-v1', *isolated, '--address', f'127.0.0.1:{temporal_port}', '--namespace', namespace, '--output', 'json'])
        if secret.encode() in history_bytes or subject.encode() in history_bytes:
            raise RuntimeError('Raw deletion credential/target entered workflow history')
        private_write(realm.stage / 'deletion-history.json', history_bytes)
        checks.extend(['real-browser-deletion-request-lost-ack-reload-without-resubmit','real-temporal-worker-nonzero-retirement-and-canonical-admin-auth-cleanup','real-browser-terminal-observer-payload-sdk-erasure','deletion-history-no-secret-or-raw-auth-target'])
        if browser_process.wait(timeout=20) != 0:
            raise RuntimeError('Actual browser process did not complete cleanly')
        browser_grants = json.loads(realm.sql(f"SELECT jsonb_build_object('total',count(*),'active',count(*) FILTER(WHERE state='active')) FROM app.cosmetic_grant WHERE purchase_id='{browser_purchase}'"))
        if browser_grants != {'total': 2, 'active': 0} or len(provider.keys) != 3 or len(provider.rows) != 3:
            raise RuntimeError('Browser recovery/restoration created unexpected purchases or grants')
        checks.extend(['real-smtp-sdk-signup-and-canonical-account', 'real-browser-wallet-and-owned-library', 'real-browser-canonical-rejection-reactivation-new-request', 'real-browser-dropped-ack-reload-same-request', 'real-browser-paid-reversal-new-grant-restoration-and-logout'])
        if provider.errors:
            raise RuntimeError('Controlled provider rejected requests')
        end_hashes = source_hashes()
        if hashes != end_hashes:
            report['sourceChanges'] = [path for path in sorted(set(hashes) | set(end_hashes)) if hashes.get(path) != end_hashes.get(path)]
            raise RuntimeError('Connected qualification source changed during execution')
        report.update(passed=True, checks=checks, grants=grants, sourceHashes=hashes, migrations=realm.manifest,
                      runtime=realm.runtime, deletion=deletion_proof, restoredBrowserGrants=restored_browser_grants, browserGrants=browser_grants, catalogue=catalogue, catalogueCounts=counts, excluded=['live-stripe', 'real-card-payment', 'hosted-configuration', 'native-os-consumer', 'external-mail-delivery', 'production-email-design', 'live-social-providers', 'all-provider-payment-methods'])
    except BaseException as error:
        primary = error
        report['failureType'] = type(error).__name__
    finally:
        provider.shutdown()
        provider.server_close()
        thread.join(timeout=5)
        try:
            realm.__exit__()
            report['cleanupVerified'] = realm.cleanup_verified
        except BaseException as cleanup:
            report['cleanupFailureType'] = type(cleanup).__name__
            if primary is None:
                primary = cleanup
        private_write(realm.stage / 'result.json', json.dumps(report, indent=2).encode())
    print(json.dumps({'passed': report['passed'], 'cleanupVerified': report['cleanupVerified'], 'checks': len(checks), 'result': str(realm.stage / 'result.json')}))
    if primary is not None:
        raise RuntimeError('Connected Stripe qualification failed; private diagnostics retained') from primary


if __name__ == '__main__':
    main()
