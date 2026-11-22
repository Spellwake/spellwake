"""Observe pinned GoTrue audit-to-hook transactions in a disposable realm.

Public/admin-origin email codes, refresh and synthetic-Discord OAuth are exercised.
No external mailbox delivery or actual provider identity verification is proved.
The exact application session helper is installed only in the disposable realm.
No production hook or existing application/Auth realm is modified.
"""
import base64
import argparse
import hashlib
import hmac
import json
import os
from pathlib import Path
import secrets
import subprocess
import time
import urllib.request
import uuid
from contextlib import ExitStack

from disposable_postgres import DisposablePostgres, DisposableAuthPostgres
from auth_oauth_fixture import DiscordFixture, qualify_oauth
from auth_concurrency_fixture import qualify_concurrency
from auth_admission_fixture import qualify_admission
from disposable_http_service import DisposableHttpService
from auth_postgrest_fixture import qualify_postgrest
from qualification_http import make_opener, request_json, object_json

ROOT = Path(__file__).resolve().parents[1]
IMAGE = 'public.ecr.aws/supabase/gotrue:v2.197.0'


def main() -> None:
    """Own one database/login/container; retain only normalized observations."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--refresh-algorithm', type=int, choices=(0, 2), default=0)
    parser.add_argument('--application-auth', action='store_true',
                        help='Run exact application migrations/hook on a fresh canonical-role database container')
    parser.add_argument('--postgrest', action='store_true', help='Test actual HTTP bearer middleware in the application realm')
    parser.add_argument('--identity-concurrency', action='store_true', help='Run actual same-subject numeric/Discord concurrency after the baseline')
    parser.add_argument('--qualification-fault', choices=('after-concurrency-start',), help='Fail an owned test run to qualify cleanup evidence')
    arguments = parser.parse_args()
    if arguments.qualification_fault and not arguments.identity_concurrency:
        parser.error('--qualification-fault requires --identity-concurrency')
    if arguments.identity_concurrency and not arguments.application_auth:
        parser.error('--identity-concurrency requires --application-auth')
    if arguments.postgrest and not arguments.application_auth:
        parser.error('--postgrest requires --application-auth')
    realm = DisposableAuthPostgres() if arguments.application_auth else DisposablePostgres('auth_producer', ('gotrue_probe',))
    role = realm.roles['gotrue_probe']
    stage = ROOT / 'target' / ('auth-producer-' + uuid.uuid4().hex)
    stage.mkdir(mode=0o700)
    secret = secrets.token_hex(32)
    database_password = secrets.token_hex(32)
    image_id = subprocess.check_output(
        ['docker', 'image', 'inspect', IMAGE, '--format', '{{.Id}}'], text=True,
    ).strip()
    image_environment = json.loads(subprocess.check_output(
        ['docker', 'image', 'inspect', IMAGE, '--format', '{{json .Config.Env}}'], text=True,
    )) or []
    timeout_names = {'GOTRUE_SESSIONS_TIMEBOX', 'GOTRUE_SESSIONS_INACTIVITY_TIMEOUT',
                     'GOTRUE_SESSIONS_ALLOW_LOW_AAL'}
    if any(entry.split('=', 1)[0] in timeout_names for entry in image_environment):
        raise RuntimeError('Unexpected optional session timeout in qualification image')
    source = ROOT / 'qualification/auth-producer-probe.sql'
    live_migration = ROOT / 'supabase/migrations/20261101005400_auth_live_session.sql'
    opener = make_opener()
    checks: list[dict] = []
    verified_claims: list[dict] = []
    private_claims: list[dict] = []
    private_tokens: list[str] = []
    applied_migrations: list[dict] = []
    services = ExitStack()
    concurrency_service = None
    auth_service = None
    rest_service = None
    primary_failure = None
    cleanup_failures = []
    report = {'passed': False, 'scope': 'real GoTrue qualification'}
    source_paths = ['tools/qualify_auth_producer.py', 'tools/auth_concurrency_fixture.py',
        'tools/auth_oauth_fixture.py', 'tools/auth_admission_fixture.py', 'tools/auth_postgrest_fixture.py',
        'tools/disposable_postgres.py', 'tools/disposable_http_service.py', 'tools/qualification_http.py',
        'qualification/auth-producer-probe.sql', 'qualification/auth-application-scaffold.sql']
    source_paths += [str(path.relative_to(ROOT)) for path in sorted((ROOT / 'supabase/migrations').glob('*.sql'))]
    input_hashes = {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in source_paths}
    report.update(inputHashes=input_hashes, checks=checks, applicationMigrations=applied_migrations)


    def encoded(value: object) -> bytes:
        return base64.urlsafe_b64encode(json.dumps(value, separators=(',', ':')).encode()).rstrip(b'=')

    head = encoded({'alg': 'HS256', 'typ': 'JWT'})
    payload = encoded({'role': 'service_role', 'aud': 'authenticated', 'exp': int(time.time()) + 600})
    message = head + b'.' + payload
    admin_token = (message + b'.' + base64.urlsafe_b64encode(
        hmac.new(secret.encode(), message, hashlib.sha256).digest(),
    ).rstrip(b'=')).decode()

    def verify_access_token(token: str, record: bool = True) -> dict:
        """Verify actual signed output without retaining bearer or subject IDs."""
        def decode(value: str) -> bytes:
            return base64.urlsafe_b64decode(value + '=' * (-len(value) % 4))
        parts = token.split('.')
        if len(parts) != 3:
            raise RuntimeError('Malformed Auth bearer')
        header = json.loads(decode(parts[0]))
        signature = hmac.new(secret.encode(), (parts[0] + '.' + parts[1]).encode(), hashlib.sha256).digest()
        if header.get('alg') != 'HS256' or not hmac.compare_digest(signature, decode(parts[2])):
            raise RuntimeError('Auth bearer signature differs')
        claims = json.loads(decode(parts[1]))
        authority = claims.get('spellwake_auth')
        validity = {
            'issuer': claims.get('iss') == 'http://127.0.0.1:9999',
            'audience': claims.get('aud') in ('authenticated', ['authenticated']),
            'role': claims.get('role') == 'authenticated',
            'expiry': type(claims.get('exp')) is int and claims['exp'] > int(time.time()),
            'authorityKeys': isinstance(authority, dict) and set(authority) == {'version', 'method', 'bindingId', 'sessionGrantId'},
            'authorityVersion': isinstance(authority, dict) and type(authority.get('version')) is int and authority['version'] == 1,
            'authorityMethod': isinstance(authority, dict) and authority.get('method') in ('google', 'apple', 'discord', 'email_code'),
        }
        if not all(validity.values()):
            raise RuntimeError('Auth bearer authority contract differs: ' + json.dumps(validity))
        for key in ('bindingId', 'sessionGrantId'):
            if str(uuid.UUID(authority[key])) != authority[key]:
                raise RuntimeError('Auth authority identifier differs')
        subject = str(uuid.UUID(claims['sub']))
        session = str(uuid.UUID(claims['session_id']))
        if record:
            verified_claims.append({'authority': authority,
                                    'subjectHash': hashlib.sha256(subject.encode()).hexdigest(),
                                    'sessionHash': hashlib.sha256(session.encode()).hexdigest()})
            private_claims.append(claims)
            private_tokens.append(token)
        return claims

    def request(path: str, body: dict | None = None, admin: bool = False) -> tuple[int, dict]:
        headers = {'Content-Type': 'application/json'}
        if admin:
            headers['Authorization'] = 'Bearer ' + admin_token
        request = urllib.request.Request(
            base + path, data=None if body is None else json.dumps(body).encode(), headers=headers,
        )
        status, decoded = request_json(opener, request)
        value = object_json(decoded)
        if arguments.application_auth and status == 200 and isinstance(value.get('access_token'), str):
            verify_access_token(value['access_token'])
        return status, value

    def code() -> tuple[str, str]:
        status, value = request('/admin/generate_link',
                                {'type': 'magiclink', 'email': 'probe@example.invalid'}, True)
        if status != 200 or not isinstance(value.get('email_otp'), str):
            raise RuntimeError(f'Administrative fixture code generation failed: HTTP {status}')
        return value['email_otp'], value['verification_type']

    def snapshot() -> str:
        # Compare every canonical row, not just counts. Only the aggregate digest
        # leaves SQL; fixture codes/refresh tokens are never retained as evidence.
        tables = ('auth.users', 'auth.identities', 'auth.sessions', 'auth.mfa_amr_claims',
                  'auth.refresh_tokens', 'auth.one_time_tokens',
                  'auth.audit_log_entries', 'probe.observation')
        if arguments.application_auth:
            tables += ('app.command_receipt',)
        fields = ','.join("'" + table + "',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM "
                          + table + ' t)' for table in tables)
        return realm.sql("SELECT encode(sha256(convert_to(jsonb_build_object("
                         + fields + ")::text,'UTF8')),'hex')")

    try:
        with realm, DiscordFixture() as discord:
            try:
                realm.sql(f"ALTER ROLE {role} PASSWORD '{database_password}';", database='postgres')
                realm.sql(f'CREATE SCHEMA auth AUTHORIZATION {role};')
                realm.sql(f'ALTER ROLE {role} IN DATABASE {realm.database} SET search_path=auth,public;')
                config = {
                    'GOTRUE_DB_DRIVER': 'postgres', 'DB_NAMESPACE': 'auth',
                    'DATABASE_URL': f'postgres://{role}:{database_password}@host.docker.internal:{realm.port}/{realm.database}?sslmode=disable',
                    'API_EXTERNAL_URL': 'http://127.0.0.1:9999',
                    'GOTRUE_SITE_URL': 'http://127.0.0.1:9999', 'GOTRUE_API_HOST': '0.0.0.0', 'PORT': '9999',
                    'GOTRUE_JWT_SECRET': secret, 'GOTRUE_JWT_AUD': 'authenticated',
                    'GOTRUE_JWT_DEFAULT_GROUP_NAME': 'authenticated',
                    'GOTRUE_JWT_ISSUER': 'http://127.0.0.1:9999',
                    'GOTRUE_JWT_ADMIN_ROLES': 'service_role', 'GOTRUE_JWT_EXP': '3600',
                    'GOTRUE_SECURITY_REFRESH_TOKEN_ALGORITHM_VERSION': str(arguments.refresh_algorithm),
                    'GOTRUE_SECURITY_REFRESH_TOKEN_ROTATION_ENABLED': 'true',
                    'GOTRUE_SECURITY_REFRESH_TOKEN_REUSE_INTERVAL': '0',
                    'GOTRUE_SECURITY_REFRESH_TOKEN_ALLOW_REUSE': 'false',
                    'GOTRUE_SESSIONS_SINGLE_PER_USER': 'false',
                    'GOTRUE_EXTERNAL_EMAIL_ENABLED': 'true', 'GOTRUE_MAILER_AUTOCONFIRM': 'false',
                    'GOTRUE_SMTP_MAX_FREQUENCY': '0s',
                    'GOTRUE_RATE_LIMIT_EMAIL_SENT': '100',
                    'GOTRUE_DISABLE_SIGNUP': 'false', 'GOTRUE_DB_MAX_POOL_SIZE': '1',
                    'GOTRUE_DB_MAX_IDLE_POOL_SIZE': '1',
                    'GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_ENABLED': 'true',
                    'GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_URI': 'pg-functions://postgres/probe/token_hook',
                    'GOTRUE_HOOK_SEND_EMAIL_ENABLED': 'true',
                    'GOTRUE_HOOK_SEND_EMAIL_URI': 'pg-functions://postgres/probe/capture_mail',
                    'GOTRUE_EXTERNAL_DISCORD_ENABLED': 'true',
                    'GOTRUE_EXTERNAL_DISCORD_CLIENT_ID': 'fixture-client',
                    'GOTRUE_EXTERNAL_DISCORD_SECRET': 'fixture-secret',
                    'GOTRUE_EXTERNAL_DISCORD_URL': discord.url,
                    'GOTRUE_EXTERNAL_DISCORD_REDIRECT_URI': 'http://127.0.0.1:9999/callback',
                }
                auth_service = services.enter_context(DisposableHttpService('gotrue', config, stage))
                base = auth_service.base
                deadline = time.monotonic() + 30
                while True:
                    try:
                        status, health = request('/health')
                        if status == 200:
                            if health.get('version') != 'v2.197.0':
                                raise RuntimeError('Unexpected disposable Auth server version')
                            break
                    except (OSError, ValueError):
                        pass
                    if time.monotonic() >= deadline:
                        raise RuntimeError('Disposable GoTrue health deadline exceeded')
                    time.sleep(0.2)
                # Exercise the exact application function against real managed Auth,
                # not a second handwritten predicate or the bare-PG table fixtures.
                if arguments.application_auth:
                    realm.sql((ROOT / 'qualification/auth-application-scaffold.sql').read_text())
                    for migration in sorted((ROOT / 'supabase/migrations').glob('*.sql')):
                        realm.sql(migration.read_text())
                        applied_migrations.append({'name': migration.name, 'sha256': hashlib.sha256(migration.read_bytes()).hexdigest()})
                else:
                    realm.sql('CREATE SCHEMA app; REVOKE ALL ON SCHEMA app FROM PUBLIC;')
                    realm.sql(live_migration.read_text())
                realm.sql(source.read_text().replace('__TRUSTED_ROLE__', role)
                          .replace('__APPLICATION_AUTH__', 'true' if arguments.application_auth else 'false'))
                otp, kind = code()
                before = snapshot()
                realm.sql('UPDATE probe.control SET reject_token=true;')
                status, _ = request('/verify', {'type': kind, 'email': 'probe@example.invalid', 'token': otp})
                if status < 400 or snapshot() != before:
                    raise RuntimeError('Rejected hook did not roll back confirmation/session/audit')
                checks.append({'case': 'rejected_hook_rolls_back_signup', 'passed': True})
                realm.sql('UPDATE probe.control SET reject_token=false;')
                status, token = request('/verify', {'type': kind, 'email': 'probe@example.invalid', 'token': otp})
                if status != 200 or not isinstance(token.get('refresh_token'), str):
                    raise RuntimeError(f'Actual signup verify failed: HTTP {status}')
                original_refresh = token['refresh_token']
                first_session = str(uuid.UUID(realm.sql(
                    "SELECT s.id FROM auth.sessions s JOIN auth.users u ON u.id=s.user_id "
                    "WHERE u.email='probe@example.invalid' ORDER BY s.created_at LIMIT 1")))
                checks.append({'case': 'same_code_succeeds_after_rollback', 'passed': True})
                for _ in range(2):
                    status, token = request('/token?grant_type=refresh_token', {'refresh_token': token['refresh_token']})
                    if status != 200 or not isinstance(token.get('refresh_token'), str):
                        raise RuntimeError(f'Actual refresh failed: HTTP {status}')
                checks.append({'case': 'two_real_refreshes', 'passed': True})
                otp, kind = code()
                status, _ = request('/verify', {'type': kind, 'email': 'probe@example.invalid', 'token': otp})
                if status != 200:
                    raise RuntimeError(f'Existing-user verification failed: HTTP {status}')
                # With one pooled backend, a prior successful hook's local setting
                # must not authorize a later request whose capture trigger is absent.
                otp, kind = code()
                before = snapshot()
                capture_trigger = 'application_auth_provenance' if arguments.application_auth else 'producer_probe'
                realm.sql('ALTER TABLE auth.audit_log_entries DISABLE TRIGGER ' + capture_trigger + ';')
                status, _ = request('/verify', {'type': kind, 'email': 'probe@example.invalid', 'token': otp})
                if status < 400 or snapshot() != before:
                    raise RuntimeError('Missing capture accepted or did not roll back')
                realm.sql('ALTER TABLE auth.audit_log_entries ENABLE TRIGGER ' + capture_trigger + ';')
                status, _ = request('/verify', {'type': kind, 'email': 'probe@example.invalid', 'token': otp})
                if status != 200:
                    raise RuntimeError('Code retry after missing capture failed')
                checks.append({'case': 'pooled_missing_context_rejected_and_retry_succeeds', 'passed': True})
                observations = json.loads(realm.sql('SELECT jsonb_agg(value ORDER BY id) FROM probe.observation'))
                if len(observations) != 5 or len({item['backendPid'] for item in observations}) != 1:
                    raise RuntimeError('Expected five hook observations on one actual pooled backend')
                if len({item['xact'] for item in observations}) != 5:
                    raise RuntimeError('Actual requests unexpectedly share a transaction')
                refresh_action = 'token_revoked' if arguments.refresh_algorithm == 0 else 'token_refreshed'
                expected = [('user_signedup', 'otp', 'email'),
                            (refresh_action, 'token_refresh', None),
                            (refresh_action, 'token_refresh', None),
                            ('login', 'otp', None), ('login', 'otp', None)]
                actual = [(item['action'], item['method'], item['provider']) for item in observations]
                if actual != expected or any(item['amr'] != ['otp'] for item in observations):
                    raise RuntimeError('Pinned producer/method/AMR labels differ')
                if len({item['sessionHash'] for item in observations[:3]}) != 1:
                    raise RuntimeError('Refresh did not preserve the original actual session')
                if len({item['sessionHash'] for item in observations}) != 3:
                    raise RuntimeError('New interactive logins did not create distinct sessions')
                checks.append({'case': 'pinned_producer_labels_and_session_lineage', 'passed': True})
                checks.append({'case': 'same_pool_connection_distinct_transactions', 'passed': True})
                for index in range(2):
                    count = realm.sql('SELECT count(*) FROM probe.observation')
                    status, _ = request('/otp', {'email': 'public-probe@example.invalid', 'create_user': True})
                    if status != 200 or realm.sql('SELECT count(*) FROM probe.observation') != count:
                        raise RuntimeError('Public OTP request failed or unexpectedly issued a token')
                    delivery = json.loads(realm.sql(
                        "DELETE FROM probe.mailbox RETURNING jsonb_build_object('token',token,'action',action)"))
                    if delivery['action'] != ('signup' if index == 0 else 'magiclink'):
                        raise RuntimeError('Public OTP delivery action differs')
                    status, _ = request('/verify', {'type': 'email', 'email': 'public-probe@example.invalid',
                                                   'token': delivery['token']})
                    if status != 200:
                        raise RuntimeError(f'Public-issued OTP verification failed: HTTP {status}')
                observations = json.loads(realm.sql('SELECT jsonb_agg(value ORDER BY id) FROM probe.observation'))
                if len(observations) != 7 or [(item['action'], item['method'], item['provider'])
                                             for item in observations[-2:]] != [
                        ('user_signedup', 'otp', 'email'), ('login', 'otp', None)]:
                    raise RuntimeError('Public OTP producer labels differ')
                if any(item['amr'] != ['otp'] for item in observations):
                    raise RuntimeError('Public OTP AMR differs')
                if len({item['backendPid'] for item in observations}) != 1 or len({item['xact'] for item in observations}) != 7:
                    raise RuntimeError('Public OTP pool/transaction observations differ')
                if len({item['sessionHash'] for item in observations}) != 5:
                    raise RuntimeError('Public OTP did not create distinct interactive sessions')
                checks.append({'case': 'public_otp_signup_and_existing_login_via_test_sink', 'passed': True})
                checks.extend(qualify_oauth(base, opener, request, realm.sql, snapshot,
                                           verify_access_token if arguments.application_auth else None))
                if arguments.application_auth:
                    status, _ = request('/otp', {'email': 'discord-probe@example.invalid', 'create_user': True})
                    if status != 200:
                        raise RuntimeError('Alternate-method test code request failed')
                    delivery = json.loads(realm.sql(
                        "DELETE FROM probe.mailbox RETURNING jsonb_build_object('token',token,'action',action)"))
                    before = snapshot()
                    status, _ = request('/verify', {'type': 'email', 'email': 'discord-probe@example.invalid',
                                                   'token': delivery['token']})
                    if status < 400 or snapshot() != before:
                        raise RuntimeError('Discord subject accepted an alternate email method or mutated authority')
                    qualify_oauth(base, opener, request, realm.sql, snapshot, verify_access_token,
                                  expected_implicit_action='login')
                    checks.append({'case': 'alternate_email_denied_and_original_discord_remains_usable', 'passed': True})
                observations = json.loads(realm.sql('SELECT jsonb_agg(value ORDER BY id) FROM probe.observation'))
                authority_counts = None
                if arguments.application_auth:
                    if len(verified_claims) != 11 or any(item['login'] != 'supabase_auth_admin' for item in observations):
                        raise RuntimeError('Expected eleven signed claims through canonical Auth LOGIN')
                    for index, (observed, verified) in enumerate(zip(observations, verified_claims, strict=True)):
                        if observed['authority'] != verified['authority'] or observed['sessionHash'] != verified['sessionHash']:
                            raise RuntimeError('Signed Auth authority differs from committed hook result')
                        authority = verified['authority']
                        if authority['method'] != ('email_code' if index < 7 else 'discord'):
                            raise RuntimeError('Signed method does not match the current producer')
                        persisted = realm.sql(
                            "SELECT EXISTS(SELECT 1 FROM app.command_receipt b JOIN app.command_receipt g ON g.target_ref=b.id::text "
                            f"WHERE b.id='{authority['bindingId']}' AND g.id='{authority['sessionGrantId']}' "
                            f"AND b.principal_key='auth:{verified['subjectHash']}' AND g.principal_key=b.principal_key "
                            "AND b.scope='auth_binding_v1' AND g.scope='auth_session_v1' AND b.state='accepted' AND g.state='accepted' "
                            f"AND g.request_ref='{verified['sessionHash']}' AND b.result_ref::jsonb->>'method'='{authority['method']}')")
                        if persisted != 't':
                            raise RuntimeError('Signed authority does not select matching committed receipts')
                    if any(item['authority'] != verified_claims[0]['authority'] for item in verified_claims[:3]):
                        raise RuntimeError('Refresh changed immutable authority')
                    authority_counts = json.loads(realm.sql(
                        "SELECT jsonb_build_object('bindings',(SELECT count(*) FROM app.command_receipt WHERE scope='auth_binding_v1' AND state='accepted'),"
                        "'sessions',(SELECT count(*) FROM app.command_receipt WHERE scope='auth_session_v1' AND state='accepted'),"
                        "'accounts',(SELECT count(*) FROM app.account))"))
                    if authority_counts != {'bindings': 3, 'sessions': 9, 'accounts': 0}:
                        raise RuntimeError('Application authority count differs')
                    checks.append({'case': 'exact_application_hook_signed_authority_and_refresh_lineage', 'passed': True})
                registration = None
                after_revocation = None
                if arguments.application_auth:
                    registration, after_revocation = qualify_admission(private_claims[0], realm.sql)
                    checks.append({'case': 'signed_claims_registration_replay_and_owner_rls', 'passed': True})
                http_report = None
                after_http_revocation = None
                rest_service = None
                if arguments.postgrest:
                    http_report, after_http_revocation, rest_service = qualify_postgrest(
                        services, stage, realm.database, realm.port, secret, private_tokens, private_claims, realm.sql, opener)
                    checks.append({'case': 'actual_http_bearer_signature_roles_root_authority_and_write_denial', 'passed': True})

                def session_state() -> dict:
                    return json.loads(realm.sql(
                        "SELECT jsonb_build_object('sessionExists',EXISTS(SELECT 1 FROM auth.sessions "
                        f"WHERE id='{first_session}'),'admitted',app.auth_session_is_live("
                        f"(SELECT user_id FROM auth.sessions WHERE id='{first_session}'),'{first_session}','otp'))"))

                before_abuse = session_state()
                status, _ = request('/token?grant_type=refresh_token', {'refresh_token': original_refresh})
                after_abuse = session_state()
                if not before_abuse['admitted'] or status < 400 or after_abuse['admitted']:
                    raise RuntimeError('Committed native refresh reuse did not revoke session admission')
                if after_abuse['sessionExists'] != (arguments.refresh_algorithm == 0):
                    raise RuntimeError('Pinned native legacy/counter revocation shape differs')
                checks.append({'case': 'native_refresh_reuse_revokes_shared_session_predicate', 'passed': True})
                if after_revocation is not None:
                    after_revocation()
                    if registration is None:
                        raise RuntimeError('Admission check lost its registration evidence')
                    registration['revokedTokenDenied'] = True
                    checks.append({'case': 'native_reuse_denies_old_jwt_rls_and_registration', 'passed': True})
                if after_http_revocation is not None:
                    after_http_revocation()
                    checks.append({'case': 'actual_http_revoked_token_denied_other_session_admitted', 'passed': True})
                concurrency_report = None
                if arguments.identity_concurrency:
                    concurrency_stage = stage / 'concurrency-service'
                    concurrency_stage.mkdir(mode=0o700)
                    concurrent_config = config | {'GOTRUE_DB_MAX_POOL_SIZE': '4', 'GOTRUE_DB_MAX_IDLE_POOL_SIZE': '4', 'GOTRUE_SMTP_MAX_FREQUENCY': '1ms'}
                    concurrency_service = services.enter_context(DisposableHttpService('gotrue', concurrent_config, concurrency_stage))
                    deadline = time.monotonic() + 30
                    while True:
                        try:
                            response, concurrent_health = request_json(make_opener(), urllib.request.Request(concurrency_service.base + '/health'))
                            if response == 200 and object_json(concurrent_health).get('version') == 'v2.197.0':
                                break
                        except (OSError, ValueError):
                            pass
                        if time.monotonic() >= deadline:
                            raise RuntimeError('Owned concurrent Auth service readiness failed')
                        time.sleep(0.1)
                    if arguments.qualification_fault == 'after-concurrency-start':
                        raise RuntimeError('Intentional owned concurrency lifecycle fault')
                    concurrency_report = qualify_concurrency(concurrency_service.base, discord, realm.sql,
                        lambda token: verify_access_token(token, record=False))
                if input_hashes != {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in source_paths}:
                    raise RuntimeError('Auth qualification inputs changed during execution')
                report = {
                    'scope': 'real GoTrue exact application hook' if arguments.application_auth else 'real GoTrue transaction provenance only',
                    'image': IMAGE, 'imageId': image_id, 'healthVersion': health.get('version'),
                    'refreshAlgorithm': arguments.refresh_algorithm,
                    'databaseImageId': realm.image_id, 'databaseVersion': realm.server_version,
                    # Pinned source rejects zero-duration pointers; absent optional
                    # env keys intentionally leave these three pointers unset.
                    'sessionConfig': {'singlePerUser': False, 'optionalTimeoutsUnset': True},
                    'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                    'runnerSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    'lifecycleSha256': hashlib.sha256((ROOT / 'tools/disposable_postgres.py').read_bytes()).hexdigest(),
                    'httpLifecycleSha256': hashlib.sha256((ROOT / 'tools/disposable_http_service.py').read_bytes()).hexdigest(),
                    'oauthFixtureSha256': hashlib.sha256((ROOT / 'tools/auth_oauth_fixture.py').read_bytes()).hexdigest(),
                    'admissionFixtureSha256': hashlib.sha256((ROOT / 'tools/auth_admission_fixture.py').read_bytes()).hexdigest(),
                    'postgrestFixtureSha256': hashlib.sha256((ROOT / 'tools/auth_postgrest_fixture.py').read_bytes()).hexdigest(),
                    'liveSessionMigrationSha256': hashlib.sha256(live_migration.read_bytes()).hexdigest(),
                    'applicationAuth': arguments.application_auth, 'applicationMigrations': applied_migrations,
                    'scaffoldSha256': hashlib.sha256((ROOT / 'qualification/auth-application-scaffold.sql').read_bytes()).hexdigest() if arguments.application_auth else None,
                    'inputHashes': input_hashes, 'identityConcurrency': concurrency_report,
                    'checks': checks, 'observations': observations,
                    'verifiedClaims': verified_claims, 'preRegistrationAuthorityCounts': authority_counts,
                    'registration': registration,
                    'postgrest': http_report,
                    'nativeRevocation': {'before': before_abuse, 'after': after_abuse},
                    'excluded': ['external mailbox delivery', 'real external provider verification', 'Google/Apple integration',
                                 'Spellwake HTTP actor', 'hosted support']
                                + ([] if arguments.identity_concurrency else ['identity concurrency'])
                                + ([] if arguments.postgrest else ['PostgREST middleware'])
                                + ([] if arguments.application_auth else ['binding/session grants', 'registration/application RLS']),
                }
            except BaseException as failure:
                primary_failure = failure
            finally:
                try:
                    services.close()
                except BaseException as failure:
                    cleanup_failures.append(type(failure).__name__)
    except BaseException as failure:
        if primary_failure is None:
            primary_failure = failure
        else:
            cleanup_failures.append(type(failure).__name__)
    report['cleanupVerified'] = realm.cleanup_verified and auth_service is not None and auth_service.cleanup_verified
    report['concurrentServiceCleanupVerified'] = concurrency_service.cleanup_verified if concurrency_service is not None else None
    report['postgrestCleanupVerified'] = rest_service.cleanup_verified if rest_service is not None else None
    report['databaseVolumeCleanupVerified'] = realm.volume_cleanup_verified if arguments.application_auth else None
    report['passed'] = primary_failure is None and not cleanup_failures and report['cleanupVerified'] and (not arguments.identity_concurrency or report['concurrentServiceCleanupVerified']) and (not arguments.postgrest or report['postgrestCleanupVerified']) and (not arguments.application_auth or report['databaseVolumeCleanupVerified'])
    report['injectedFault'] = arguments.qualification_fault
    report['resourceNames'] = {'databaseContainer':realm.container, 'databaseVolumes':sorted(getattr(realm,'owned_volumes',set())),
        'authContainer':auth_service.container if auth_service is not None else None,
        'concurrentAuthContainer':concurrency_service.container if concurrency_service is not None else None,
        'postgrestContainer':rest_service.container if rest_service is not None else None}
    report['primaryFailure'] = type(primary_failure).__name__ if primary_failure is not None else None
    report['cleanupFailures'] = cleanup_failures
    if primary_failure is not None:
        fd=os.open(stage / 'private-primary-failure.txt',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,'w') as file:
            file.write(str(primary_failure))
    fd=os.open(stage / 'result.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'w') as file:
        file.write(json.dumps(report,indent=2)+'\n')
    if not report['passed']:
        raise RuntimeError('Owned Auth qualification failed; private result: '+str(stage / 'result.json')) from None
    print('Real GoTrue producer probe passed; normalized result:', stage / 'result.json')


if __name__ == '__main__':
    main()
