"""Real GoTrue concurrency with synthetic Discord, never external identity proof."""
import base64
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
import hashlib
import json
import re
import secrets
import time
from typing import Callable
import urllib.parse
import urllib.request
import uuid

from auth_oauth_fixture import DiscordFixture, DiscordProfile, oauth_begin, oauth_redirect
from qualification_http import make_opener, object_json, request_json


@dataclass(repr=False)
class Attempt:
    path: str
    body: dict


@dataclass(repr=False)
class Reply:
    status: int
    value: dict
    started: float
    finished: float


@dataclass(repr=False)
class Case:
    email: str
    subject: str
    profile: DiscordProfile
    email_attempt: Attempt
    discord_attempt: Attempt | None


def exact_shape(actual: dict, expected: dict) -> None:
    """Reject incomplete or wrong-source fixture admission, including bool counts."""
    if set(actual) != set(expected) or any(type(actual[key]) is not type(value) or actual[key] != value for key,value in expected.items()):
        raise RuntimeError('Exact fixture admission differs')


def expected_family(actual: str, expected: str) -> None:
    if actual != expected:
        raise RuntimeError('Signed credential selected an unexpected family')


def require_overlap(observed: bool, a: tuple[float,float], b: tuple[float,float]) -> None:
    if observed is not True or max(a[0],b[0]) >= min(a[1],b[1]):
        raise RuntimeError('Required controlled overlap is absent')


def admission_controls() -> list[str]:
    """Exercise the same validators used by real fixture observations."""
    import copy
    valid_flow={'subject':'same','method':'email/signup','provider':'email'}
    valid_pre={'emailIdentities':1,'discordIdentities':1,'sessions':0,'authority':0,'numericValid':True,'profileMatches':True}
    valid_authority={'bindings':1,'grants':1,'sessions':1,'accounts':0}
    controls=[]
    cases=[('separate_subject',valid_flow,'subject','other'),
        ('unsupported_flow_class',valid_flow,'method','oauth'),
        ('missing_verified_discord',valid_pre,'discordIdentities',0),
        ('invalid_numeric_delivery',valid_pre,'numericValid',False),
        ('wrong_external_identity',valid_pre,'profileMatches',False),
        ('missing_binding',valid_authority,'bindings',0),
        ('extra_grant',valid_authority,'grants',2)]
    for name,expected,key,value in cases:
        mutated=copy.deepcopy(expected);mutated[key]=value
        try:exact_shape(mutated,expected)
        except RuntimeError:controls.append(name)
        else:raise RuntimeError('Admission negative control was not detected')
        exact_shape(expected,expected)
    try:expected_family('email_code','discord')
    except RuntimeError:controls.append('wrong_family')
    else:raise RuntimeError('Family negative control was not detected')
    for name,observed,a,b in [('missing_hook_overlap',False,(0.,2.),(1.,3.)),('nonoverlapping_requests',True,(0.,1.),(2.,3.))]:
        try:require_overlap(observed,a,b)
        except RuntimeError:controls.append(name)
        else:raise RuntimeError('Overlap negative control was not detected')
    require_overlap(True,(0.,2.),(1.,3.))
    return controls


def qualify_concurrency(base: str, discord: DiscordFixture, sql: Callable[..., str],
                        validate_token: Callable[[str], dict]) -> list[dict]:
    """Keep every credential private and qualify only admitted, actually run cases."""
    results: list[dict] = []

    def literal(value: str) -> str:
        return "'" + value.replace("'", "''") + "'"

    def query(command: str) -> str:
        return sql("SET statement_timeout='750ms'; " + command, timeout=5)

    def request(attempt: Attempt) -> Reply:
        started = time.monotonic()
        status, value = request_json(make_opener(), urllib.request.Request(base + attempt.path,
            headers={'Content-Type': 'application/json'}, data=json.dumps(attempt.body).encode()))
        return Reply(status, object_json(value), started, time.monotonic())

    def snapshot() -> str:
        tables = ('auth.users', 'auth.identities', 'auth.sessions', 'auth.mfa_amr_claims',
                  'auth.refresh_tokens', 'auth.one_time_tokens', 'auth.audit_log_entries',
                  'auth.flow_state', 'app.command_receipt', 'probe.observation')
        fields = ','.join(literal(table) + ',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM ' + table + ' t)' for table in tables)
        return query("SELECT encode(sha256(convert_to(jsonb_build_object(" + fields + ")::text,'UTF8')),'hex')")

    def code_from_location(location: str) -> str:
        parsed = urllib.parse.urlparse(location)
        values = urllib.parse.parse_qs(parsed.query).get('code', [])
        if len(values) != 1 or not values[0] or urllib.parse.parse_qs(parsed.fragment).get('access_token'):
            raise RuntimeError('Owned callback did not commit exactly one token-free handoff')
        return values[0]

    def mailbox() -> dict:
        value = object_json(json.loads(query("DELETE FROM probe.mailbox RETURNING jsonb_build_object('token',token,'action',action,'hash',token_hash)")))
        if not isinstance(value.get('token'), str) or not re.fullmatch('[0-9]{6}', value['token']) or value.get('action') not in ('signup', 'magiclink'):
            raise RuntimeError('Owned numeric delivery contract differs')
        return value

    def prepare(pause_callback: bool = False) -> Case:
        email = 'concurrency-' + uuid.uuid4().hex + '@example.invalid'
        verifier = secrets.token_urlsafe(48)
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip('=')
        if request(Attempt('/otp', {'email': email, 'create_user': True, 'code_challenge': challenge, 'code_challenge_method': 's256'})).status != 200:
            raise RuntimeError('Owned email confirmation setup failed')
        delivery = mailbox()
        if not isinstance(delivery.get('hash'), str) or not delivery['hash'].startswith('pkce_'):
            raise RuntimeError('Actual email PKCE setup hash is unavailable')
        location = oauth_redirect(base, make_opener(), '/verify?' + urllib.parse.urlencode({'token': delivery['hash'], 'type': delivery['action']}), (302,303))
        unsupported_code = code_from_location(location)
        subject = str(uuid.UUID(query('SELECT id FROM auth.users WHERE email=' + literal(email))))
        setup_flow=object_json(json.loads(query("SELECT jsonb_build_object('subject',user_id,'method',authentication_method,'provider',provider_type) FROM auth.flow_state WHERE auth_code="+literal(unsupported_code))))
        exact_shape(setup_flow,{'subject':subject,'method':'email/signup','provider':'email'})
        before = snapshot()
        unsupported = request(Attempt('/token?grant_type=pkce', {'auth_code': unsupported_code, 'code_verifier': verifier}))
        if unsupported.status < 400 or unsupported.value.get('access_token') or snapshot() != before:
            raise RuntimeError('Unsupported email PKCE class obtained authority or changed canonical state')
        if request(Attempt('/otp', {'email': email, 'create_user': True})).status != 200:
            raise RuntimeError('Owned fresh numeric delivery failed')
        delivery = mailbox()
        attempt = Attempt('/verify', {'email': email, 'token': delivery['token'], 'type': 'email'})
        expected_hash = hashlib.sha224((email + delivery['token']).encode()).hexdigest()
        if query('SELECT recovery_token=' + literal(expected_hash) + ' FROM auth.users WHERE id=' + literal(subject)) != 't':
            raise RuntimeError('Fresh numeric attempt is not the actual pending credential')
        profile = discord.register(email, pause=pause_callback)
        case = Case(email, subject, profile, attempt, None)
        if not pause_callback:
            callback, verifier = oauth_begin(base, make_opener(), True, profile.code)
            code = code_from_location(oauth_redirect(base, make_opener(), callback))
            case.discord_attempt = Attempt('/token?grant_type=pkce', {'auth_code': code, 'code_verifier': verifier})
            shape = object_json(json.loads(query("SELECT jsonb_build_object('subject',user_id,'method',authentication_method,'provider',provider_type) FROM auth.flow_state WHERE auth_code=" + literal(code))))
            if shape != {'subject': subject, 'method': 'oauth', 'provider': 'discord'}:
                raise RuntimeError('Actual Discord handoff does not select the exact shared subject')
        # Recheck the actual pending numeric hash AFTER the Discord callback,
        # so a losing email attempt is not merely an already-invalid credential.
        shape=object_json(json.loads(query("SELECT jsonb_build_object('emailIdentities',(SELECT count(*) FROM auth.identities WHERE user_id="+literal(subject)+" AND provider='email' AND identity_data->'email_verified'='true'::jsonb),'discordIdentities',(SELECT count(*) FROM auth.identities WHERE user_id="+literal(subject)+" AND provider='discord' AND identity_data->'email_verified'='true'::jsonb),'sessions',(SELECT count(*) FROM auth.sessions WHERE user_id="+literal(subject)+"),'authority',(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash("+literal(subject)+")),'numericValid',(SELECT recovery_token="+literal(expected_hash)+" FROM auth.users WHERE id="+literal(subject)+"),'profileMatches',"+("true" if pause_callback else "EXISTS(SELECT 1 FROM auth.identities WHERE user_id="+literal(subject)+" AND provider='discord' AND provider_id="+literal(profile.identity)+")")+")")))
        exact_shape(shape,{'emailIdentities':1,'discordIdentities':0 if pause_callback else 1,'sessions':0,'authority':0,'numericValid':True,'profileMatches':True})
        return case

    def claims(reply: Reply, case: Case, expected_method: str) -> dict:
        if reply.status != 200 or not isinstance(reply.value.get('access_token'), str):
            raise RuntimeError('Expected supported credential did not issue a signed token')
        value = validate_token(reply.value['access_token'])
        if value.get('sub') != case.subject:
            raise RuntimeError('Signed token changed the admitted subject')
        authority = object_json(value.get('spellwake_auth'))
        session = str(uuid.UUID(value['session_id']))
        binding = str(uuid.UUID(authority['bindingId']))
        grant = str(uuid.UUID(authority['sessionGrantId']))
        method = authority['method']
        expected_family(method, expected_method)
        class_name = 'otp' if method == 'email_code' else 'oauth'
        if query("SELECT EXISTS(SELECT 1 FROM app.command_receipt b JOIN app.command_receipt g ON g.target_ref=b.id::text WHERE b.id=" + literal(binding) + " AND g.id=" + literal(grant) + " AND b.principal_key='auth:'||app.auth_uuid_hash(" + literal(case.subject) + ") AND g.principal_key=b.principal_key AND b.scope='auth_binding_v1' AND b.state='accepted' AND g.scope='auth_session_v1' AND g.state='accepted' AND b.result_ref::jsonb->>'method'=" + literal(method) + " AND g.result_ref::jsonb->>'method'=" + literal(method) + " AND g.request_ref=app.auth_uuid_hash(" + literal(session) + ")) AND app.auth_session_is_live(" + literal(case.subject) + ',' + literal(session) + ',' + literal(class_name) + ')') != 't':
            raise RuntimeError('Signed session does not select exact live canonical receipts')
        return value

    def check_authority(case: Case, accepted: list[dict]) -> dict:
        if not accepted:
            raise RuntimeError('No supported credential won selection')
        methods = {item['spellwake_auth']['method'] for item in accepted}
        bindings = {item['spellwake_auth']['bindingId'] for item in accepted}
        sessions = {item['session_id'] for item in accepted}
        grants = {item['spellwake_auth']['sessionGrantId'] for item in accepted}
        if len(methods) != 1 or len(bindings) != 1 or len(grants) != len(sessions):
            raise RuntimeError('Returned tokens violate canonical family or session grant identity')
        state = object_json(json.loads(query("SELECT jsonb_build_object('bindings',(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash(" + literal(case.subject) + ") AND scope='auth_binding_v1' AND state='accepted'),'grants',(SELECT count(*) FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash(" + literal(case.subject) + ") AND scope='auth_session_v1' AND state='accepted'),'sessions',(SELECT count(*) FROM auth.sessions WHERE user_id=" + literal(case.subject) + "),'accounts',(SELECT count(*) FROM app.account WHERE auth_subject=" + literal(case.subject) + "))")))
        exact_shape(state,{'bindings':1, 'grants':len(sessions), 'sessions':len(sessions), 'accounts':0})
        return {'family': next(iter(methods)), 'bindings':1, 'sessions':len(sessions), 'grants':len(grants), 'accounts':0}

    def liveness(case: Case, method: str) -> dict:
        if method == 'email_code':
            if request(Attempt('/otp', {'email':case.email,'create_user':True})).status != 200:
                raise RuntimeError('Original email method delivery failed')
            reply = request(Attempt('/verify', {'email':case.email,'type':'email','token':mailbox()['token']}))
            return claims(reply, case, method)
        callback, verifier = oauth_begin(base, make_opener(), True, case.profile.code)
        code = code_from_location(oauth_redirect(base, make_opener(), callback))
        return claims(request(Attempt('/token?grant_type=pkce',{'auth_code':code,'code_verifier':verifier})),case, method)

    def reject_unchanged(attempt: Attempt) -> int:
        before = snapshot()
        reply = request(attempt)
        if reply.status < 400 or reply.value.get('access_token') or snapshot() != before:
            raise RuntimeError('Losing credential changed canonical state or obtained authority')
        return reply.status

    economy_before=query("SELECT jsonb_build_object('accounts',(SELECT count(*) FROM app.account),'signupOperations',(SELECT count(*) FROM app.ledger_operation WHERE source_scope='signup'),'inventory',(SELECT count(*) FROM app.inventory_asset))")
    for winner in ('email_code','discord'):
        case = prepare()
        if case.discord_attempt is None:
            raise RuntimeError('Discord attempt missing')
        first, second = (case.email_attempt,case.discord_attempt) if winner=='email_code' else (case.discord_attempt,case.email_attempt)
        accepted=[claims(request(first),case,winner)]
        if accepted[0]['spellwake_auth']['method'] != winner:
            raise RuntimeError('Sequential first credential selected the wrong family')
        loser_status=reject_unchanged(second)
        accepted.append(liveness(case,winner))
        results.append({'case':'first_' + winner, 'passed':True, 'loserStatus':loser_status, 'state':check_authority(case,accepted), 'unsupportedEmailPkceDenied':True})

    def overlap(first: Attempt, second: Attempt) -> tuple[Reply,Reply,dict]:
        query('UPDATE probe.control SET delay_milliseconds=400')
        try:
            with ThreadPoolExecutor(max_workers=2) as workers:
                left=workers.submit(request,first)
                deadline=time.monotonic()+2
                observed=False
                while time.monotonic()<deadline and not left.done():
                    backend=int(query("SELECT coalesce((SELECT pid FROM pg_stat_activity WHERE datname=current_database() AND usename='supabase_auth_admin' AND state='active' AND wait_event='PgSleep' AND query LIKE '%probe%' AND query LIKE '%token_hook%' ORDER BY pid LIMIT 1),0)"))
                    observed=backend>0
                    if observed:
                        break
                if not observed:
                    raise RuntimeError('No actual uncommitted hook overlap was observed')
                right=workers.submit(request,second)
                blocking_observed=False
                while not left.done() and time.monotonic()<deadline:
                    blocking_observed=query("SELECT EXISTS(SELECT 1 FROM pg_stat_activity waiter WHERE waiter.datname=current_database() AND waiter.usename='supabase_auth_admin' AND waiter.state='active' AND waiter.wait_event_type='Lock' AND "+str(backend)+"=ANY(pg_blocking_pids(waiter.pid)))")=='t'
                    if blocking_observed:
                        break
                a,b=left.result(timeout=12),right.result(timeout=12)
            require_overlap(observed,(a.started,a.finished),(b.started,b.finished))
            return a,b,{'uncommittedHookObserved':True,'requestsOverlapped':True,'directBackendBlockingObserved':blocking_observed,'specificLockClaimed':False}
        finally:
            query('UPDATE probe.control SET delay_milliseconds=0')

    for scenario in ('email_first','discord_first','duplicate'):
        duplicate=scenario=='duplicate'
        case=prepare()
        if case.discord_attempt is None:
            raise RuntimeError('Discord attempt missing')
        # Duplicate means one identical Discord PKCE code/verifier. The mixed
        # case deliberately makes numeric email the controlled first contender.
        first=case.email_attempt if scenario=='email_first' else case.discord_attempt
        second=case.email_attempt if scenario=='discord_first' else case.discord_attempt
        a,b,evidence=overlap(first,second)
        accepted=[claims(reply,case,method) for reply,method in [(a,'email_code' if scenario=='email_first' else 'discord'),(b,'email_code' if scenario=='discord_first' else 'discord')] if reply.status==200]
        if any(reply.status<400 and reply.status!=200 for reply in (a,b)):
            raise RuntimeError('Unexpected overlapping credential response')
        state=check_authority(case,accepted)
        if not duplicate and len(accepted)!=1:
            raise RuntimeError('Competing families both obtained authority')
        state_before_liveness=dict(state)
        accepted.append(liveness(case,state['family']))
        check_authority(case,accepted)
        results.append({'case':'duplicate_discord_pkce' if duplicate else 'overlapping_families_' + scenario,'passed':True,
            'statuses':[a.status,b.status],'state':state_before_liveness,'overlap':evidence,'unsupportedEmailPkceDenied':True})

    case=prepare(pause_callback=True)
    callback,verifier=oauth_begin(base,make_opener(),True,case.profile.code)
    with ThreadPoolExecutor(max_workers=1) as workers:
        pending=workers.submit(oauth_redirect,base,make_opener(),callback)
        try:
            if not case.profile.entered.wait(timeout=3):
                raise RuntimeError('Callback did not reach the actual provider response barrier')
            accepted=[claims(request(case.email_attempt),case,'email_code')]
            check_authority(case,accepted)
        finally:
            case.profile.release.set()
        location=pending.result(timeout=12)
    parsed=urllib.parse.urlparse(location)
    codes=urllib.parse.parse_qs(parsed.query).get('code',[])
    if urllib.parse.parse_qs(parsed.fragment).get('access_token'):
        raise RuntimeError('Overlapping alternate callback issued a token')
    if codes:
        if len(codes)!=1:
            raise RuntimeError('Overlapping callback returned ambiguous flow')
        reject_unchanged(Attempt('/token?grant_type=pkce',{'auth_code':codes[0],'code_verifier':verifier}))
    elif not (urllib.parse.parse_qs(parsed.fragment).get('error') or urllib.parse.parse_qs(parsed.query).get('error')):
        raise RuntimeError('Overlapping callback did not report a checked rejection')
    if query("SELECT count(*) FROM auth.identities WHERE provider_id="+literal(case.profile.identity)+" AND provider='discord'")!='0':
        raise RuntimeError('Paused-before-persistence callback added a bound alternate identity')
    accepted.append(liveness(case,'email_code'))
    results.append({'case':'callback_before_identity_persistence_overlaps_email_binding','passed':True,
        'providerBarrierObserved':True,'state':check_authority(case,accepted),'unsupportedEmailPkceDenied':True})
    economy_after=query("SELECT jsonb_build_object('accounts',(SELECT count(*) FROM app.account),'signupOperations',(SELECT count(*) FROM app.ledger_operation WHERE source_scope='signup'),'inventory',(SELECT count(*) FROM app.inventory_asset))")
    if economy_after!=economy_before:
        raise RuntimeError('Auth concurrency changed application issuance or accounts')
    controls=admission_controls()
    results.append({'case':'fixture_admission_negative_controls','passed':True,'controls':controls})
    return results
