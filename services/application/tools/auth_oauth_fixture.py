"""Local Discord stand-in for pinned GoTrue provenance tests, never provider proof."""
import base64
import hashlib
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import secrets
import threading
from typing import Callable
import urllib.error
import urllib.parse
import urllib.request


@dataclass(repr=False)
class DiscordProfile:
    """Private stand-in credentials and bounded user-info response control."""
    email: str
    identity: str
    code: str = field(default_factory=lambda: secrets.token_urlsafe(32))
    access_token: str = field(default_factory=lambda: secrets.token_urlsafe(32))
    entered: threading.Event = field(default_factory=threading.Event)
    release: threading.Event = field(default_factory=threading.Event)


class DiscordFixture:
    """Serve only synthetic token/user replies and stop our exact server thread."""

    def register(self, email: str, pause: bool = False) -> DiscordProfile:
        """Register only in-process test identities; never contact an external provider."""
        profile = DiscordProfile(email, str(secrets.randbelow(9 * 10**17) + 10**17))
        if not pause:
            profile.release.set()
        with self.lock:
            self.profiles[profile.code] = profile
            self.tokens[profile.access_token] = profile
        return profile

    def __enter__(self) -> 'DiscordFixture':
        self.lock = threading.Lock()
        self.profiles: dict[str, DiscordProfile] = {}
        self.tokens: dict[str, DiscordProfile] = {}
        default = DiscordProfile('discord-probe@example.invalid', '123456789012345678',
                                 'fixture-code', 'fixture-access-only')
        default.release.set()
        self.profiles[default.code] = default
        self.tokens[default.access_token] = default
        fixture = self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_: object) -> None:
                pass

            def reply(self, value: dict) -> None:
                raw = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_POST(self) -> None:
                if self.path != '/api/oauth2/token':
                    self.send_error(404)
                    return
                length = int(self.headers.get('Content-Length', '0'))
                if length < 1 or length > 4096:
                    self.send_error(400)
                    return
                fields = urllib.parse.parse_qs(self.rfile.read(length).decode())
                values = fields.get('code', [])
                with fixture.lock:
                    profile = fixture.profiles.get(values[0]) if len(values) == 1 else None
                if profile is None:
                    self.send_error(400)
                    return
                self.reply({'access_token': profile.access_token, 'token_type': 'Bearer', 'expires_in': 3600})

            def do_GET(self) -> None:
                bearer = self.headers.get('Authorization', '').removeprefix('Bearer ')
                with fixture.lock:
                    profile = fixture.tokens.get(bearer)
                if self.path != '/api/users/@me' or profile is None:
                    self.send_error(404)
                    return
                profile.entered.set()
                if not profile.release.wait(timeout=8):
                    self.send_error(504)
                    return
                self.reply({'id': profile.identity, 'username': 'fixture-discord',
                            'global_name': 'Fixture', 'discriminator': '1234',
                            'email': profile.email, 'verified': True, 'avatar': ''})

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = 'http://host.docker.internal:' + str(self.server.server_address[1])
        return self

    def __exit__(self, *_: object) -> None:
        with self.lock:
            for profile in self.profiles.values():
                profile.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError('Owned Discord fixture thread did not stop')


def oauth_redirect(base: str, opener: urllib.request.OpenerDirector, path: str, statuses: tuple[int, ...] = (302,)) -> str:
    try:
        response = opener.open(base + path, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        if response.status not in statuses or not response.headers.get('Location'):
            raise RuntimeError(f'Expected owned Auth redirect, got HTTP {response.status}')
        return response.headers['Location']

def oauth_begin(base: str, opener: urllib.request.OpenerDirector, pkce: bool, code: str = "fixture-code") -> tuple[str, str]:
    verifier = secrets.token_urlsafe(48)
    query = {'provider': 'discord'}
    if pkce:
        query.update(code_challenge=base64.urlsafe_b64encode(
            hashlib.sha256(verifier.encode()).digest()).decode().rstrip('='), code_challenge_method='s256')
    location = urllib.parse.urlparse(oauth_redirect(base, opener, '/authorize?' + urllib.parse.urlencode(query)))
    state = urllib.parse.parse_qs(location.query).get('state', [])
    if location.hostname != 'host.docker.internal' or location.path != '/api/oauth2/authorize' or len(state) != 1:
        raise RuntimeError('Auth did not select the local synthetic provider')
    callback = '/callback?' + urllib.parse.urlencode({'state': state[0], 'code': code})
    return callback, verifier


def qualify_oauth(
    base: str, opener: urllib.request.OpenerDirector,
    request: Callable[..., tuple[int, dict]], sql: Callable[[str], str],
    snapshot: Callable[[], str],
    verify_access_token: Callable[[str], None] | None = None,
    expected_implicit_action: str = 'user_signedup',
) -> list[dict]:
    """Exercise actual implicit and PKCE callbacks with synthetic provider replies."""
    def redirect(path: str) -> str:
        return oauth_redirect(base, opener, path)

    def begin(pkce: bool) -> tuple[str, str]:
        return oauth_begin(base, opener, pkce)

    def flow_snapshot() -> str:
        return sql("SELECT encode(sha256(convert_to(coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),"
                   "'[]'::jsonb)::text,'UTF8')),'hex') FROM auth.flow_state t")

    count = int(sql('SELECT count(*) FROM probe.observation'))
    callback, _ = begin(False)
    fragment = urllib.parse.parse_qs(urllib.parse.urlparse(redirect(callback)).fragment)
    if not fragment.get('access_token') or fragment.get('error'):
        raise RuntimeError('Implicit synthetic-provider callback failed')
    if verify_access_token is not None:
        verify_access_token(fragment['access_token'][0])
    if int(sql('SELECT count(*) FROM probe.observation')) != count + 1:
        raise RuntimeError('Implicit callback did not invoke token hook exactly once')
    callback, verifier = begin(True)
    location = urllib.parse.urlparse(redirect(callback))
    codes = urllib.parse.parse_qs(location.query).get('code', [])
    if len(codes) != 1 or int(sql('SELECT count(*) FROM probe.observation')) != count + 1:
        raise RuntimeError('PKCE callback did not commit an unredeemed flow without issuance')
    persisted = sql("SELECT count(*) FROM auth.flow_state f JOIN auth.users u ON u.id=f.user_id "
                    "JOIN auth.identities i ON i.user_id=u.id WHERE f.provider_type='discord' "
                    "AND f.authentication_method='oauth' AND f.auth_code IS NOT NULL "
                    "AND u.email_confirmed_at IS NOT NULL AND i.provider='discord' "
                    "AND i.identity_data->>'email_verified'='true'")
    if persisted != '1':
        raise RuntimeError('PKCE callback did not persist the verified identity and pending flow')
    body = {'auth_code': codes[0], 'code_verifier': 'wrong-' + verifier}
    before = snapshot()
    flow_before = flow_snapshot()
    status, _ = request('/token?grant_type=pkce', body)
    if status < 400 or snapshot() != before or flow_snapshot() != flow_before:
        raise RuntimeError('Wrong PKCE verifier reached successful hook')
    body['code_verifier'] = verifier
    sql('UPDATE probe.control SET reject_token=true;')
    status, _ = request('/token?grant_type=pkce', body)
    flow_after = flow_snapshot()
    if status < 400 or snapshot() != before or flow_after != flow_before:
        raise RuntimeError('Rejected PKCE issuance committed observation')
    sql('UPDATE probe.control SET reject_token=false;')
    status, token = request('/token?grant_type=pkce', body)
    if status != 200 or not token.get('access_token'):
        raise RuntimeError('Same PKCE code did not succeed after hook rejection')
    observations = json.loads(sql('SELECT jsonb_agg(value ORDER BY id) FROM probe.observation'))[-2:]
    if [(item['action'], item['method'], item['provider'], item['providerType']) for item in observations] != [
            (expected_implicit_action, 'oauth', 'discord', None), ('login', 'oauth', None, 'discord')]:
        raise RuntimeError('Actual OAuth producer labels differ')
    if any(item['amr'] != ['oauth'] for item in observations):
        raise RuntimeError('OAuth session AMR differs')
    if observations[0]['sessionHash'] == observations[1]['sessionHash'] or observations[0]['xact'] == observations[1]['xact']:
        raise RuntimeError('Separate OAuth issuances share session/transaction unexpectedly')
    return [
        {'case': 'actual_implicit_callback_with_synthetic_discord', 'passed': True},
        {'case': 'actual_pkce_callback_no_token_then_verified_exchange', 'passed': True},
        {'case': 'pkce_hook_rejection_and_same_code_retry', 'passed': True},
    ]
