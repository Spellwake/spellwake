"""Actual vendor HTTP bearer/RLS qualification, not the Spellwake Rust API."""
import base64
from contextlib import ExitStack
import hashlib
import hmac
import json
from pathlib import Path
import secrets
import time
from typing import Callable
import urllib.request
import uuid

from disposable_http_service import DisposableHttpService
from qualification_http import request_json


def qualify_postgrest(
    stack: ExitStack, stage: Path, database: str, port: int, secret: str,
    tokens: list[str], claims: list[dict], sql: Callable[[str], str],
    opener: urllib.request.OpenerDirector,
) -> tuple[dict, Callable[[], None], DisposableHttpService]:
    """Keep original bearer strings only in memory across native revocation."""
    password = secrets.token_hex(32)
    sql("CREATE ROLE authenticator LOGIN NOINHERIT NOCREATEDB NOCREATEROLE NOSUPERUSER "
        "PASSWORD '" + password + "'; GRANT anon,authenticated,service_role TO authenticator;")
    rest_stage = stage / 'postgrest'
    rest_stage.mkdir(mode=0o700)
    config = {
        'PGRST_DB_URI': f'postgres://authenticator:{password}@host.docker.internal:{port}/{database}?sslmode=disable',
        'PGRST_DB_ANON_ROLE': 'anon', 'PGRST_DB_SCHEMAS': 'app', 'PGRST_DB_EXTRA_SEARCH_PATH': 'public',
        'PGRST_DB_CONFIG': 'false', 'PGRST_DB_POOL': '2',
        'PGRST_JWT_SECRET': secret, 'PGRST_JWT_AUD': 'authenticated',
        'PGRST_SERVER_HOST': '0.0.0.0', 'PGRST_SERVER_PORT': '3000', 'PGRST_LOG_LEVEL': 'error',
    }
    service = stack.enter_context(DisposableHttpService('postgrest', config, rest_stage))

    def request(path: str, token: str | None, body: dict | None = None) -> tuple[int, object]:
        headers = {'Content-Type': 'application/json'}
        if token is not None:
            headers['Authorization'] = 'Bearer ' + token
        command = urllib.request.Request(service.base + path, headers=headers,
                                         data=None if body is None else json.dumps(body).encode())
        return request_json(opener, command)

    def sign(value: dict) -> str:
        def encode(item: object) -> bytes:
            return base64.urlsafe_b64encode(json.dumps(item, separators=(',', ':')).encode()).rstrip(b'=')
        message = encode({'alg': 'HS256', 'typ': 'JWT'}) + b'.' + encode(value)
        return (message + b'.' + base64.urlsafe_b64encode(
            hmac.new(secret.encode(), message, hashlib.sha256).digest()).rstrip(b'=')).decode()

    account = str(uuid.UUID(sql("SELECT id FROM app.account WHERE auth_subject='" + str(uuid.UUID(claims[0]['sub'])) + "'")))
    deadline = time.monotonic() + 30
    while True:
        try:
            status, value = request('/rpc/current_account_id', tokens[0], {})
            if status == 200:
                if value != account:
                    raise RuntimeError('Actual bearer did not select its registered account')
                break
        except (OSError, ValueError):
            pass
        if time.monotonic() >= deadline:
            raise RuntimeError('Owned PostgREST readiness deadline exceeded')
        time.sleep(0.2)

    denied_session = {'status': 'fail', 'data': {'session': 'unauthorized'}}

    def expected_session(index: int, state: str, account_id: str | None) -> dict:
        return {'status': 'success', 'data': {'state': state, 'method': claims[index]['spellwake_auth']['method'],
                'subjectId': claims[index]['sub'], 'sessionId': claims[index]['session_id'], 'accountId': account_id}}

    if sql("SELECT count(*) FROM app.account WHERE auth_subject='" + str(uuid.UUID(claims[5]['sub'])) + "'") != '0':
        raise RuntimeError('HTTP setup subject unexpectedly registered')
    if request('/rpc/read_current_session', tokens[5], {}) != (200, expected_session(5, 'needs_registration', None)):
        raise RuntimeError('Actual unregistered bearer did not select setup state')
    if request('/rpc/read_current_session', tokens[0], {}) != (200, expected_session(0, 'active', account)):
        raise RuntimeError('Actual registered bearer did not select active state')
    if request('/rpc/read_current_session', None, {})[0] not in (401, 403, 404):
        raise RuntimeError('Anonymous session read unexpectedly accessible')
    if request('/rpc/read_current_session', tokens[0], {'p_subject': claims[5]['sub']})[0] != 404:
        raise RuntimeError('Session read accepted caller-selected identity')

    def wallet(token: str) -> tuple[int, object]:
        return request('/coin_balance?select=quantity::text', token)

    if wallet(tokens[0]) != (200, [{'quantity': '14500'}]):
        raise RuntimeError('Actual HTTP owner wallet differs')
    if wallet(tokens[5]) != (200, []):
        raise RuntimeError('Other real subject read owner inventory')
    status, _ = request('/coin_balance?select=quantity::text', None)
    if status not in (401, 403, 404):
        raise RuntimeError('Unauthenticated HTTP inventory unexpectedly accessible')
    parts = tokens[0].split('.')
    signature = bytearray(base64.urlsafe_b64decode(parts[2] + '=' * (-len(parts[2]) % 4)))
    signature[0] ^= 1
    forged = parts[0] + '.' + parts[1] + '.' + base64.urlsafe_b64encode(signature).decode().rstrip('=')
    if wallet(forged)[0] != 401:
        raise RuntimeError('PostgREST did not reject the altered signature')
    for field, value in [('aud', 'other-project'), ('exp', int(time.time()) - 120)]:
        invalid = dict(claims[0]); invalid[field] = value
        if wallet(sign(invalid))[0] != 401:
            raise RuntimeError('PostgREST did not reject wrong audience or expiry')
    legacy = json.loads(json.dumps(claims[0]))
    removed_authority = legacy.pop('spellwake_auth')
    legacy.setdefault('user_metadata', {})['spellwake_auth'] = removed_authority
    if wallet(sign(legacy)) != (200, []):
        raise RuntimeError('Validly signed pre-hook token read owner inventory')
    if request('/rpc/read_current_session', sign(legacy), {}) != (200, denied_session):
        raise RuntimeError('Metadata-only authority obtained current session')
    wrong = json.loads(json.dumps(claims[0]))
    wrong['spellwake_auth']['bindingId'] = str(uuid.uuid4())
    if wallet(sign(wrong)) != (200, []):
        raise RuntimeError('Validly signed mismatched grant read owner inventory')
    if request('/rpc/read_current_session', sign(wrong), {}) != (200, denied_session):
        raise RuntimeError('Mismatched root grant obtained current session')
    for path, body in [
        ('/rpc/read_current_auth_context', {}),
        ('/account', {'username': 'forbidden_http_write'}),
        ('/rpc/custom_access_token_hook', {'p_event': {}}),
        ('/rpc/read_auth_session_authority', {'p_subject': claims[0]['sub'], 'p_session': claims[0]['session_id']}),
        ('/rpc/register_account', {'p_subject': claims[0]['sub'], 'p_session': claims[0]['session_id'],
                                  'p_username': 'forbidden_http_write', 'p_back': str(uuid.uuid4()),
                                  'p_sleeve': str(uuid.uuid4()), 'p_dice': str(uuid.uuid4())}),
    ]:
        if request(path, tokens[0], body)[0] not in (403, 404):
            raise RuntimeError('Authenticated HTTP bypassed a service-only mutation')

    report = {'image': service.image, 'imageId': service.image_id, 'ownerWalletCoins': '14500',
              'otherSubjectDenied': True, 'anonymousDenied': True, 'badSignatureDenied': True,
              'wrongAudienceAndExpiredDenied': True, 'legacyAndWrongAuthorityRowsHidden': True,
              'rawAndServiceOnlyWritesDenied': True, 'separateSubjectsSetupAndActiveRead': True,
              'sessionIdentityArgumentsDenied': True, 'metadataOnlySessionDenied': True}

    def after_revocation() -> None:
        if wallet(tokens[0]) != (200, []) or request('/rpc/current_account_id', tokens[0], {}) != (200, None):
            raise RuntimeError('Revoked but unexpired bearer retained HTTP account access')
        if request('/rpc/read_current_session', tokens[0], {}) != (200, denied_session):
            raise RuntimeError('Revoked but unexpired bearer retained current-session authority')
        if request('/rpc/read_current_session', tokens[3], {}) != (200, expected_session(3, 'active', account)):
            raise RuntimeError('Other live session lost current-session authority')
        if wallet(tokens[3]) != (200, [{'quantity': '14500'}]):
            raise RuntimeError('Revoking one session incorrectly revoked another live session')
        report['revokedBearerRowsHidden'] = True
        report['otherSessionStillAdmitted'] = True

    return report, after_revocation, service
