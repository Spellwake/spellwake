"""Qualify native artifact bytes against an owned disposable real Storage gateway."""
from pathlib import Path
import argparse
import base64
import hashlib
import json
import os
import time
import urllib.request
import urllib.parse
import uuid

from auth_admission_fixture import qualify_admission
from disposable_supabase import DisposableSupabase, QualificationCommandError, ROOT, private_write
from qualification_http import make_opener, request_json, object_json

TEST = 'native_game::artifacts::tests::owned_native_input_retention_and_reconstruction'
REPO = ROOT.parent.parent
RESULT = REPO / '.quzzar-system-design/application-architecture/reviews/api-native-artifacts-connected-v1.json'


def main() -> None:
    """Keep primary, object cleanup and environment cleanup independently observable."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=('artifacts', 'initial-source', 'checkpoint', 'selected-checkpoint', 'source-reader', 'results', 'delivery'), default='artifacts')
    parser.add_argument('--delivery-case', choices=('lifecycle','expiry','logout','presentation','deletion-before','deletion-during','deletion-after','sources','handoff','failures','history-authority','replay-lost-ack','publication','history-expiry','history-errors','history-error-expiry','read-lifecycle','read-unavailable','read-cancellation','archive-authority','read-account','read-account-expiry','publication-budget','mixed-replay','controller'), default='lifecycle')
    parser.add_argument('--archive-variant', choices=('completed-own','completed-full','incident-own'), default='completed-own')
    parser.add_argument('--publication-case', choices=('completed','source','logout','stream','incident'), default='completed')
    parser.add_argument('--read-origin', choices=('active','completed','incident'), default='active')
    parser.add_argument('--lifecycle-action', choices=('pending','requester','peer'), default='pending')
    parser.add_argument('--structure', choices=('bo1','bo3'), default='bo1')
    parser.add_argument('--attempt', type=int, default=1)
    parser.add_argument('--result-turns', type=int, choices=(0, 3), default=3)
    parser.add_argument('--unsupported-work', action='store_true', help='Reach a genuine unsupported native boundary in results mode')
    parser.add_argument('--rule-errors', action='store_true', help='Reach source-bound native target/payment errors')
    parser.add_argument('--no-contest', action='store_true', help='End the results game by an actual unrecoverable incident')
    arguments = parser.parse_args()
    if (arguments.read_origin != 'active' or arguments.lifecycle_action != 'pending') and arguments.delivery_case != 'read-lifecycle':
        parser.error('read origin/lifecycle action requires read-lifecycle mode')
    if arguments.archive_variant != 'completed-own' and arguments.delivery_case != 'archive-authority':
        parser.error('archive variant requires archive-authority case')
    if arguments.delivery_case == 'controller' and arguments.structure != 'bo3':
        parser.error('controller consumer scenario requires first-game BO3 concession')
    if arguments.delivery_case == 'mixed-replay' and arguments.structure != 'bo1':
        parser.error('mixed replay requires BO1 fixture')
    if arguments.delivery_case == 'archive-authority' and arguments.structure != 'bo1':
        parser.error('archive authority requires selected BO1 fixture')
    if arguments.delivery_case == 'read-lifecycle' and (arguments.structure != 'bo1' or arguments.read_origin == 'active' and arguments.lifecycle_action != 'pending'):
        parser.error('lifecycle retirement requires the selected terminal two-human BO1 fixture')
    if arguments.publication_case != 'completed' and arguments.delivery_case != 'publication':
        parser.error('publication-case requires delivery publication case')
    if arguments.delivery_case != 'lifecycle' and arguments.mode != 'delivery':
        parser.error('delivery-case requires delivery mode')
    if arguments.rule_errors and (arguments.mode != 'results' or arguments.unsupported_work):
        parser.error('rule-errors requires results mode without unsupported-work')
    if arguments.unsupported_work and arguments.mode != 'results':
        parser.error('unsupported-work requires results mode')
    if not 1 <= arguments.attempt <= 99:
        parser.error('attempt must be between 1 and 99')
    mode = arguments.mode
    connected_mode = mode in ('source-reader', 'results', 'delivery')
    if arguments.structure == 'bo3' and not connected_mode:
        parser.error('BO3 structure requires an actual connected source/result mode')
    initial = mode != 'artifacts'
    result_path = RESULT if not initial else RESULT.with_name('api-native-' + mode + ('-bo3' if arguments.structure == 'bo3' else '') + '-connected-v1.json')
    if arguments.unsupported_work:
        result_path = result_path.with_name('api-native-unsupported' + ('-bo3' if arguments.structure == 'bo3' else '') + '-connected-v1.json')
    if arguments.no_contest:
        result_path = result_path.with_name('api-native-no-contest' + ('-bo3' if arguments.structure == 'bo3' else '') + '-connected-v1.json')
    if arguments.rule_errors:
        result_path = result_path.with_name('api-native-rule-errors' + ('-bo3' if arguments.structure == 'bo3' else '') + '-connected-v1.json')
    if mode == 'delivery' and arguments.delivery_case != 'lifecycle':
        result_path = result_path.with_name('api-native-delivery-' + arguments.delivery_case + ('-bo3' if arguments.structure == 'bo3' else '') + '-connected-v1.json')
    if mode == 'delivery' and arguments.delivery_case == 'publication' and arguments.publication_case != 'completed':
        result_path = result_path.with_name(result_path.name.replace('-connected-v1', '-' + arguments.publication_case + '-connected-v1'))
    if mode == 'delivery' and arguments.delivery_case == 'read-lifecycle':
        result_path = result_path.with_name('api-native-delivery-lifecycle-' + arguments.read_origin + '-' + arguments.lifecycle_action + '-connected-v1.json')
    if mode == 'delivery' and arguments.delivery_case == 'archive-authority':
        result_path = result_path.with_name('api-native-delivery-archive-authority-' + arguments.archive_variant + '-connected-v1.json')
    if arguments.attempt != 1:
        result_path = result_path.with_name(result_path.stem.removesuffix('v1') + 'v' + str(arguments.attempt) + '.json')
    test = TEST if not initial else ('native_game::initial_source::tests::owned_native_initial_source_and_replay' if mode == 'initial-source' else 'native_game::initial_source::tests::owned_native_checkpoint_chain_recovery')
    if connected_mode:
        test = 'native_game::source_reader::connected::owned_native_source_reader_reconstructs_original_and_advanced_checkpoint'
    elif mode == 'selected-checkpoint':
        test = 'native_game::initial_source::tests::selected::owned_native_selected_checkpoint_chain_recovery'
    if result_path.exists():
        raise RuntimeError('Refusing to overwrite retained connected evidence')
    vocabulary = Path(os.environ['SPELLWAKE_CREATURE_VOCABULARY_INPUT']).resolve(strict=True)
    source_environment = {}
    if initial:
        source_environment = {name: str(Path(os.environ[name]).resolve(strict=True)) for name in (
            'SPELLWAKE_CATALOGUE_METADATA', 'SPELLWAKE_CATALOGUE_INPUT')}
    short_expiry = mode == 'delivery' and arguments.delivery_case in ('expiry','handoff','history-expiry','history-error-expiry','archive-authority','read-account-expiry')
    realm = DisposableSupabase(storage=True, jwt_expiry_seconds=30 if short_expiry else None)
    result: dict = {'scope': 'owned-local-native-' + mode + '-private-storage-actual-auth', 'passed': False,
                    'objectsCleanupVerified': False, 'bucketCleanupVerified': False,
                    'environmentCleanupVerified': False, 'sourcePinsStable': False}
    paths = list((ROOT / 'src').rglob('*.rs')) + list((ROOT / 'supabase/migrations').glob('*.sql'))
    paths += list((REPO / 'crates/engine/src').rglob('*.rs')) + list((REPO / 'crates/contracts/src').rglob('*.rs'))
    paths += [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', vocabulary, Path(__file__),
              ROOT / 'tools/disposable_supabase.py', ROOT / 'tools/qualification_http.py',
              ROOT / 'tools/auth_admission_fixture.py', ROOT / 'tools/loopback_docker.py']
    paths += [ROOT / 'build.rs'] + list((ROOT / 'build_support').rglob('*.rs'))
    # The library-test image embeds these fixed wire vectors even when the owned
    # selector does not execute their separate compatibility/closure assertions.
    paths += [ROOT / 'tests/fixtures/native-initial-v1-wire-golden.json',
              REPO / 'packages/application-contracts/fixtures/player-wire.json',
              *list((ROOT / 'tests/support').rglob('*.rs'))]
    for crate in ('card_programs', 'content'):
        paths += list((REPO / 'crates' / crate / 'src').rglob('*.rs'))
        paths += [REPO / 'crates' / crate / 'Cargo.toml']
    paths += [Path(path) for path in source_environment.values()]
    if connected_mode:
        paths += [ROOT / 'tools/native_source_reader_acceptance.py', REPO / 'packages/application-contracts/tools/validate-native-view-wire.ts', REPO / 'packages/application-contracts/package.json', REPO / 'bun.lock', REPO / 'package.json']
        paths += list((REPO / 'packages/application-contracts/src').rglob('*.ts'))
    paths += [REPO / 'Cargo.toml', REPO / 'rust-toolchain.toml', REPO / 'crates/engine/Cargo.toml', REPO / 'crates/contracts/Cargo.toml']
    config_paths = (REPO / '.cargo/config', REPO / '.cargo/config.toml', ROOT / '.cargo/config', ROOT / '.cargo/config.toml')
    config_presence = {str(path): path.exists() for path in config_paths}
    paths += [path for path in config_paths if path.exists()]
    watched_roots = [ROOT / 'src', ROOT / 'build_support', ROOT / 'tests/support'] + [REPO / 'crates' / name / 'src' for name in ('engine', 'contracts', 'card_programs', 'content')]
    membership = {str(tree): sorted(str(path.relative_to(tree)) for path in tree.rglob('*') if path.is_file()) for tree in watched_roots}
    pins = {str(path.resolve()): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    bucket = 'native-owned-' + uuid.uuid4().hex
    bucket_attempted = False
    primary: BaseException | None = None
    try:
        realm.command(['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--lib', '--no-run'], timeout=240)
        realm.__enter__()
        base = realm.status['API_URL']
        public = realm.status['PUBLISHABLE_KEY']
        secret = realm.status['SECRET_KEY']
        if not base.startswith('http://127.0.0.1:') or not secret.startswith('sb_secret_'):
            raise RuntimeError('Owned server configuration differs')
        opener = make_opener()

        def request(path: str, *, body: dict | None = None, admin: bool = True,
                    method: str | None = None) -> tuple[int, object]:
            if not path.startswith(('/auth/v1/', '/storage/v1/')) or '..' in path:
                raise RuntimeError('Unexpected owned fixture path')
            return request_json(opener, urllib.request.Request(base + path,
                headers={'apikey': secret if admin else public, 'Content-Type': 'application/json'},
                method=method, data=None if body is None else json.dumps(body).encode()), timeout=30)

        refresh_tokens: dict[str,str] = {}
        def signed_credential(status: int, signed: object, email: str) -> tuple[str, dict]:
            """Validate genuine GoTrue output; retain refresh material only privately."""
            signed = object_json(signed)
            if status != 200 or not isinstance(signed.get('access_token'), str):
                raise RuntimeError('Owned GoTrue credential issuance failed')
            bearer = signed['access_token']
            parts = bearer.split('.')
            if len(parts) != 3:
                raise RuntimeError('Owned caller token encoding differs')
            claims = object_json(json.loads(base64.urlsafe_b64decode(parts[1] + '=' * (-len(parts[1]) % 4))))
            if claims.get('role') != 'authenticated' or claims.get('iss') != base + '/auth/v1' or claims.get('exp', 0) <= time.time():
                raise RuntimeError('Owned ordinary caller role/issuer/expiry differs')
            if short_expiry and (type(claims.get('iat')) is not int or type(claims.get('exp')) is not int or claims['exp']-claims['iat'] != 30):
                raise RuntimeError('Real issued lifetime differs from owned short-life configuration')
            refresh = signed.get('refresh_token')
            if short_expiry:
                if not isinstance(refresh,str) or not refresh:
                    raise RuntimeError('Owned GoTrue refresh material missing')
                refresh_tokens[email] = refresh
            return bearer, claims

        def sign_in(email: str) -> tuple[str, dict]:
            """Redeem one genuine code and validate its actual role/issuer/expiry."""
            status, generated = request('/auth/v1/admin/generate_link',
                body={'type': 'magiclink', 'email': email})
            generated = object_json(generated)
            properties = object_json(generated.get('properties', generated))
            if status != 200 or not isinstance(properties.get('email_otp'), str):
                raise RuntimeError('Owned GoTrue code generation failed')
            status, signed = request('/auth/v1/verify', admin=False,
                body={'email': email, 'token': properties['email_otp'], 'type': properties['verification_type']})
            return signed_credential(status,signed,email)

        def refresh_owner() -> tuple[str,dict]:
            email = 'native-artifact-owner@example.invalid'
            status, signed = request('/auth/v1/token?grant_type=refresh_token',admin=False,
                body={'refresh_token':refresh_tokens[email]})
            fresh, fresh_claims = signed_credential(status,signed,email)
            if fresh_claims.get('sub') != claims['sub']:
                raise RuntimeError('Fresh genuine credential changed the fixture owner')
            return fresh,fresh_claims

        bearer, claims = sign_in('native-artifact-owner@example.invalid')
        admission, _ = qualify_admission(claims, realm.sql)
        result['ordinaryAccountAdmitted'] = admission['replaySameAccount'] is True
        bucket_attempted = True
        status, _ = request('/storage/v1/bucket', body={'id': bucket, 'name': bucket, 'public': False,
            'file_size_limit': 32 * 1024 * 1024, 'allowed_mime_types': ['application/json']})
        if status not in (200, 201):
            raise RuntimeError('Owned native bucket creation failed')
        status, metadata = request('/storage/v1/bucket/' + bucket)
        if status != 200 or object_json(metadata).get('id') != bucket or object_json(metadata).get('public') is not False:
            raise RuntimeError('Owned native bucket is not private')
        if not initial:
            control = 'qualification-positive.json'
            status, _ = request('/storage/v1/object/' + bucket + '/' + control,
                body={'qualification': 'ordinary-read-control'})
            if status not in (200, 201):
                raise RuntimeError('Owned positive control upload failed')
            realm.sql("CREATE POLICY native_artifact_positive_control ON storage.objects FOR SELECT TO authenticated USING (bucket_id='" + bucket + "' AND name='" + control + "')")
            result['fixturePositivePolicy'] = 'SELECT only on qualification-positive.json in the unique owned bucket; native-artifact-v1 paths excluded'
        else:
            result['privacyScope'] = 'Private bucket and exact server Store read only; ordinary read/sign denials qualified separately by artifacts mode'
        reservation_environment = {}
        participant_environment = {}
        reservation_file = None
        if connected_mode:
            from native_source_reader_acceptance import prepare_source_reader_reservation
            owner = str(uuid.UUID(realm.sql("SELECT id FROM app.account WHERE auth_subject='" + str(uuid.UUID(claims['sub'])) + "'")))
            peer_bearer, peer_claims = sign_in('native-source-peer@example.invalid')
            subject, session = str(uuid.UUID(peer_claims['sub'])), str(uuid.UUID(peer_claims['session_id']))
            cosmetics = [str(uuid.UUID(realm.sql("SELECT id FROM app.cosmetic WHERE kind='" + kind + "' AND is_free ORDER BY id LIMIT 1"))) for kind in ('back','sleeve','dice')]
            peer = str(uuid.UUID(realm.sql("SELECT app.register_account('" + subject + "','" + session + "','NativeSourcePeer'," + ','.join("'" + value + "'" for value in cosmetics) + ")")))
            outsider_bearer, outsider_claims = sign_in('native-source-outsider@example.invalid')
            outsider_subject, outsider_session = str(uuid.UUID(outsider_claims['sub'])), str(uuid.UUID(outsider_claims['session_id']))
            realm.sql("SELECT app.register_account('" + outsider_subject + "','" + outsider_session + "','NativeSourceOutsider'," + ','.join("'" + value + "'" for value in cosmetics) + ")")
            presentation_free = dict(zip(('backId','sleeveId','diceId'),cosmetics,strict=True)) | {'playmatId':None}
            participant_environment = {'SPELLWAKE_NATIVE_SOURCE_PEER_BEARER':peer_bearer,'SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER':outsider_bearer,'SPELLWAKE_NATIVE_PRESENTATION_FREE':json.dumps(presentation_free)}
            reservation = prepare_source_reader_reservation(realm,owner,peer,oracle_ref='058917c1-21ab-488a-9f9c-591c55f3c596' if arguments.rule_errors else 'b2c6aa39-2d2a-459c-a555-fb48ba993373',quantity=20,structure=arguments.structure,unlimited_basic_land=not arguments.rule_errors,mixed_ai=arguments.delivery_case in ('mixed-replay','controller'))
            if arguments.delivery_case == 'controller':
                # Separate normalized source family proves existing foreign grant authority only.
                foreign = prepare_source_reader_reservation(realm,peer,owner,quantity=20,structure='bo3',mixed_ai=True)
                reservation_environment['SPELLWAKE_NATIVE_FOREIGN_CONTROLLER_MATCH'] = str(uuid.UUID(foreign['matchId']))
            selected = object_json(json.loads(realm.sql("SELECT app.read_native_admission_source('" + reservation['gameId'] + "')")))
            if selected.get('status') != 'success':
                raise RuntimeError('Owned selected SQL reservation unavailable')
            selected_seats = object_json(selected['data'])['seats']
            if not isinstance(selected_seats, list) or len(selected_seats) != 2:
                raise RuntimeError('Owned selected SQL seat count differs')
            selected_entries = object_json(selected_seats[0]).get('entries')
            if not isinstance(selected_entries, list) or len(selected_entries) != 1:
                raise RuntimeError('Owned selected SQL entry count differs')
            first_entry = object_json(selected_entries[0])
            reservation['definitionId'] = str(uuid.UUID(first_entry['definitionId']))
            reservation['printingId'] = str(uuid.UUID(object_json(first_entry.get('printing'))['id']))
            reservation_file = realm.stage / 'private-native-source-reservation.json'
            private_write(reservation_file,json.dumps(reservation).encode())
            reservation_environment['SPELLWAKE_NATIVE_SOURCE_RESERVATION'] = str(reservation_file)
        phases = (('prepare','delivery') if mode == 'delivery' else ('prepare','results') if mode == 'results' else ('prepare','advance','verify')) if connected_mode else (None,)
        if mode == 'delivery' and arguments.delivery_case == 'expiry':
            phases = ('prepare','expiry-auth-preflight','expiry-source-preflight','expiry-auth-first','expiry-source-first','expiry-auth-later','expiry-source-later','expiry-unpolled','expiry-reconnect-auth','expiry-reconnect-source')
        if mode == 'delivery' and arguments.delivery_case == 'logout':
            phases = ('prepare','logout-reconnect-cursor','logout-stream-cursor','logout-first-presentation','logout-later-presentation')
        if mode == 'delivery' and arguments.delivery_case == 'presentation':
            phases = ('prepare','presentation')
        if mode == 'delivery' and arguments.delivery_case == 'sources':
            phases = ('prepare','sources')
        if mode == 'delivery' and arguments.delivery_case == 'handoff':
            phases = ('prepare','handoff-view','handoff-reconnect','handoff-stream','handoff-history','handoff-replay')
        if mode == 'delivery' and arguments.delivery_case == 'failures':
            phases = ('prepare','delivery-failures')
        if mode == 'delivery' and arguments.delivery_case == 'history-authority':
            phases = ('prepare','history-source','history-logout-history','history-logout-replay','history-bearer-history','history-bearer-replay')
        if mode == 'delivery' and arguments.delivery_case == 'replay-lost-ack':
            phases = ('prepare','replay-lost-ack')
        if mode == 'delivery' and arguments.delivery_case == 'mixed-replay':
            phases = ('prepare','mixed-replay')
        if mode == 'delivery' and arguments.delivery_case == 'controller':
            phases = ('prepare','controller')
        if mode == 'delivery' and arguments.delivery_case == 'publication-budget':
            phases = ('prepare','publication-budget')
        if mode == 'delivery' and arguments.delivery_case == 'publication':
            phases = ('prepare','publication-' + arguments.publication_case)
        if mode == 'delivery' and arguments.delivery_case == 'history-expiry':
            phases = ('prepare','history-expiry-history','history-expiry-replay','history-expiry-bearer-history','history-expiry-bearer-replay')
        if mode == 'delivery' and arguments.delivery_case == 'history-errors':
            phases = ('prepare','history-error-history','history-error-replay')
        if mode == 'delivery' and arguments.delivery_case == 'history-error-expiry':
            phases = ('prepare','history-error-expiry-history','history-error-expiry-replay')
        if mode == 'delivery' and arguments.delivery_case == 'read-account':
            phases = ('prepare','read-account-history','read-account-replay')
        if mode == 'delivery' and arguments.delivery_case == 'read-account-expiry':
            phases = ('prepare','handoff-account-history','handoff-account-replay')
        if mode == 'delivery' and arguments.delivery_case == 'archive-authority':
            variant = arguments.archive_variant
            phases = ('prepare','archive-prepare-' + ('incident' if variant == 'incident-own' else 'completed'),
                'archive-session-logout-compose-'+variant,'archive-session-logout-bearer-'+variant,
                'archive-session-expiry-compose-'+variant,'archive-session-expiry-bearer-'+variant,
                'archive-handoff-'+variant,'archive-unavailable-'+variant)
        if mode == 'delivery' and arguments.delivery_case in ('read-unavailable','read-cancellation'):
            phases = ('prepare',arguments.delivery_case + '-history',arguments.delivery_case + '-replay')
        if mode == 'delivery' and arguments.delivery_case == 'read-lifecycle':
            phases = ('prepare','lifecycle-read-' + arguments.read_origin + '-' + arguments.lifecycle_action)
        if mode == 'delivery' and arguments.delivery_case.startswith('deletion-'):
            phases = ('prepare',arguments.delivery_case)
        output_file = realm.stage / 'private-native-source-output.json'
        if connected_mode:
            private_write(output_file,b'{}')
            closed = realm.sql("SELECT NOT has_function_privilege('service_role','app.accept_reserved_game(uuid,numeric,uuid,text,jsonb)','EXECUTE') AND NOT has_function_privilege('service_role','app.accept_host_candidate(uuid,text,numeric,numeric,uuid,jsonb)','EXECUTE')")
            if closed != 't':
                raise RuntimeError('Native writer fixture requires unchanged closed runtime capabilities')
        stage_results = []
        for phase in phases:
            phase_environment = {} if phase is None else {'SPELLWAKE_NATIVE_SOURCE_PHASE':phase,'SPELLWAKE_NATIVE_SOURCE_OUTPUT':str(output_file), 'SPELLWAKE_NATIVE_VIEW_HTTP_WIRE':str(realm.stage / ('private-native-view-http-'+phase+'.json'))}
            if mode == 'delivery' and arguments.delivery_case in ('expiry','logout','handoff','history-authority','history-expiry','history-error-expiry','read-account-expiry') and phase != 'prepare':
                fresh_bearer,fresh_claims = refresh_owner() if short_expiry else sign_in('native-artifact-owner@example.invalid')
                if fresh_claims.get('sub') != claims['sub']:
                    raise RuntimeError('Delivery credential changed the actual owner')
                phase_environment.update(SPELLWAKE_NATIVE_STORAGE_CALLER=fresh_bearer,
                    SPELLWAKE_NATIVE_EXPIRY=str(fresh_claims['exp']),
                    SPELLWAKE_NATIVE_SUBJECT=str(uuid.UUID(fresh_claims['sub'])),
                    SPELLWAKE_NATIVE_SESSION=str(uuid.UUID(fresh_claims['session_id'])))
            if mode == 'delivery' and arguments.delivery_case == 'archive-authority' and phase != 'prepare':
                # Never refresh a session revoked by a preceding leaf phase.
                fresh_bearer,fresh_claims = sign_in('native-artifact-owner@example.invalid')
                phase_environment.update(SPELLWAKE_NATIVE_STORAGE_CALLER=fresh_bearer,
                    SPELLWAKE_NATIVE_EXPIRY=str(fresh_claims['exp']),
                    SPELLWAKE_NATIVE_SUBJECT=str(uuid.UUID(fresh_claims['sub'])),
                    SPELLWAKE_NATIVE_SESSION=str(uuid.UUID(fresh_claims['session_id'])))
            if mode == 'delivery' and (arguments.delivery_case in ('presentation','sources','read-lifecycle','archive-authority','read-account','read-account-expiry','publication-budget','mixed-replay','controller') or arguments.delivery_case.startswith('deletion-')) and phase != 'prepare':
                database_url = realm.status.get('DB_URL')
                parsed_database = urllib.parse.urlsplit(database_url) if isinstance(database_url,str) else None
                if parsed_database is None or parsed_database.hostname != '127.0.0.1' or parsed_database.port != realm.ports[1]:
                    raise RuntimeError('Presentation fixture requires exact owned loopback database')
                phase_environment['SPELLWAKE_NATIVE_FIXTURE_DATABASE_URL'] = database_url
            output = realm.command(['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--lib', test,
                '--', '--ignored', '--exact', '--nocapture'], timeout=2700 if mode == 'results' else 900 if mode == 'source-reader' and phase == 'verify' else 180, extra_environment={
                    'SPELLWAKE_SUPABASE_URL': base, 'SPELLWAKE_SUPABASE_SECRET_KEY': secret,
                    'SPELLWAKE_SUPABASE_PUBLISHABLE_KEY': public, 'SPELLWAKE_NATIVE_ARTIFACT_BUCKET': bucket,
                    'SPELLWAKE_NATIVE_STORAGE_CALLER': bearer, 'SPELLWAKE_NATIVE_STORAGE_QUALIFY': '1',
                    'SPELLWAKE_NATIVE_RESULT_TURNS':str(arguments.result_turns),
                    'SPELLWAKE_NATIVE_UNSUPPORTED_WORK':'1' if arguments.unsupported_work else '0',
                    'SPELLWAKE_NATIVE_RULE_ERRORS':'1' if arguments.rule_errors else '0',
                    'SPELLWAKE_NATIVE_NO_CONTEST':'1' if arguments.no_contest else '0',
                    **({'SPELLWAKE_NATIVE_VIEWS_POOL':'resolution-selection-natural-mana',
                        'SPELLWAKE_NATIVE_RECOVERY_MAX_RECORDS':'10000', 'SPELLWAKE_NATIVE_RECOVERY_MAX_BYTES':'10000000'} if connected_mode else {}),
                    'SPELLWAKE_CREATURE_VOCABULARY_INPUT': str(vocabulary),
                    **source_environment, **reservation_environment, **participant_environment, **phase_environment,
                })
            prefix = b'OWNED_NATIVE_SOURCE_READER_OK ' if connected_mode else b'OWNED_NATIVE_SELECTED_CHECKPOINT_OK ' if mode == 'selected-checkpoint' else b'OWNED_NATIVE_CHECKPOINT_OK ' if mode == 'checkpoint' else (b'OWNED_NATIVE_INITIAL_OK ' if initial else b'OWNED_NATIVE_ARTIFACT_OK ')
            rows = [line[len(prefix):] for line in output.splitlines() if line.startswith(prefix)]
            if len(rows) != 1 or b'test result: ok. 1 passed;' not in output:
                raise RuntimeError('Owned actual native Storage selector did not execute exactly once')
            metrics = object_json(json.loads(rows[0]))
            if phase in ('history-error-expiry-history','history-error-expiry-replay') and output.count(b'HTTP_RESPONSE_BODY_LIMIT_EXCEEDED 1\n') != 1:
                raise RuntimeError('Expired-error control did not reach actual one-byte body refusal exactly once')
            if phase in ('history-error-history','history-error-replay') and (output.count(b'NATIVE_RECOVERY_BUDGET_EXHAUSTED records\n') != 3 or output.count(b'HTTP_RESPONSE_BODY_LIMIT_EXCEEDED 1\n') != 3):
                raise RuntimeError('Causal recovery record/byte starvation did not execute exactly')
            if phase == 'replay-lost-ack' and output.count(b'REPLAY_PUBLICATION_BODY_TRANSPORT_FAILURE') != 1:
                raise RuntimeError('Actual publication client body-read failure was not observed exactly once')
            if connected_mode:
                if metrics.get('actualNativeTransitionEvidence') is not True:
                    raise RuntimeError('Actual native transition retention/accepted-history evidence did not execute')
                if phase == 'prepare' and not (metrics.get('actualInitialClockObservation') is True and metrics.get('falseOpeningClockRejected') is True):
                    raise RuntimeError('Actual initial clock observation/denial did not execute')
                if phase != 'prepare' and mode != 'delivery' and metrics.get('actualClockIntentDerived') is not True:
                    raise RuntimeError('Actual native clock intent producer did not execute')
            if mode == 'selected-checkpoint' and not (
                    metrics.get('selectedPrintingProvenance') is True
                    and metrics.get('verifiedTips') == 4
                    and metrics.get('fullyQualifiedCards') == 0
                    and metrics.get('deckFormatAdmissionQualified') is False):
                raise RuntimeError('Selected native provenance or explicit qualification limits differ')
            if connected_mode:
                if not all(metrics.get(field) is True for field in (
                        'initialVersion2', 'selectedPrintingProvenance', 'selectedSourceActualSql', 'immutableSourceDisplay')):
                    raise RuntimeError('Owned v2 selected SQL provenance did not execute')
                if metrics.get('phase') != phase:
                    raise RuntimeError('Owned native reader phase differs')
                if phase in ('prepare','advance'):
                    body = object_json(json.loads(output_file.read_bytes()))
                    if body.get('contextId') != reservation['contextId']:
                        raise RuntimeError('Owned native writer proposed a foreign context')
                    if phase == 'prepare':
                        if metrics.get('actualRustOpeningAcceptance') is not True or metrics.get('sameOriginalRetry') is not True:
                            raise RuntimeError('Owned Rust original opening acceptance/retry did not execute')
                        sql = f"SELECT app.read_native_opening_source('{reservation['gameId']}')"
                    else:
                        if metrics.get('actualRustEntropyCommit') is not True or metrics.get('sameOriginalRetry') is not True:
                            raise RuntimeError('Owned Rust original entropy commit/retry did not execute')
                        sql = f"SELECT app.read_native_game_source('{reservation['gameId']}')"
                    ack = object_json(json.loads(realm.sql(sql)))
                    if ack.get('status') != 'success':
                        raise RuntimeError('Owned native source readback failed')
                    if phase == 'prepare' and arguments.structure == 'bo3':
                        from native_source_reader_acceptance import qualify_initial_clock_recovery
                        result['initialClockRecovery'] = qualify_initial_clock_recovery(realm, reservation, body)
                if phase in ('advance','verify'):
                    if not all(metrics.get(field) is True for field in ('actualTypedSnapshotCursor','actualTypedCursorCorrelation','actualTypedCurrentPresentation','actualNativeReconnectHttp','actualNativeStreamHttp','actualReconnectUnknownNotResubmitted')) or metrics.get('currentPresentationFree') != presentation_free:
                        raise RuntimeError('Typed cursor/presentation adapters or actual free configuration did not execute')
                    if metrics.get('actualNativeViewHttp') is not True or metrics.get('publicNativeViewScopeDiscovered') is not True or metrics.get('actualSelectedClockPolicyObserved') is not True:
                        raise RuntimeError('Actual native View HTTP boundary did not execute')
                    wire_file = realm.stage / ('private-native-view-http-'+phase+'.json')
                    validation = realm.command(['bun', str(REPO / 'packages/application-contracts/tools/validate-native-view-wire.ts'), str(wire_file)])
                    if validation.strip() != b'NATIVE_VIEW_RECONNECT_STREAM_HTTP_WIRE_OK 2 4 3':
                        raise RuntimeError('Actual Rust HTTP payloads failed maintained Zod validation')
                    wire_capture=object_json(json.loads(wire_file.read_bytes()))
                    reconnect_samples=wire_capture.get('reconnect')
                    if not isinstance(reconnect_samples,list) or len(reconnect_samples)!=4:
                        raise RuntimeError('Missing actual reconnect wire captures')
                    unknown=object_json(reconnect_samples[-1])
                    original=object_json(object_json(unknown['request'])['receipts'])['outstanding'][0]
                    original_id=str(uuid.UUID(object_json(original)['requestId']))
                    if realm.sql("SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND request_ref='"+original_id+"'")!='0':
                        raise RuntimeError('Reconnect resubmitted an unknown original')
                    metrics['actualNativeReconnectWireValidated']=True
                    if not isinstance(wire_capture.get('streams'),list) or len(wire_capture['streams'])!=3:
                        raise RuntimeError('Missing actual stream wire captures')
                    metrics['actualNativeStreamWireValidated']=True
                    metrics['actualNativeViewHttpWireValidated'] = True
                    required_race = 'actualNativeViewLateStale' if phase=='advance' else 'actualNativeViewLateRevocation'
                    if metrics.get(required_race) is not True:
                        raise RuntimeError('Owned in-flight View authority control did not execute')
                    # Verify also issues the peer's revision-two View before late Auth revocation.
                    expected_views = 2 if phase == 'advance' else 6
                    view_objects = realm.sql("SELECT count(*) FROM storage.objects WHERE bucket_id='" + bucket + "' AND name LIKE 'native-artifact-v1/" + reservation['gameId'] + "/" + reservation['contextId'] + "/view/%'")
                    view_receipts = realm.sql("SELECT count(*) FROM app.command_receipt WHERE scope='native_view_v1' AND state='accepted' AND target_ref='" + reservation['gameId'] + "'")
                    if view_objects != str(expected_views) or view_receipts != str(expected_views):
                        raise RuntimeError('Owned cached View delivery created extra objects or receipts')
                    metrics['issuedViewObjects'] = expected_views
                stage_results.append(metrics)
            else:
                result['native'] = metrics
        if mode == 'delivery' and arguments.delivery_case == 'lifecycle':
            metrics = stage_results[-1]
            required = ('actualControlledBodyPolling','actualFirstPollStale','actualStreamCursorAdvanceSequence',
                        'actualStreamCapacity','actualStreamPrePollDrop','actualStreamDelayDrop',
                        'actualStreamInFlightDrop','actualStreamWorkPermitRetained','noDetachedSamples')
            if not all(metrics.get(field) is True for field in required):
                raise RuntimeError('Required actual stream body lifecycle case did not execute')
            if metrics.get('actualStreamExpiryQualified') is not False or metrics.get('actualStreamCosmeticRacesQualified') is not False:
                raise RuntimeError('Unexecuted expiry/cosmetic qualification must remain explicit')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('accepted',count(*) FILTER (WHERE state='accepted'),'rejected',count(*) FILTER (WHERE state='rejected'),'total',count(*)) FROM app.command_receipt WHERE scope='native_player_v1'")))
            if counts != {'accepted':2,'rejected':0,'total':2} or metrics.get('submittedPlayerCommands') != 2:
                raise RuntimeError('Stream lifecycle submitted unexpected player originals')
            result['native'] = {'stages':stage_results,'lifecycle':metrics,'playerOriginalCounts':counts,
                'matchStructure':arguments.structure,'nativeTransitionEvidenceQualified':True,
                'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual controlled Axum router/body with real Auth/SQL/private Storage; expiry, cosmetic races and final component reviews remain open'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case in ('handoff','failures'):
            metrics = stage_results[-1]
            if arguments.delivery_case == 'handoff':
                if tuple(item.get('phase') for item in stage_results) != ('prepare','handoff-view','handoff-reconnect','handoff-stream','handoff-history','handoff-replay') or not all(all(item.get(field) is True for field in ('positiveDelivery','actualUnpolledWorkerComplete','actualOriginalExpiryCrossed','expectedUnauthorized','sourceUnchanged')) for item in stage_results[1:]) or any(item.get('parkedFinalBearerReplies') != 2 or item.get('actualFinalBearerRepliesAccepted') is not True or item.get('noResumeIo') is not True for item in stage_results if item.get('phase') in ('handoff-history','handoff-replay')):
                    raise RuntimeError('Genuine completed-worker JSON handoff did not execute')
            elif len(stage_results)!=2 or metrics.get('actualFailureCases')!=17 or not all(metrics.get(field) is True for field in ('actualInitialJson503','actualTerminalServiceClosure','actualBoundedRecovery','sourceUnchanged')):
                raise RuntimeError('Actual delivery service-failure cases did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('Read delivery fixture mutated native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual native delivery; controlled provider byte/status/schema faults are not physical Storage corruption or full-card/format qualification'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'history-authority':
            expected_phases = ('prepare','history-source','history-logout-history','history-logout-replay','history-bearer-history','history-bearer-replay')
            if tuple(item.get('phase') for item in stage_results) != expected_phases:
                raise RuntimeError('Required late history/replay phases did not execute exactly')
            if sum(item.get('lateSourceCases',0) for item in stage_results) != 2 or sum(item.get('lateLogoutCases',0) for item in stage_results) != 2 or sum(item.get('requestSideFinalBearerCases',0) for item in stage_results) != 2:
                raise RuntimeError('Required actual source/logout/final bearer boundaries did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('History authority fixture changed accepted native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'lateSourceCases':2,'lateLogoutCases':2,'requestSideFinalBearerCases':2,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Active original-walk history/replay source and logout subset only; expiry, lifecycle, archive and parked-worker cases remain open'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'replay-lost-ack':
            metrics = stage_results[-1]
            required = ('actualTerminalCommand','acceptedSqlPublication','actualSafeInitialFailure','sameCanonicalManifestRetry','incompatibleCurrentProgramsRetry','sourceUnchangedAcrossPublication')
            if tuple(item.get('phase') for item in stage_results) != ('prepare','replay-lost-ack') or not all(metrics.get(field) is True for field in required) or metrics.get('lostAcknowledgmentBodies') != 1 or metrics.get('rebuildAttempts') != 0:
                raise RuntimeError('Required accepted-publication transport-loss/original-retry cases did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':1,'revision':'1','gameResults':1,'replayManifests':1}:
                raise RuntimeError('Publication loss/retry changed canonical accepted source or manifest')
            result['native'] = {'stages':stage_results,'persisted':counts,'actualClientBodyReadFailure':True,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual accepted publication followed by truncated HTTP body and original archive retry; concurrent publication/source/actor races remain open'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'controller':
            metrics = stage_results[-1]
            expected = {'actualLocalPossession':True,'actualControllerView':True,'reciprocalPrivateHands':True,'humanConsumerPreserved':True,'sealedBridgeRejection':True,'actualStaleDispatch':True,'actualControllerCommand':True,'originalRetry':True,'replacementOriginal':True,'lostAckRecovered':True,'localLateRevocationDenied':True,'canonicalLateRevocationDenied':True,'compositionErrorRevocationDenied':True,'canonicalRevokedGrantDenied':True,'foreignExistingGrantDenied':True,'unknownGrantDenied':True,'foreignBindingDenied':True,'wrongEpochDenied':True,'foreignContextDenied':True,'supersededGenerationDenied':True,'supersededCommandNoWrite':True,'rawCredentialAbsentFromCanonicalRows':True,'providerActivated':False}
            if tuple(item.get('phase') for item in stage_results) != ('prepare','controller') or any(metrics.get(key) != value for key,value in expected.items()):
                raise RuntimeError('Required actual private controller consumer branches did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchPhase',(SELECT state FROM app.match WHERE id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':3,'revision':'1','gameResults':1,'matchPhase':'between_games','replayManifests':0}:
                raise RuntimeError('Actual controller command changed canonical once-only counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,'limits':'Actual local secret registry, private controller consumers, canonical RPC/Storage/native bytes. Controlled normalized provider/delivery/Ready observations; no model/provider activation, durable secret custody or full game/deck/format qualification.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'mixed-replay':
            metrics = stage_results[-1]
            expected = {'sourceHumanSeats':1,'sourceAiSeats':1,'archivePerspectives':3,'filteredHumanPerspectives':1,
                'completedPerspectives':2,'sponsorAiHistoricalDenials':2,'outsideRouteDenials':20,'outsideAccessDenials':2,'authorizedArchiveReads':3,
                'sourceUnchanged':True,'originalIdentityUnchanged':True,'canonicalRowsUnchanged':True,'completedHistoryPositive':True,'incompatibleArchiveNoRecovery':True,'canonicalGrantAdopted':True,'providerActivated':False,'controllerPossessionQualified':False}
            if tuple(item.get('phase') for item in stage_results) != ('prepare','mixed-replay') or any(metrics.get(key) != value for key,value in expected.items()):
                raise RuntimeError('Required actual mixed human/AI archive branches did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':1,'revision':'1','gameResults':1,'replayManifests':1}:
                raise RuntimeError('Mixed replay changed canonical native/result/archive counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Real native bytes and accepted mixed roster/archive; canonical self-sponsored AI slot/bootstrap/adoption with controlled fixture provider/delivery/Ready observations. No real provider, SecretStore possession, AI command or genuine admission qualification.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'publication-budget':
            metrics = stage_results[-1]
            if tuple(item.get('phase') for item in stage_results) != ('prepare','publication-budget') or not all(metrics.get(key) is True for key in ('actualAggregateAccounts','sharedStarvationIsolated','sourceUnchanged','sameCanonicalManifest')):
                raise RuntimeError('Actual publication budget controls did not execute')
            inventory = object_json(metrics['actualIndependentInventory'])
            for name in ('measured','restored'):
                measured = object_json(metrics[name])
                if measured.get('plannedWalks') != 4 or measured.get('humanSeats') != 2 or measured.get('rosterSeats') != 2 or measured.get('firstTemplate') != {'records':10000,'bytes':10000000} or measured.get('sharedCapacity') != {'records':30000,'bytes':30000000}:
                    raise RuntimeError('Publication template/perspective measurements differ')
                for key in ('retainedPayloadOperations','retainedPayloadBytes','pageCounts','cutsPerPerspective'):
                    if measured.get(key) != inventory.get(key):
                        raise RuntimeError('Publication independent inventory differs')
                walks = measured.get('walks')
                if not isinstance(walks,list) or len(walks) != 4 or sum(walk.get('first') is True for walk in walks) != 1:
                    raise RuntimeError('Publication walk accounting is incomplete')
                for axis in ('records','bytes'):
                    first = object_json(measured['firstSpend'])[axis]
                    shared = object_json(measured['sharedSpend'])[axis]
                    if first + measured['firstRemaining'][axis] != measured['firstTemplate'][axis] or shared + measured['sharedRemaining'][axis] != measured['sharedCapacity'][axis]:
                        raise RuntimeError('Publication account limits do not reconcile')
                    retained = inventory['retainedPayloadOperations' if axis == 'records' else 'retainedPayloadBytes']
                    if shared != retained + sum(walk['spend'][axis] for walk in walks if not walk['first']):
                        raise RuntimeError('Publication shared debits do not reconcile')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchResults',(SELECT count(*) FROM app.match_result WHERE match_id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':1,'revision':'1','gameResults':1,'matchResults':1,'replayManifests':1}:
                raise RuntimeError('Publication budget changed canonical native/result/archive counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual finite Concede publication aggregate recovery-work/retained-payload accounting and isolated shared-record starvation; no demonstrated long-game headroom or total network/CPU/memory/latency qualification.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'publication':
            metrics = stage_results[-1]
            case = arguments.publication_case
            incident = case == 'incident'
            expected = {'actualConcurrentPublication':True,'sameCanonicalManifest':True,'publicationInterleavings':1,
                'sourceAdvanced':case == 'source','sourceUnchanged':case != 'source','accessPublicationOnly':case != 'source',
                'actorRevoked':case == 'logout','operationalPrefixOnly':incident,'strictStreamTerminalStale':case == 'stream'}
            if tuple(item.get('phase') for item in stage_results) != ('prepare','publication-' + case) or any(metrics.get(field) != value for field,value in expected.items()):
                raise RuntimeError('Required actual publication interleaving or authority distinction did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'),'gameState',(SELECT state::text FROM app.game WHERE id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':0 if incident else 1,'revision':'0' if incident else '1','gameResults':0 if incident else 1,'replayManifests':1,'gameState':'no_contest' if incident else 'completed'}:
                raise RuntimeError('Publication fixture changed expected canonical native source or manifest')
            result['native'] = {'stages':stage_results,'persisted':counts,'publicationCase':case,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'One separately owned causal publication case; other publication/authority cases have separate receipts. Stream stale after headers is a terminal SSE failure, not a rewritten HTTP status.'}
            if case == 'stream':
                result['native']['streamDispositionAuthority'] = 'reviews/native-history-implementation-plan-v2.md:15 (existing current stream behavior remains strict)'
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'history-expiry':
            if tuple(item.get('phase') for item in stage_results) != ('prepare','history-expiry-history','history-expiry-replay','history-expiry-bearer-history','history-expiry-bearer-replay') or sum(item.get('lateExpiryCases',0) for item in stage_results) != 2 or sum(item.get('requestSideExpiryCases',0) for item in stage_results) != 2 or not all(item.get('expiryCredentialAfterPreparation') is True for item in stage_results[1:]):
                raise RuntimeError('Required genuine history/replay expiry boundaries did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('History expiry fixture changed accepted native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'lateExpiryCases':2,'requestSideExpiryCases':2,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Active original-walk history/replay genuine signed credential expiry only; lifecycle/archive/error controls remain separate'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'history-errors':
            expected = {'actualBudgetFailures':6,'unchangedAuthSafe503':4,'lateAuthOverrides503':2,'restoredFullBudgetSuccesses':2,'budgetRecordFailures':3,'budgetByteFailures':3,'sourceUnchanged':True}
            if tuple(item.get('phase') for item in stage_results) != ('prepare','history-error-history','history-error-replay') or any(any(item.get(field) != value for field,value in expected.items()) for item in stage_results[1:]):
                raise RuntimeError('Required original-error/late-auth precedence controls did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('History error-precedence fixture changed accepted native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'actualBudgetFailures':12,'lateAuthOverrides503':4,'byteRefusalBoundary':'SQL RPC response bounded by the caller remaining byte budget of1','fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Causal active-walk recovery record/byte starvation and late-logout error precedence only; archive/lifecycle/page-refit/scan/publication aggregate budgets remain separate'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'history-error-expiry':
            required = ('actualBoundedErrorBeforeExpiry','actualExpiredError401','expiryCredentialAfterPreparation','sourceUnchanged')
            if tuple(item.get('phase') for item in stage_results) != ('prepare','history-error-expiry-history','history-error-expiry-replay') or not all(all(item.get(field) is True for field in required) for item in stage_results[1:]):
                raise RuntimeError('Genuine expired composition-error guards did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('Expired-error fixture changed native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'actualExpiredError401':True,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Real JWT expiry after bounded recovery failure; no source/session/auth proof fabrication or longer network timeout'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case in ('read-account','read-account-expiry'):
            expiry = arguments.delivery_case == 'read-account-expiry'
            expected = {'sourceUnchanged':True,'noNativeIo':True,'restoredExactPositive':True}
            if expiry:
                expected.update({'actualAccountNone':True,'expiryCredentialAfterPreparation':True,'actualUnpolledWorkerComplete':True,'parkedFinalBearerReplies':2,
                    'actualFinalBearerRepliesAccepted':True,'actualOriginalExpiryCrossed':True,'expectedUnauthorized':True,'noResumeIo':True,'noResumeInitiatedIo':True})
            else:
                expected.update({'genuineAccountUnavailable':True,'acceptedUnregisteredReplies':2})
            if tuple(item.get('phase') for item in stage_results) != phases or len(stage_results) != 3 or any(any(item.get(key) != value for key,value in expected.items()) for item in stage_results[1:]):
                raise RuntimeError('Genuine unavailable-account read controls did not execute')
            subjects = {str(uuid.UUID(item['unregisteredSubject'])) for item in stage_results[1:]}
            if len(subjects) != 1:
                raise RuntimeError('Unregistered subject changed between read paths')
            subject = subjects.pop()
            absent = object_json(json.loads(realm.sql("SELECT jsonb_build_object('authSubjects',(SELECT count(*) FROM auth.users WHERE id='"+subject+"'),'applicationAccounts',(SELECT count(*) FROM app.account WHERE auth_subject='"+subject+"'))")))
            if absent != {'authSubjects':1,'applicationAccounts':0}:
                raise RuntimeError('Genuine Auth caller must remain unregistered in application ledger')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchResults',(SELECT count(*) FROM app.match_result WHERE match_id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':0,'revision':'0','gameResults':0,'matchResults':0,'replayManifests':0}:
                raise RuntimeError('Account intake changed native source/results/archive')
            result['native'] = {'stages':stage_results,'persisted':counts,'genuineUnregisteredSource':absent,
                'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Genuine authenticated needs_registration caller; unchanged active native source, no native/Storage work for measured denied requests. Worker expiry uses actual completed router worker and real original JWT deadline.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'archive-authority':
            variant = arguments.archive_variant
            if tuple(item.get('phase') for item in stage_results) != phases or len(stage_results) != 8 or stage_results[1].get('canonicalArchivePrepared') is not True or stage_results[1].get('fixtureObligationsEmpty') is not True or stage_results[1].get('fixtureShapeChecked') is not True or stage_results[1].get('incidentDeniedModesChecked') != (3 if variant == 'incident-own' else 0) or stage_results[1].get('incident') != (variant == 'incident-own'):
                raise RuntimeError('Archive authority preparation/phases differ')
            leaf_counts = [dict(lateLogoutCases=1,requestSideFinalBearerCases=0,lateExpiryCases=0,requestSideExpiryCases=0),
                dict(lateLogoutCases=0,requestSideFinalBearerCases=1,lateExpiryCases=0,requestSideExpiryCases=0),
                dict(lateLogoutCases=0,requestSideFinalBearerCases=0,lateExpiryCases=1,requestSideExpiryCases=0),
                dict(lateLogoutCases=0,requestSideFinalBearerCases=0,lateExpiryCases=0,requestSideExpiryCases=1)]
            for index,expected in enumerate(leaf_counts):
                item = stage_results[index+2]
                if any(item.get(key) != value for key,value in expected.items()) or item.get('archiveReaderNoRecovery') is not True or item.get('sourceUnchanged') is not True or (index>=2 and item.get('expiryCredentialAfterPreparation') is not True):
                    raise RuntimeError('Archive session boundary did not execute its exact leaf')
            handoff,unavailable = stage_results[6:]
            if not all(handoff.get(key) is True for key in ('positiveDelivery','actualUnpolledWorkerComplete','actualFinalBearerRepliesAccepted','actualOriginalExpiryCrossed','expectedUnauthorized','sourceUnchanged','archiveReaderNoRecovery','expiryCredentialAfterPreparation','noResumeIo','noResumeInitiatedIo')) or handoff.get('parkedFinalBearerReplies') != 2:
                raise RuntimeError('Archive actual parked-worker expiry guard did not execute')
            if not all(unavailable.get(key) is True for key in ('controlledFinalAccessUnavailable','restoredExactPositive','sourceUnchanged','archiveReaderNoRecovery')) or unavailable.get('targetedFinalAccessHits') != 1:
                raise RuntimeError('Archive controlled final-access unavailable guard did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchResults',(SELECT count(*) FROM app.match_result WHERE match_id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            terminal = variant != 'incident-own'
            if counts != {'playerOriginals':int(terminal),'revision':'1' if terminal else '0','gameResults':int(terminal),'matchResults':1,'replayManifests':1}:
                raise RuntimeError('Archive authority changed canonical native/result/archive counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'archiveVariant':variant,'authorityLeafCases':6,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'One archive perspective over six actual late authority/handoff controls. Controlled trusted unavailable is not canonical deletion; incompatible current programs do not recover archived native state. Admission/cards/formats remain unqualified.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case in ('read-unavailable','read-cancellation'):
            case = arguments.delivery_case
            expected = {'sourceUnchanged':True,'restoredExactPositive':True}
            if case == 'read-unavailable':
                expected.update({'controlledFinalAccessUnavailable':True,'targetedFinalAccessHits':1})
            else:
                expected.update({'composedWorkers':16,'canceledCallers':1,'runningPermitRetained':True,'overflowWithoutIo':True,'completedWorkers':16,'singleWorkerRestoredCapacity':True,'canceledWorkerBearerReplies':2,'actualSuccessfulBearerReplies':32,'noLaterSamplingObservedMillis':1100})
            if tuple(item.get('phase') for item in stage_results) != ('prepare',case+'-history',case+'-replay') or any(any(item.get(field) != value for field,value in expected.items()) for item in stage_results[1:]):
                raise RuntimeError('Required actual read unavailability/cancellation controls did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchResults',(SELECT count(*) FROM app.match_result WHERE match_id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':0,'revision':'0','gameResults':0,'matchResults':0,'replayManifests':0}:
                raise RuntimeError('Read controls changed canonical native/result/archive counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Controlled trusted final-access unavailability is not canonical game deletion. Cancellation drops actual caller futures; running blocking work finishes normally. No-later-IO observation is finite1100ms.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'read-lifecycle':
            metrics = stage_results[-1]
            origin, action = arguments.read_origin, arguments.lifecycle_action
            paths = ['history','own-replay'] + (['full-replay'] if origin == 'completed' else [])
            expected = {'readPaths':paths,'concurrentComposedReads':len(paths),'actualPendingRead':action == 'pending',
                'actualRequesterRetirement':action == 'requester','actualPeerRetirement':action == 'peer','sourceUnchanged':True,'fixtureObligationsEmpty':True,'incidentDeniedModesChecked':3 if origin == 'incident' else 0,'incompatibleProgramsArchive':origin != 'active'}
            if tuple(item.get('phase') for item in stage_results) != ('prepare','lifecycle-read-' + origin + '-' + action) or any(metrics.get(field) != value for field,value in expected.items()):
                raise RuntimeError('Required concurrent canonical lifecycle read branches did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='"+reservation['gameId']+"'),'matchResults',(SELECT count(*) FROM app.match_result WHERE match_id='"+reservation['matchId']+"'),'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='"+reservation['gameId']+"'))")))
            if counts != {'playerOriginals':1 if origin == 'completed' else 0,'revision':'1' if origin == 'completed' else '0','gameResults':1 if origin == 'completed' else 0,'matchResults':0 if origin == 'active' else 1,'replayManifests':0 if origin == 'active' else 1}:
                raise RuntimeError('Lifecycle fixture changed canonical native/result/archive counts')
            result['native'] = {'stages':stage_results,'persisted':counts,'origin':origin,'lifecycleAction':action,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'One owned canonical lifecycle interleaving over every selected real read path; archive readers use incompatible current programs. Upstream admission/cards/formats remain unqualified.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'sources':
            metrics = stage_results[-1]
            if len(stage_results)!=2 or metrics.get('actualCanonicalIncidentCases')!=4 or metrics.get('actualCanonicalRestorations')!=4 or not all(metrics.get(field) is True for field in ('actualTerminalDisclosureCrossing','sourceUnchangedAcrossReads')):
                raise RuntimeError('Required actual late source/operational cases did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND state='accepted'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'incidents',(SELECT count(*) FROM app.command_receipt WHERE scope='game_incident_v1' AND state='accepted'),'restorations',(SELECT count(*) FROM app.command_receipt WHERE scope='game_restoration_v1' AND state='accepted'))")))
            if counts != {'playerOriginals':1,'revision':'1','incidents':4,'restorations':4}:
                raise RuntimeError('Late source fixture committed unexpected originals')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Canonical trusted fixture incidents/restorations and actual terminal crossing; not real host-failure automation, replay/semantic-history or native timeout qualification'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case.startswith('deletion-'):
            metrics = stage_results[-1]
            if len(stage_results)!=2 or not all(metrics.get(field) is True for field in (
                    'actualTerminalCommand','canonicalOutcomeConsumed','peerRetiredCanonically',
                    'actualDeletedOwnerReplacement','actualCallerDeletionRefused','sourceUnchangedAcrossReads')):
                raise RuntimeError('Required actual historical-owner/caller deletion case did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND state='accepted'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'),'retired',(SELECT count(*) FROM app.account WHERE state='deleted'))")))
            if counts != {'playerOriginals':1,'revision':'1','retired':2}:
                raise RuntimeError('Deletion fixture terminal/retirement source differs')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual native terminal and historical-owner/caller deletion; full replay/semantic history remains unqualified'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'presentation':
            metrics = stage_results[-1]
            if len(stage_results) != 2 or not all(metrics.get(field) is True for field in (
                    'actualRemainingGrant','actualLastGrantRefresh','actualEqualCursorAppearanceRefresh',
                    'actualBeforeObservation','actualDuringObservation','actualAfterObservation',
                    'actualHeaderCatalogueGrantMatrix','sourceUnchanged')):
                raise RuntimeError('Required actual presentation case did not execute')
            if metrics.get('coherentCases') != 9 or metrics.get('deletedOwnerQualified') is not False:
                raise RuntimeError('Presentation scope/count differs or unexecuted deletion hidden')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('Cosmetic observations mutated accepted native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual snapshot/presentation delivery over normalized SQL entitlement fixtures; not Stripe settlement. Deleted historical owner remains separate.'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'logout':
            if len(stage_results) != 5 or not all(all(item.get(field) is True for field in (
                    'positiveDelivery','acceptedIoBeforeLogout','actualLocalLogout','bearerUnexpired',
                    'expectedUnauthorized','sourceUnchanged')) for item in stage_results[1:]):
                raise RuntimeError('Required actual late-logout case did not execute')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('Logout observations mutated accepted native source')
            result['native'] = {'stages':stage_results,'persisted':counts,'fullyQualifiedCards':0,
                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual late local logout; deletion/disclosure/cosmetic races and final component reviews remain separate'}
            output_file.unlink()
        if mode == 'delivery' and arguments.delivery_case == 'expiry':
            if len(stage_results) != 10 or not all(all(item.get(field) is True for field in (
                    'positiveDelivery','actualOriginalExpiryCrossed','expectedUnauthorized','sourceUnchanged'))
                    for item in stage_results[1:]):
                raise RuntimeError('Required genuine expiry case did not execute')
            if not all(item.get('actualAcceptedReplyHeld') is True for item in stage_results[1:] if item['phase'] != 'expiry-unpolled'):
                raise RuntimeError('Expiry requires real accepted upstream reply hold')
            counts = object_json(json.loads(realm.sql("SELECT jsonb_build_object('playerOriginals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1'),'revision',(app.read_native_game_source('"+reservation['gameId']+"')->'data'->>'acceptedRevision'))")))
            if counts != {'playerOriginals':0,'revision':'0'}:
                raise RuntimeError('Expiry observations mutated accepted native source')
            result['native'] = {'stages':stage_results,'issuedJwtLifetimeSeconds':30,'persisted':counts,
                'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Genuine timed expiry only; logout and cosmetic races remain separate'}
            output_file.unlink()
        if mode == 'source-reader':
            if stage_results[-1].get('actualNativeHumanCommandHttp') is not True or stage_results[-1].get('actualFreshCommandStoreFailure') is not True or stage_results[-1].get('actualRuleRejectHttp') is not True or stage_results[-1].get('actualCommandOwnerRaces') is not True or stage_results[-1].get('actualCommandLateRevocation') is not True or stage_results[-1].get('actualCompetingCommands') is not True or stage_results[-1].get('actualCommandBasisControls') is not True or stage_results[-1].get('actualCommandBudgetRefusals') is not True or stage_results[-1].get('actualCommandArtifactFailures') is not True or stage_results[-1].get('actualCommandWriteFailures') is not True or stage_results[-1].get('actualCommandSourceProjectionFaults') is not True or stage_results[-1].get('actualCommandSqlClockRefusal') is not True:
                raise RuntimeError('Actual native human command HTTP/Store failure/rejection did not execute')
            if arguments.structure == 'bo3' and stage_results[-1].get('actualCommandExhaustedBankProjection') is not True:
                raise RuntimeError('Owned BO3 exhausted-bank projection control did not execute')
            if not all(item.get('actualHumanAccessAndBinding') is True and item.get('peerSeatAndOutsiderChecked') is True and item.get('actualIssuedViewAndRegistry') is True for item in stage_results[1:]) or stage_results[-1].get('revokedSessionDenied') is not True or stage_results[-1].get('actualNativePlayerCommitAndReceipts') is not True:
                raise RuntimeError('Owned actual human access/binding/revocation did not execute')
            result['native'] = {'stages':stage_results,'verifiedTips':sum(item.get('verifiedTips',0) for item in stage_results),
                                'sameActualCandidate':all(item.get('sameActualCandidate') is True for item in stage_results[1:]),
                                'actualManagedReadRpc':True,'actualPrivateStorage':True,'fullyQualifiedCards':0,
                                'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                                'actualHumanAccessAndBinding':True,'peerSeatAndOutsiderChecked':True,'revokedSessionDenied':True,
                                'actualIssuedViewAndRegistry':True,'actualNativeViewHttp':True,'publicNativeViewScopeDiscovered':True,'actualSelectedClockPolicyObserved':True,'actualClockIntentDerived':True,'matchStructure':arguments.structure,'actualNativeViewHttpWireValidated':True,'actualNativeViewLateStale':True,'actualNativeViewLateRevocation':True,'displayQualified':False,
                                'actualNativePlayerCommitAndReceipts':True,'actualNativeHumanCommandHttp':True,'actualFreshCommandStoreFailure':True,'actualRuleRejectHttp':True,'actualCommandOwnerRaces':True,'actualCompetingCommands':True,'actualCommandBasisControls':True,'actualCommandBudgetRefusals':True,'actualCommandArtifactFailures':True,'actualCommandWriteFailures':True,'actualCommandSourceProjectionFaults':True,'actualCommandSqlClockRefusal':True,'actualCommandExhaustedBankProjection':arguments.structure == 'bo3','actualCommandLateRevocation':True,
                                'initialVersion2':True,'selectedPrintingProvenance':True,'selectedSourceActualSql':True,'immutableSourceDisplay':True,
                                'nativeTransitionEvidenceQualified':True,'writeBoundary':'Rust service-only original224 opening and231entropy; actual human HTTP command execution, originals, rule/stale rejection and anchored recovery; fixture admission/cards and complete feature metrics remain unqualified'}
            output_file.unlink()
        if mode == 'results' and arguments.unsupported_work:
            metrics = stage_results[-1]
            if not all(metrics.get(key) is True for key in ('actualUnsupportedWorkRefused','originalUnknown','acceptedSourceUnchanged')):
                raise RuntimeError('Actual unsupported native boundary did not execute')
            payload = object_json(json.loads(output_file.read_bytes()))
            if payload.get('contextId') != reservation['contextId'] or payload.get('unsupportedKind') != metrics.get('unsupportedKind'):
                raise RuntimeError('Unsupported native source scope differs')
            game = str(uuid.UUID(reservation['gameId']))
            request_id = str(uuid.UUID(payload['requestId']))
            counts = realm.sql(f"SELECT (SELECT count(*) FROM app.game_result WHERE game_id='{game}')::text||':'||(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND target_ref='{game}' AND request_ref='{request_id}')::text")
            if counts != '0:0':
                raise RuntimeError('Unsupported work created a result or durable player receipt')
            result['native'] = {'stages':stage_results,'actualUnsupportedWorkRefused':True,'unsupportedKind':metrics['unsupportedKind'],'canonicalResultAndReceiptCounts':counts,'matchStructure':arguments.structure,'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False}
            output_file.unlink()
        if mode == 'results' and arguments.no_contest:
            metrics = stage_results[-1]
            if metrics.get('actualNoContestReplayHttp') is not True or metrics.get('operationalPrefixPublished') is not True:
                raise RuntimeError('Actual no-contest prefix replay did not execute')
            if not isinstance(metrics.get('activeReplayFramesChecked'), int) or metrics['activeReplayFramesChecked'] <= 0:
                raise RuntimeError('Actual in-progress own-seat replay did not execute')
            game = str(uuid.UUID(reservation['gameId']))
            persisted = object_json(json.loads(realm.sql(f"""SELECT jsonb_build_object(
              'gameState',(SELECT state FROM app.game WHERE id='{game}'),
              'matchResult',(SELECT jsonb_build_object('outcome',r.outcome,'kind',r.terminal_kind) FROM app.match_result r JOIN app.game g ON g.match_id=r.match_id WHERE g.id='{game}'),
              'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='{game}'),
              'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='{game}' AND schema_revision='spellwake-replay-v2' AND source_receipt_id IS NOT NULL))""")))
            if persisted != {'gameState':'no_contest','matchResult':{'outcome':'no_contest','kind':'unrecoverable_incident'},'gameResults':0,'replayManifests':1}:
                raise RuntimeError('No-contest terminal or prefix publication differs')
            result['native'] = {'stages':stage_results,'persisted':persisted,'actualNoContestReplayHttp':True,'matchStructure':arguments.structure,
                'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,
                'limits':'Actual incident pause and unrecoverable terminalization; own-seat operational prefix only, no full reveal'}
            output_file.unlink(missing_ok=True)
        if mode == 'results' and not arguments.unsupported_work and not arguments.rule_errors and not arguments.no_contest:
            terminal_metrics = stage_results[-1]
            if not (terminal_metrics.get('actualClockAccountingChecked') is True and terminal_metrics.get('actualWaitingActorChecked') is True):
                raise RuntimeError('Actual native clock ownership/accounting checks did not execute')
            if not all(terminal_metrics.get(key) is True for key in ('actualTerminalCommandHttp', 'actualTerminalStaleHttp', 'actualPlayerPathHttp', 'actualHistoryPagesHttp', 'actualPortableReplayHttp')):
                raise RuntimeError('Actual player/terminal HTTP path did not execute')
            if not isinstance(terminal_metrics.get('activeReplayFramesChecked'), int) or terminal_metrics['activeReplayFramesChecked'] <= 0:
                raise RuntimeError('Actual in-progress own-seat replay did not execute')
            budget = terminal_metrics.get('publicationBudget')
            if not isinstance(budget, dict) or not all(isinstance(budget.get(k), int) and budget[k] > 0 for k in ('cuts','walkRecords','walkBytes','configuredRecords','configuredBytes','plannedWalks')):
                raise RuntimeError('Publication budget measurement did not execute')
            controls = object_json(budget.get('archiveControls'))
            if controls.get('denseInteriorControls') != 3 or type(controls.get('genuineSilentCutInLoadedPrefix')) is not bool or controls.get('executed') != 30 + int(controls['genuineSilentCutInLoadedPrefix']):
                raise RuntimeError('Decoded archive interior gap/duplicate/reorder controls did not execute')
            if terminal_metrics.get('actualTerminalCommitAndRetry') is not True or terminal_metrics.get('exactColdResultEvidence') is not True or terminal_metrics.get('falseResultsRejectedBeforeCommit') is not True or terminal_metrics.get('actualSelectedClockPolicyObserved') is not True or terminal_metrics.get('targetOwnTurns') != arguments.result_turns:
                raise RuntimeError('Actual terminal result/participation producer did not execute')
            payload = object_json(json.loads(output_file.read_bytes()))
            expected_counts = ['3','2'] if arguments.result_turns == 3 else ['0','0']
            if payload.get('ownTurns') != expected_counts:
                raise RuntimeError('Actual native own-turn totals differ')
            game = str(uuid.UUID(reservation['gameId']))
            request_id = str(uuid.UUID(payload['requestId']))
            persisted = object_json(json.loads(realm.sql(f"""SELECT jsonb_build_object(
              'participation',(SELECT jsonb_agg(jsonb_build_object('seatId',p.match_seat_id,'ownTurns',p.own_turns::text,'realGame',p.real_game,'reason',p.reason) ORDER BY s.seat_index) FROM app.game_participation p JOIN app.match_seat s ON s.id=p.match_seat_id WHERE p.game_id='{game}'),
              'result',(SELECT jsonb_build_object('outcome',r.outcome,'winners',r.winner_seat_ids,'reason',r.end_reason) FROM app.game_result r WHERE r.game_id='{game}'),
              'results',(SELECT count(*) FROM app.game_result WHERE game_id='{game}'),
              'originals',(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND target_ref='{game}' AND request_ref='{request_id}' AND state='accepted'),
              'replayManifests',(SELECT count(*) FROM app.replay_manifest WHERE game_id='{game}' AND schema_revision='spellwake-replay-v2' AND source_receipt_id IS NOT NULL))""")))
            rows = persisted.get('participation')
            if not isinstance(rows, list) or len(rows) != 2:
                raise RuntimeError('Terminal SQL requires exactly both human participants')
            for index, row in enumerate(rows):
                expected = {'seatId':reservation['seatIds'][index], 'ownTurns':expected_counts[index], 'realGame':int(expected_counts[index])>=3, 'reason':'native_turn_starts_v1'}
                if row != expected:
                    raise RuntimeError('Persisted participation differs from actual native chain')
            if persisted.get('replayManifests') != 1:
                raise RuntimeError('Portable replay must publish exactly one canonical manifest')
            if persisted.get('result') != {'outcome':'win','winners':[reservation['seatIds'][1]],'reason':'native_concession'} or persisted.get('results') != 1 or persisted.get('originals') != 1:
                raise RuntimeError('Terminal SQL result/original retry is not source-once')
            result['native'] = {'stages':stage_results, 'targetOwnTurns':arguments.result_turns, 'actualTerminalCommitAndRetry':True, 'actualTerminalCommandHttp':True, 'actualTerminalStaleHttp':True, 'actualPlayerPathHttp':True, 'exactColdResultEvidence':True, 'persisted':persisted, 'actualClockIntentDerived':True,'actualClockAccountingChecked':True,'actualWaitingActorChecked':True,'matchStructure':arguments.structure, 'fullyQualifiedCards':0, 'semanticFactsEventsQualified':False, 'deckFormatAdmissionQualified':False, 'nativeTransitionEvidenceQualified':True, 'limits':'Actual result/participation, canonical transition evidence and derived clock intent; feature metrics, full cards and native timeout unsupported'}
            output_file.unlink()
        if mode == 'results' and arguments.rule_errors:
            metrics = stage_results[-1]
            if not (metrics.get('actualRuleTargetPaymentHttp') is True and metrics.get('actualExactRejectedRetries') is True):
                raise RuntimeError('Actual source-bound native rule errors did not execute')
            payload = object_json(json.loads(output_file.read_bytes()))
            if payload.get('contextId') != reservation['contextId'] or payload.get('oracleRef') != '058917c1-21ab-488a-9f9c-591c55f3c596':
                raise RuntimeError('Native rule source differs')
            ids = payload.get('requestIds')
            if not isinstance(ids,list) or len(ids)!=3 or len(set(ids))!=3:
                raise RuntimeError('Native rule requests differ')
            wanted = ','.join("'"+str(uuid.UUID(value))+"'" for value in ids)
            game = str(uuid.UUID(reservation['gameId']))
            counts = realm.sql(f"SELECT (SELECT count(*) FROM app.game_result WHERE game_id='{game}')::text||':'||(SELECT count(*) FROM app.command_receipt WHERE scope='native_player_v1' AND target_ref='{game}' AND request_ref IN({wanted}) AND state='rejected')::text")
            if counts != '0:3':
                raise RuntimeError('Native rejection originals are not source-once')
            result['native'] = {'stages':stage_results,'actualRuleTargetPaymentHttp':True,'canonicalResultAndRejectedOriginalCounts':counts,'matchStructure':arguments.structure,'fullyQualifiedCards':0,'semanticFactsEventsQualified':False,'deckFormatAdmissionQualified':False,'limits':'Actual Edict mechanism errors; synthetic SQL release/deck fixture, no complete-card or format admission'}
            output_file.unlink()
        if reservation_file is not None:
            reservation_file.unlink()
        result['primaryPassed'] = True
    except BaseException as error:
        primary = error
        result['primaryPassed'] = False
        result['primaryFailureType'] = type(error).__name__
        if isinstance(error, QualificationCommandError):
            result['primaryCommandFailure'] = {'reason': error.reason, 'sequence': error.sequence}
    finally:
        if realm.status and bucket_attempted:
            try:
                # This namespace belongs to this disposable fixture from creation.
                # Storage API performs deletes; SQL only observes exact owned names/counts.
                names = json.loads(realm.sql("SELECT coalesce(json_agg(name ORDER BY name),'[]'::json) FROM storage.objects WHERE bucket_id='" + bucket + "'"))
                if not isinstance(names, list) or not all(isinstance(name, str) and (name.startswith('native-artifact-v1/') or name == 'qualification-positive.json') for name in names):
                    raise RuntimeError('Owned native fixture has unexpected object names')
                if names:
                    status, _ = request('/storage/v1/object/' + bucket, method='DELETE', body={'prefixes': names})
                    if status != 200:
                        raise RuntimeError('Owned native object cleanup failed')
                if realm.sql("SELECT count(*) FROM storage.objects WHERE bucket_id='" + bucket + "'") != '0':
                    raise RuntimeError('Owned native object metadata survived cleanup')
                result['objectsCleanupVerified'] = True
                status, _ = request('/storage/v1/bucket/' + bucket, method='DELETE')
                if status not in (200, 404):
                    raise RuntimeError('Owned native bucket cleanup failed')
                if realm.sql("SELECT count(*) FROM storage.buckets WHERE id='" + bucket + "'") != '0':
                    raise RuntimeError('Owned native bucket metadata survived cleanup')
                result['bucketCleanupVerified'] = True
            except BaseException as error:
                result['fixtureCleanupFailureType'] = type(error).__name__
        try:
            realm.__exit__()
            result['environmentCleanupVerified'] = realm.cleanup_verified
        except BaseException as error:
            result['environmentCleanupFailureType'] = type(error).__name__
        result['configPresenceStable'] = config_presence == {str(path): path.exists() for path in config_paths}
        result['configPresence'] = config_presence
        result['sourceMembershipStable'] = result['configPresenceStable'] and membership == {str(tree): sorted(str(path.relative_to(tree)) for path in tree.rglob('*') if path.is_file()) for tree in watched_roots}
        result['sourceMembership'] = membership
        result['sourcePinsStable'] = result['sourceMembershipStable'] and all(hashlib.sha256(Path(path).read_bytes()).hexdigest() == digest for path, digest in pins.items())
        result['sourcePins'] = pins
        result['runtime'] = realm.runtime
        result['migrationManifest'] = realm.manifest
        result['privateStage'] = str(realm.stage)
        result['passed'] = primary is None and all(result.get(key) is True for key in (
            'primaryPassed', 'objectsCleanupVerified', 'bucketCleanupVerified', 'environmentCleanupVerified', 'sourcePinsStable'))
        private_write(result_path, (json.dumps(result, indent=2) + '\n').encode())
    print(json.dumps({key: result[key] for key in ('passed','objectsCleanupVerified','bucketCleanupVerified','environmentCleanupVerified','sourcePinsStable')}), flush=True)
    if not result['passed']:
        raise RuntimeError('Owned native artifact qualification failed; separate evidence retained privately') from None


if __name__ == '__main__':
    main()
