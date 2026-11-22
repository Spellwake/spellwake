"""Real owned Supabase gateway/direct-hook/native-actor qualification; no hosted activation."""
import base64
import hashlib
import json
from pathlib import Path
import re
import secrets
import socket
import time
import urllib.request
import uuid

from auth_admission_fixture import qualify_admission
from audit_asset_authority import read_asset_authority, assert_retired_asset_authority
from disposable_supabase import DisposableSupabase, ROOT, private_write
from qualification_http import make_opener, request_json, object_json

TEST = 'actor::tests::owned_managed_gateway_resolves_canonical_actor'


def main() -> None:
    """Keep primary and cleanup results separate and all credentials in memory/private files."""
    realm = DisposableSupabase()
    result: dict = {'scope': 'managed-local-direct-hook-native-actor-account-api-identity-cleanup', 'passed': False, 'cleanupVerified': False}
    source_paths = ['tools/qualify_managed_actor.py', 'tools/disposable_supabase.py', 'tools/loopback_docker.py',
        'tools/qualification_http.py', 'tools/audit_asset_authority.py', 'tools/auth_admission_fixture.py', 'src/actor.rs', 'src/actor_tests.rs',
        'src/http_transport.rs', 'src/backend.rs', 'src/api.rs', 'src/api_tests.rs', 'src/inventory.rs',
        'src/bin/application_api.rs', 'src/lib.rs', 'tests/bootstrap.sql', 'tests/database/auth_identity.rs',
        'tests/database/asset_lifecycle.rs', 'tests/database/auth_admission.rs', 'tests/database/auth_sessions.rs', 'tests/database/auth_provenance.rs', 'Cargo.toml', 'Cargo.lock']
    source_paths += [str(path.relative_to(ROOT)) for path in sorted((ROOT / 'supabase/migrations').glob('*.sql'))]
    source_paths += [str(path.relative_to(ROOT)) for path in sorted((ROOT / 'src').rglob('*.rs'))]
    sources = {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in source_paths}
    primary: BaseException | None = None
    try:
        realm.command(['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--bin', 'application_api'], timeout=180)
        realm.__enter__()
        base = realm.status['API_URL']
        public = realm.status['PUBLISHABLE_KEY']
        secret = realm.status.get('SECRET_KEY', '')
        if not secret.startswith('sb_secret_'):
            raise RuntimeError('Owned admin gateway key unavailable')
        opener = make_opener()
        result['httpStatuses'] = []
        result['apiStatuses'] = []
        api_base = ''

        def api(path: str, bearer: str | None = None, body: dict | None = None) -> tuple[int, object]:
            if not api_base or path not in ('/health', '/v1/session', '/v1/account', '/v1/inventory', '/v1/account/deletion', '/v1/account/deletion/observe'):
                raise RuntimeError('Unexpected owned application fixture path')
            headers = {'Content-Type': 'application/json'}
            if bearer is not None:
                headers['Authorization'] = 'Bearer ' + bearer
            status, value = request_json(opener, urllib.request.Request(api_base + path, headers=headers,
                data=None if body is None else json.dumps(body).encode()))
            result['apiStatuses'].append({'path': path, 'status': status})
            return status, value

        def api_inventory(bearer: str) -> None:
            status, value = api('/v1/inventory', bearer)
            data = object_json(object_json(value).get('data'))
            items = data.get('items')
            if status != 200 or object_json(value).get('status') != 'success' or not isinstance(items, list) or len(items) != 1:
                raise RuntimeError('Owned application inventory shape differs')
            item = object_json(items[0])
            if item.get('owned') != '14500' or item.get('held') != '0' or item.get('available') != '14500' or object_json(item.get('asset')).get('kind') != 'coin' or data.get('nextCursor') is not None:
                raise RuntimeError('Owned application inventory quantities differ')

        def request(path: str, body: dict | None = None, bearer: str | None = None,
                    admin: bool = False, key: str | None = public, method: str | None = None) -> tuple[int, object]:
            if not path.startswith(('/auth/v1/', '/rest/v1/')) or '..' in path:
                raise RuntimeError('Unexpected owned gateway fixture path')
            headers = {'Content-Type': 'application/json', 'Accept-Profile': 'app', 'Content-Profile': 'app'}
            if key is not None:
                headers['apikey'] = secret if admin else key
            if bearer:
                headers['Authorization'] = 'Bearer ' + bearer
            command = urllib.request.Request(base + path, method=method, headers=headers,
                        data=None if body is None else json.dumps(body).encode())
            status, value = request_json(opener, command)
            result['httpStatuses'].append({'path': path.split('?', 1)[0], 'status': status})
            if status >= 400:
                private_write(realm.stage / ('private-http-error-' + str(len(result['httpStatuses'])) + '.json'), json.dumps(value).encode())
            return status, value

        history = json.loads(realm.sql("SELECT coalesce(jsonb_agg(version ORDER BY version),'[]'::jsonb) FROM supabase_migrations.schema_migrations"))
        expected = [row['file'].split('_', 1)[0] for row in realm.manifest]
        if len(expected) != 63 or history != expected:
            raise RuntimeError('Owned applied migration history differs')
        body = realm.sql("SELECT to_json(prosrc) FROM pg_proc WHERE oid='app.custom_access_token_hook(jsonb)'::regprocedure")
        hook_body = json.loads(body)
        source = (ROOT / 'supabase/migrations/20261101005500_auth_provenance_and_grants.sql').read_text()
        match = re.search(r'CREATE FUNCTION app\.custom_access_token_hook\(p_event jsonb\).*?AS \$\$(.*?)\$\$;', source, re.S)
        if match is None or hook_body != match.group(1):
            raise RuntimeError('Owned direct hook body differs from exact source')
        installation = json.loads(realm.sql("SELECT jsonb_build_object('authLogin',(SELECT rolcanlogin FROM pg_roles WHERE rolname='supabase_auth_admin'),"
            "'authHookExecute',has_function_privilege('supabase_auth_admin','app.custom_access_token_hook(jsonb)','EXECUTE'),"
            "'clientHookExecute',has_function_privilege('authenticated','app.custom_access_token_hook(jsonb)','EXECUTE'),"
            "'captureEnabled',EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='auth.audit_log_entries'::regclass AND tgname='application_auth_provenance' AND tgenabled='O'))"))
        if installation != {'authLogin': True, 'authHookExecute': True, 'clientHookExecute': False, 'captureEnabled': True}:
            raise RuntimeError('Owned hook role/trigger installation differs')
        hook_config = realm.command(['docker', 'inspect', '--format', '{{range .Config.Env}}{{if or (eq . "GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_ENABLED=true") (eq . "GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_URI=pg-functions://postgres/app/custom_access_token_hook")}}{{println .}}{{end}}{{end}}', 'supabase_auth_' + realm.project]).decode().splitlines()
        hook_config = [line for line in hook_config if line]
        if set(hook_config) != {'GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_ENABLED=true', 'GOTRUE_HOOK_CUSTOM_ACCESS_TOKEN_URI=pg-functions://postgres/app/custom_access_token_hook'} or len(hook_config) != 2:
            raise RuntimeError('Owned runtime hook configuration differs')

        def login(email: str, create: bool) -> tuple[str, dict]:
            if create and realm.sql("SELECT count(*) FROM auth.users WHERE email='" + email + "'") != '0':
                raise RuntimeError('Owned initial email identity unexpectedly exists')
            # Admin generate_link creates the absent user and returns its actual
            # signup verification kind. Precreating an unverified identity and
            # forcing a magiclink would skip that canonical confirmation path.
            status, generated = request('/auth/v1/admin/generate_link', {'type': 'magiclink', 'email': email}, admin=True)
            generated = object_json(generated)
            properties = object_json(generated.get('properties', generated))
            if status != 200 or not isinstance(properties.get('email_otp'), str):
                raise RuntimeError('Owned admin-origin code generation failed')
            status, signed = request('/auth/v1/verify', {'email': email, 'token': properties['email_otp'], 'type': properties['verification_type']})
            signed = object_json(signed)
            if status != 200 or not isinstance(signed.get('access_token'), str):
                raise RuntimeError('Owned direct-hook code redemption failed')
            bearer = signed['access_token']
            segments = bearer.split('.')
            if len(segments) != 3:
                raise RuntimeError('Owned token encoding differs')
            claims = object_json(json.loads(base64.urlsafe_b64decode(segments[1] + '=' * (-len(segments[1]) % 4))))
            if claims.get('iss') != base + '/auth/v1' or claims.get('role') != 'authenticated' or claims.get('exp', 0) <= time.time():
                raise RuntimeError('Owned token issuer/role/expiry differs')
            uuid.UUID(claims['sub']); uuid.UUID(claims['session_id'])
            return bearer, claims

        def native(bearer: str, claims: dict, expected_state: str, account: str = 'none', issuer: str | None = None, key: str | None = None) -> None:
            environment = {'SPELLWAKE_ACTOR_FIXTURE_URL': base, 'SPELLWAKE_ACTOR_FIXTURE_PUBLIC_KEY': public if key is None else key,
                'SPELLWAKE_ACTOR_FIXTURE_ISSUER': base + '/auth/v1' if issuer is None else issuer, 'SPELLWAKE_ACTOR_FIXTURE_BEARER': bearer,
                'SPELLWAKE_ACTOR_FIXTURE_EXPECTED': expected_state, 'SPELLWAKE_ACTOR_FIXTURE_SUBJECT': claims['sub'],
                'SPELLWAKE_ACTOR_FIXTURE_SESSION': claims['session_id'], 'SPELLWAKE_ACTOR_FIXTURE_ACCOUNT': account}
            output = realm.command(['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--lib', TEST,
                '--', '--ignored', '--exact', '--nocapture'], timeout=180, extra_environment=environment)
            if output.count(b'OWNED_MANAGED_ACTOR_CASE_OK') != 1 or b'test result: ok. 1 passed;' not in output:
                raise RuntimeError('Owned native actor fixture did not execute exactly once')

        first, claims = login('actor-one@example.invalid', True)
        native(first, claims, 'needs_registration')
        native(first, claims, 'unauthorized', issuer=base + '/wrong-issuer')
        parts = first.split('.')
        signature = bytearray(base64.urlsafe_b64decode(parts[2] + '=' * (-len(parts[2]) % 4)))
        signature[0] ^= 1
        forged = '.'.join([parts[0], parts[1], base64.urlsafe_b64encode(signature).decode().rstrip('=')])
        native(forged, claims, 'unauthorized')
        if request('/rest/v1/rpc/read_current_session', {}, forged)[0] != 401:
            raise RuntimeError('Managed remote signature rejection differs')
        def first_registration(ids: list[str], username: str) -> str:
            nonlocal api_base
            with socket.socket() as listener:
                listener.bind(('127.0.0.1', 0))
                port = listener.getsockname()[1]
            api_base = 'http://127.0.0.1:' + str(port)
            process = realm.start_background([str(ROOT / 'target/debug/application_api')], {
                'SPELLWAKE_SUPABASE_URL': base, 'SPELLWAKE_SUPABASE_PUBLISHABLE_KEY': public,
                'SPELLWAKE_SUPABASE_SECRET_KEY': secret, 'SPELLWAKE_AUTH_ISSUER': base + '/auth/v1',
                'SPELLWAKE_FREE_BACK_ID': ids[0], 'SPELLWAKE_FREE_SLEEVE_ID': ids[1], 'SPELLWAKE_FREE_DICE_ID': ids[2],
                'SPELLWAKE_API_BIND': '127.0.0.1:' + str(port),
            })
            deadline = time.monotonic() + 15
            while True:
                if process.poll() is not None:
                    raise RuntimeError('Owned application exited before readiness')
                try:
                    ready = api('/health')
                except (OSError, RuntimeError):
                    ready = None
                if ready == (200, {'status': 'success', 'data': {'service': 'spellwake-application', 'gameplayEnabled': False}}):
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError('Owned application health did not become ready')
                time.sleep(0.05)
            setup = {'status': 'success', 'data': {'state': 'needs_registration', 'method': 'email_code',
                'subjectId': claims['sub'], 'sessionId': claims['session_id'], 'accountId': None}}
            if api('/v1/session', first) != (200, setup):
                raise RuntimeError('Owned application first setup identity differs')
            status, registered = api('/v1/account', first, {'username': username})
            if status != 200 or object_json(registered).get('status') != 'success':
                raise RuntimeError('Owned application first registration failed')
            account_id = str(uuid.UUID(object_json(object_json(registered).get('data'))['accountId']))
            if api('/v1/account', first, {'username': 'ignored_replay_name'}) != (200, registered):
                raise RuntimeError('Owned application registration retry differs')
            active = json.loads(json.dumps(setup))
            active['data'].update(state='active', accountId=account_id)
            if api('/v1/session', first) != (200, active):
                raise RuntimeError('Owned application active identity differs')
            if api('/v1/session')[0] != 401 or api('/v1/session', forged)[0] != 401 or api('/v1/inventory', forged)[0] != 401:
                raise RuntimeError('Owned application admitted missing or forged bearer')
            api_inventory(first)
            return account_id

        registration, after_revocation = qualify_admission(claims, realm.sql, first_registration)
        account = str(uuid.UUID(realm.sql("SELECT id FROM app.account WHERE auth_subject='" + str(uuid.UUID(claims['sub'])) + "'")))
        native(first, claims, 'active', account)
        second, second_claims = login('actor-one@example.invalid', False)
        if second_claims['sub'] != claims['sub'] or second_claims['session_id'] == claims['session_id']:
            raise RuntimeError('Owned second same-subject session differs')
        native(second, second_claims, 'active', account)
        other, other_claims = login('actor-two@example.invalid', True)
        native(other, other_claims, 'needs_registration')
        if api('/v1/inventory', other) != (409, {'status': 'fail', 'data': {'account': 'unavailable'}}):
            raise RuntimeError('Other owned subject read application owner inventory')
        if request('/rest/v1/coin_balance?select=quantity::text', bearer=other) != (200, []):
            raise RuntimeError('Other owned subject read registered inventory')
        if request('/rest/v1/coin_balance?select=quantity::text', bearer=first) != (200, [{'quantity': '14500'}]):
            raise RuntimeError('Managed owner inventory differs')
        denied = request('/rest/v1/rpc/register_account', {'p_subject': claims['sub'], 'p_session': claims['session_id'],
            'p_username': 'forbidden', 'p_back': str(uuid.uuid4()), 'p_sleeve': str(uuid.uuid4()), 'p_dice': str(uuid.uuid4())}, first)[0]
        if denied not in (403, 404):
            raise RuntimeError('Managed public caller reached service-only registration')
        native(first, claims, 'active', account, key='sb_publishable_invalid_fixture_only')
        canonical = {'status': 'success', 'data': {'subjectId': claims['sub'], 'sessionId': claims['session_id'],
            'method': 'email_code', 'state': 'active', 'accountId': account}}
        unauthorized = {'status': 'fail', 'data': {'session': 'unauthorized'}}
        for key in (None, 'sb_publishable_invalid_fixture_only'):
            if request('/rest/v1/rpc/read_current_session', {}, first, key=key) != (200, canonical):
                raise RuntimeError('Local gateway did not preserve the same valid caller identity')
            for invalid_bearer in (None, forged):
                status, value = request('/rest/v1/rpc/read_current_session', {}, invalid_bearer, key=key)
                if not (status in (401, 403, 404) or (status == 200 and value == unauthorized)):
                    raise RuntimeError('Gateway key without a valid bearer obtained canonical authority')
                status, value = request('/rest/v1/coin_balance?select=quantity::text', bearer=invalid_bearer, key=key)
                if not (status in (401, 403, 404) or (status == 200 and value == [])):
                    raise RuntimeError('Gateway key without a valid bearer read protected inventory')
        if claims['exp'] <= time.time():
            raise RuntimeError('Original fixture token expired before revocation proof')
        if request('/auth/v1/logout?scope=local', {}, first)[0] not in (200, 204):
            raise RuntimeError('Owned local-session logout failed')
        native(first, claims, 'unauthorized')
        if api('/v1/session', first)[0] != 401 or api('/v1/inventory', first)[0] != 401 or api('/v1/account', first, {'username': 'revoked_actor'})[0] != 401:
            raise RuntimeError('Owned application admitted revoked original session')
        api_inventory(second)
        after_revocation()
        native(second, second_claims, 'active', account)
        if request('/rest/v1/coin_balance?select=quantity::text', bearer=second) != (200, [{'quantity': '14500'}]):
            raise RuntimeError('Other same-subject live session lost managed wallet access')
        if request('/rest/v1/coin_balance?select=quantity::text', bearer=first) != (200, []):
            raise RuntimeError('Logged-out original bearer retained managed inventory')
        # Actual admin requests must follow the application's canonical cleanup
        # guard; these are not direct fixture DELETEs or a completed finalizer.
        active_subject = str(uuid.UUID(claims['sub']))
        identity_key_sql = "SELECT app.auth_json_hash(jsonb_build_object('id',id,'provider',provider,'providerId',provider_id,'userId',user_id)) FROM auth.identities WHERE user_id='" + active_subject + "' AND provider='email'"
        before_key = realm.sql(identity_key_sql)
        status, _ = request('/auth/v1/admin/users/' + active_subject,
            {'email': 'actor-one-updated@example.invalid', 'email_confirm': True}, admin=True, method='PUT')
        if status != 200 or realm.sql(identity_key_sql) != before_key or realm.sql("SELECT ((identity_data->>'email')='actor-one-updated@example.invalid' AND identity_data->'email_verified'='true'::jsonb) FROM auth.identities WHERE user_id='" + active_subject + "' AND provider='email'") != 't':
            raise RuntimeError('Actual bound-family metadata refresh changed its identity key or failed')
        native(second, second_claims, 'active', account)
        api_inventory(second)
        for soft in (True, False):
            status, _ = request('/auth/v1/admin/users/' + str(uuid.UUID(claims['sub'])),
                {'should_soft_delete': soft}, admin=True, method='DELETE')
            if status < 400:
                raise RuntimeError('Registered active account bypassed Auth cleanup guard')
            native(second, second_claims, 'active', account)
            api_inventory(second)
        status, _ = request('/auth/v1/admin/users/' + str(uuid.UUID(other_claims['sub'])),
            {'should_soft_delete': True}, admin=True, method='DELETE')
        if status < 400:
            raise RuntimeError('Unregistered bound subject accepted soft cleanup')
        native(other, other_claims, 'needs_registration')
        status, _ = request('/auth/v1/admin/users/' + str(uuid.UUID(other_claims['sub'])),
            {'should_soft_delete': False}, admin=True, method='DELETE')
        if status != 200:
            raise RuntimeError('Unregistered bound subject canonical hard cleanup failed')
        native(other, other_claims, 'unauthorized')
        if api('/v1/session', other)[0] != 401 or api('/v1/inventory', other)[0] != 401:
            raise RuntimeError('Deleted unregistered subject retained application authority')
        target = str(uuid.UUID(other_claims['sub']))
        cleanup = json.loads(realm.sql("SELECT jsonb_build_object('users',(SELECT count(*) FROM auth.users WHERE id='" + target + "'),"
            "'identities',(SELECT count(*) FROM auth.identities WHERE user_id='" + target + "'),"
            "'sessions',(SELECT count(*) FROM auth.sessions WHERE user_id='" + target + "'),"
            "'retirement',(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash('" + target + "') AND scope='auth_retirement_v1' AND target_ref='unregistered' AND state='accepted'),"
            "'cleanupReceipt',(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash('" + target + "') AND scope='auth_cleanup_v1' AND target_ref='unregistered' AND state='accepted'))"))
        if cleanup != {'users': 0, 'identities': 0, 'sessions': 0, 'retirement': 1, 'cleanupReceipt': 1}:
            raise RuntimeError('Actual unregistered cleanup did not retire and remove exact target')
        observer_bytes = secrets.token_bytes(32)
        observer_key = base64.urlsafe_b64encode(observer_bytes).decode().rstrip('=')
        deletion_request = str(uuid.uuid4())
        status, pending = api('/v1/account/deletion', second,
            {'requestId': deletion_request, 'statusKeyHash': hashlib.sha256(observer_bytes).hexdigest()})
        if status != 202 or object_json(pending).get('status') != 'success':
            raise RuntimeError('Owned authenticated application deletion request failed')
        observer = {'accountId': account, 'requestId': deletion_request, 'statusKey': observer_key}
        expected_observation = {'status': 'success', 'data': {'accountId': account, 'requestId': deletion_request, 'phase': 'pending'}}
        if api('/v1/account/deletion/observe', body=observer) != (200, expected_observation):
            raise RuntimeError('Owned private deletion observer did not recover pending intent')
        if api('/v1/account/deletion/observe', body={**observer, 'statusKey': 'A' * 43}) != (404, {'status': 'fail', 'data': {'observer': 'unavailable'}}):
            raise RuntimeError('Owned private deletion observer accepted another capability')
        native(second, second_claims, 'deletion_pending', account)
        status, _ = request('/auth/v1/admin/users/' + str(uuid.UUID(claims['sub'])),
            {'should_soft_delete': False}, admin=True, method='DELETE')
        if status < 400:
            raise RuntimeError('Pending account bypassed Auth retirement requirement')
        native(second, second_claims, 'deletion_pending', account)
        api_inventory(second)
        asset_authority = read_asset_authority(realm.sql)
        assert_retired_asset_authority(asset_authority)
        if sources != {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in source_paths}:
            raise RuntimeError('Qualification sources changed during execution')
        result.update(passed=True, sourceHashes=sources, publicationEvents=[json.loads(line) for line in realm.events.read_text().splitlines()], observedVolumes=sorted(realm.observed_volumes), checks={'directHookInstalled': True, 'nativeSetupToActive': True, 'wrongIssuerDenied': True,
            'remoteAlteredSignatureDenied': True, 'exactIdentityAssertions': True, 'otherSubjectSetupAndPrivateRows': True,
            'serviceOnlyRegistrationDenied': True, 'validJwtHasLocalKeyPrecedence': True, 'noValidJwtNoAuthority': True, 'unexpiredOriginalLogoutDenied': True,
            'sameSubjectOtherSessionPreserved': True, 'sameSubjectOtherSessionWalletPreserved': True, 'nativeWrongPublicKeySameCanonicalActor': True, 'apiFirstRegistrationAndRetry': True, 'apiOwnerInventory': True,
            'apiMissingForgedRevokedDenied': True, 'apiOtherSubjectDenied': True, 'apiSameSubjectOtherSessionPreserved': True, 'registeredActiveHardCleanupDenied': True,
            'registeredPendingHardCleanupDenied': True, 'actualSoftCleanupDenied': True, 'unregisteredHardCleanupRetiresAndCascades': True,
            'deletedUnregisteredBearerDenied': True, 'actualBoundFamilyMetadataRefresh': True, 'retiredAssetAuthorityClosed': True, 'assetTriggerOrderingPreserved': True}, registration=registration, runtime=realm.runtime,
            assetAuthority=asset_authority, migrations=realm.manifest, hookBodySha256=hashlib.sha256(hook_body.encode()).hexdigest(), installation=installation,
            excluded=['public-email-delivery', 'external-providers', 'browser-tauri-journey', 'other-feature-api-routes', 'oauth-concurrency-guard-qualification', 'hosted-activation', 'hosted-api-key-validation'])
    except BaseException as failure:
        primary = failure
        result['primaryFailure'] = type(failure).__name__
        private_write(realm.stage / 'private-primary-failure.txt', str(failure).encode())
    finally:
        try:
            realm.stop_background()
            result['apiProcessExitVerified'] = True
        except BaseException as failure:
            result['apiProcessExitFailure'] = type(failure).__name__
        for runtime in realm.runtime:
            if runtime['name'] == 'supabase_auth_' + realm.project:
                try:
                    realm.command(['docker', 'logs', runtime['name']])
                except BaseException:
                    result['privateDiagnosticCaptureFailed'] = True
        try:
            realm.__exit__()
            result['cleanupVerified'] = realm.cleanup_verified
        except BaseException as failure:
            result['cleanupFailure'] = type(failure).__name__
        private_write(realm.stage / 'result.json', json.dumps(result, indent=2).encode())
    if primary is not None or not result['cleanupVerified'] or not result.get('apiProcessExitVerified'):
        raise RuntimeError('Owned managed actor qualification failed; private diagnostics: ' + str(realm.stage / 'result.json')) from None
    print('Owned managed actor qualification passed; normalized result: ' + str(realm.stage / 'result.json'))


if __name__ == '__main__':
    main()
