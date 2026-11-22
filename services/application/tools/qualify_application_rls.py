"""Qualify current whole-layer grants and real Auth/PostgREST reads in an owned realm."""
import argparse
import base64
import hashlib
import json
import os
import tomllib
from pathlib import Path
import urllib.request
import uuid

from disposable_supabase import DisposableSupabase, ROOT, private_write
from qualification_http import make_opener, request_json, object_json


def main() -> None:
    """Create only a disposable local project; retain evidence and verify its cleanup."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--api-import',action='store_true',help='Also execute the Rust import HTTP boundary against this owned realm')
    parser.add_argument('--native-preparation',action='store_true',help='Also execute the Rust preparation readers against live owned RPCs')
    parser.add_argument('--temporal-preparation',action='store_true',help='With --api-import, compose managed source preparation with an owned local Temporal queue (no worker)')
    options=parser.parse_args()
    if options.temporal_preparation and not options.api_import:
        parser.error('--temporal-preparation requires --api-import')
    # Configured webhook handlers need actual private evidence storage. No mail,
    # Temporal worker, browser/client or external provider is started here.
    realm = DisposableSupabase(storage=options.api_import)
    result: dict = {'scope': 'whole-application-rls-managed-local', 'passed': False, 'cleanupVerified': False}
    failure = None
    rust_sources = options.api_import or options.native_preparation
    source_paths = [Path(__file__).resolve(), ROOT / 'tools/disposable_supabase.py', ROOT / 'tools/loopback_docker.py', ROOT / 'tools/qualification_http.py', ROOT / 'tools/native_source_reader_acceptance.py', ROOT / 'tools/native_boundary_rls_acceptance.py', ROOT / 'tests/fixtures/rls-function-capabilities.json', ROOT / 'tests/fixtures/rls-relation-capabilities.json']
    source_trees = [ROOT / 'supabase/migrations']
    source_candidates: list[Path] = []
    if options.api_import:
        source_paths += [ROOT / 'tools/api_import_acceptance.py', ROOT / 'tools/human_api_acceptance.py', ROOT / 'tools/legacy_api_acceptance.py', ROOT / 'tools/economy_api_acceptance.py', ROOT / 'tools/prediction_api_acceptance.py', ROOT / 'tools/temporal_api_acceptance.py', ROOT / 'tools/provider_api_acceptance.py', ROOT / 'tools/benefit_projection_acceptance.py', ROOT / 'tools/deck_defaults_api_acceptance.py', ROOT / 'tools/qualify_stripe.py', ROOT / 'tests/fixtures/patreon-source.json', ROOT / 'tools/quest_reroll_api_acceptance.py', ROOT / 'tools/native_receipt_api_acceptance.py']
    if rust_sources:
        repository = ROOT.parents[1]
        source_paths += [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', ROOT / 'build.rs', repository / 'Cargo.toml', repository / 'rust-toolchain.toml']
        source_trees += [ROOT / 'src', ROOT / 'build_support', ROOT / 'tests/fixtures']
        # Resolve all actual local dependencies instead of assuming contracts is
        # the only path package. Tests do not thereby qualify engine behavior.
        metadata = json.loads(realm.command(['cargo', 'metadata', '--locked', '--offline', '--format-version', '1', '--manifest-path', str(ROOT / 'Cargo.toml')]))
        for package in metadata['packages']:
            if package['source'] is not None:
                continue
            manifest = Path(package['manifest_path'])
            source_paths.append(manifest)
            source_trees.append(manifest.parent / 'src')
            source_candidates.append(manifest.parent / 'build.rs')
        source_candidates += [repository / '.cargo/config', repository / '.cargo/config.toml', ROOT / '.cargo/config', ROOT / '.cargo/config.toml']
    def hash_sources() -> dict[str, str]:
        """Recollect membership as well as bytes at capture and drift verification."""
        selected = set(source_paths)
        selected.update(path for tree in source_trees for path in tree.rglob('*') if path.is_file())
        selected.update(path for path in source_candidates if path.is_file())
        return {os.path.relpath(path, ROOT): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(selected)}

    source_hashes = hash_sources()
    result['sourceHashes'] = source_hashes
    try:
        if rust_sources:
            rustc = realm.command(['rustc', '--version', '--verbose']).decode().strip()
            cargo = realm.command(['cargo', '--version']).decode().strip()
            toolchain = tomllib.loads((ROOT.parents[1] / 'rust-toolchain.toml').read_text())['toolchain']['channel']
            if rustc.splitlines()[0].split()[1] != toolchain:
                raise RuntimeError('Owned API compiler differs from repository toolchain')
            result['buildToolchain'] = {'rustc': rustc, 'cargo': cargo, 'expectedChannel': toolchain,
                                      'workingDirectory': str(realm.stage)}
            if options.api_import:
                realm.command(['cargo','build','--locked','--manifest-path',str(ROOT/'Cargo.toml'),'--bin','application_api'],timeout=180)
        realm.__enter__()
        base = realm.status['API_URL']
        public = realm.status['PUBLISHABLE_KEY']
        secret = realm.status['SECRET_KEY']
        opener = make_opener()
        checks: list[dict] = []

        def request(path: str, bearer: str | None = None, body: dict | None = None,
                    *, admin: bool = False, method: str | None = None) -> tuple[int, object]:
            if not path.startswith(('/auth/v1/', '/rest/v1/')) or '..' in path:
                raise RuntimeError('Unexpected owned request path')
            headers = {'Content-Type': 'application/json', 'apikey': secret if admin else public,
                       'Accept-Profile': 'app', 'Content-Profile': 'app'}
            if bearer is not None:
                headers['Authorization'] = 'Bearer ' + bearer
            return request_json(opener, urllib.request.Request(base + path, headers=headers, method=method,
                data=None if body is None else json.dumps(body).encode()))

        def expect(label: str, response: tuple[int, object], status: int, value: object) -> None:
            actual, payload = response
            checks.append({'case': label, 'status': actual, 'passed': actual == status and payload == value})
            if actual != status or payload != value:
                private_write(realm.stage / ('private-rls-error-' + str(len(checks)) + '.json'), json.dumps(payload).encode())
                raise RuntimeError('Managed RLS case failed: ' + label)

        history = json.loads(realm.sql("SELECT jsonb_agg(version ORDER BY version) FROM supabase_migrations.schema_migrations"))
        if history != [item['file'].split('_', 1)[0] for item in realm.manifest]:
            raise RuntimeError('Current migration manifest differs')
        capabilities = json.loads(realm.sql("SELECT jsonb_agg(jsonb_build_object('name',p.proname,'arguments',pg_get_function_identity_arguments(p.oid),'roles',"
            "(SELECT coalesce(jsonb_agg(name::text ORDER BY name),'[]'::jsonb) FROM unnest(ARRAY['anon','authenticated','service_role','supabase_auth_admin']::name[]) name WHERE has_function_privilege(name,p.oid,'EXECUTE')))) "
            "FROM pg_proc p WHERE p.pronamespace='app'::regnamespace"))
        exposed = sorted([item for item in capabilities if item['roles']], key=lambda item: (item['name'], item['arguments']))
        expected = json.loads((ROOT / 'tests/fixtures/rls-function-capabilities.json').read_text())
        if exposed != expected:
            raise RuntimeError('Managed function capability matrix differs')
        forbidden = realm.sql("SELECT count(*) FROM pg_class c CROSS JOIN unnest(ARRAY['anon','authenticated','service_role','supabase_auth_admin']::name[]) role_name "
            "WHERE c.relnamespace='app'::regnamespace AND c.relkind IN('r','v') AND (has_table_privilege(role_name,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER,MAINTAIN') OR EXISTS(SELECT 1 FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped AND (has_column_privilege(role_name,c.oid,a.attnum,'INSERT') OR has_column_privilege(role_name,c.oid,a.attnum,'UPDATE') OR has_column_privilege(role_name,c.oid,a.attnum,'REFERENCES'))))")
        if forbidden != '0':
            raise RuntimeError('Managed raw table/column mutations remain available')
        result['functionCapabilities'] = exposed
        relation_matrix = json.loads(realm.sql("SELECT jsonb_agg(jsonb_build_object('relation',c.relname,'kind',CASE WHEN c.relkind='r' THEN 'table' ELSE 'view' END,'reads',"
            "(SELECT jsonb_object_agg(role_name::text,(SELECT coalesce(jsonb_agg(a.attname ORDER BY a.attnum),'[]'::jsonb) FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped AND has_column_privilege(role_name,c.oid,a.attnum,'SELECT'))) "
            "FROM unnest(ARRAY['anon','authenticated','service_role','supabase_auth_admin']::name[]) role_name)) ORDER BY c.relname) FROM pg_class c WHERE c.relnamespace='app'::regnamespace AND c.relkind IN('r','v')"))
        expected_relations = json.loads((ROOT / 'tests/fixtures/rls-relation-capabilities.json').read_text())
        if relation_matrix != sorted(expected_relations, key=lambda item: item['relation']):
            raise RuntimeError('Managed relation/column read capability matrix differs')
        result['relationCapabilities'] = relation_matrix
        result['elevatedFunctions'] = json.loads(realm.sql("SELECT jsonb_agg(jsonb_build_object('name',p.proname,'arguments',pg_get_function_identity_arguments(p.oid),'owner',r.rolname,'superuser',r.rolsuper,'bypassRls',r.rolbypassrls,'definer',p.prosecdef,'configuration',p.proconfig) ORDER BY p.proname,pg_get_function_identity_arguments(p.oid)) FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.pronamespace='app'::regnamespace"))
        if any(item['definer'] and not any(value in ('search_path=', 'search_path=""') for value in (item['configuration'] or [])) for item in result['elevatedFunctions']):
            raise RuntimeError('Managed definer search path differs')
        ids = [str(uuid.uuid4()) for _ in range(3)]
        for identity, kind in zip(ids, ('back', 'sleeve', 'dice')):
            realm.sql("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES('" + identity + "','" + kind + "','fixture:free','active',true)")

        def authenticate(email: str) -> tuple[str, dict]:
            """Redeem an actual local one-time code without registering an app account."""
            status, generated = request('/auth/v1/admin/generate_link', body={'type': 'magiclink', 'email': email}, admin=True)
            if status != 200:
                raise RuntimeError('Owned code generation failed')
            generated = object_json(generated)
            properties = object_json(generated.get('properties', generated))
            status, signed = request('/auth/v1/verify', body={'email': email, 'token': properties['email_otp'], 'type': properties['verification_type']})
            if status != 200:
                raise RuntimeError('Owned actual code redemption failed')
            token = object_json(signed)['access_token']
            claims = json.loads(base64.urlsafe_b64decode(token.split('.')[1] + '=' * (-len(token.split('.')[1]) % 4)))
            return token, claims

        def login(email: str, username: str) -> tuple[str, dict, str]:
            token, claims = authenticate(email)
            subject, session = str(uuid.UUID(claims['sub'])), str(uuid.UUID(claims['session_id']))
            status, registered = request('/rest/v1/rpc/register_account_command', body={'p_subject': subject, 'p_session': session,
                'p_username': username, 'p_back': ids[0], 'p_sleeve': ids[1], 'p_dice': ids[2]}, admin=True)
            if status != 200 or object_json(registered).get('status') != 'success':
                private_write(realm.stage / ('private-registration-' + username + '.json'), json.dumps(registered).encode())
                raise RuntimeError('Owned registration through verified managed session failed')
            actor = str(uuid.UUID(object_json(object_json(registered)['data'])['accountId']))
            return token, claims, actor

        owner_token, owner_claims, owner = login('rls-owner@example.invalid', 'RlsOwner')
        other_token, _, peer = login('rls-other@example.invalid', 'RlsOther')
        from native_source_reader_acceptance import prepare_source_reader_fixture, prepare_source_reader_reservation
        reservation = prepare_source_reader_reservation(realm, owner, peer)
        reserved_game = reservation['gameId']
        admission_rpc = '/rest/v1/rpc/read_native_admission_source'
        admission_body = {'p_game': reserved_game}
        admission_source = object_json(json.loads(realm.sql(f"SELECT app.read_native_admission_source('{reserved_game}')")))
        if admission_source.get('status') != 'success':
            raise RuntimeError('Owned reserved admission source is unavailable')
        expect('native-admission-service-source', request(admission_rpc, body=admission_body, admin=True), 200, admission_source)
        expect('native-admission-service-unavailable', request(admission_rpc, body={'p_game':str(uuid.uuid4())}, admin=True), 200, {'status':'fail','data':{'game':'unavailable'}})
        for label, bearer in [('anonymous',None),('owner',owner_token),('stranger',other_token)]:
            status, _ = request(admission_rpc,bearer,admission_body)
            checks.append({'case':'native-admission-denied-' + label,'status':status,'passed':status in (401,403,404)})
            if not checks[-1]['passed']:
                raise RuntimeError('Managed admission source RPC exposed to ' + label)
        game, native_source = prepare_source_reader_fixture(realm, owner, peer)
        native_rpc = '/rest/v1/rpc/read_native_game_source'
        native_body = {'p_game': game}
        expect('native-source-service-frontier', request(native_rpc, body=native_body, admin=True), 200, native_source)
        expect('native-source-service-unavailable', request(native_rpc, body={'p_game':str(uuid.uuid4())}, admin=True), 200, {'status':'fail','data':{'game':'unavailable'}})
        for label, bearer in [('anonymous',None),('owner',owner_token),('stranger',other_token)]:
            status, _ = request(native_rpc,bearer,native_body)
            checks.append({'case':'native-source-denied-' + label,'status':status,'passed':status in (401,403,404)})
            if not checks[-1]['passed']:
                raise RuntimeError('Managed native source unexpectedly allowed ' + label)
        result['nativeSourceRpc']={'passed':True,'actualAcceptedOpening':True,'nativeBytesQualified':False}
        from native_boundary_rls_acceptance import qualify_native_boundaries
        result['nativeBoundaryRpc'] = qualify_native_boundaries(realm,request,expect,owner,peer,owner_claims,owner_token,other_token)
        if rust_sources:
            room_source = prepare_source_reader_reservation(realm,owner,peer,start_match=False)
            fixture = realm.stage / 'private-native-preparation-input.json'
            private_write(fixture,json.dumps({'scope':'spellwake-owned-native-preparation-v1','base':base,'secret':secret,
                'room':{'roomId':room_source['roomId'],'basis':room_source['roomRevision'],'releaseId':room_source['releaseId'],'legalityId':room_source['legalityId']},
                'between':result['nativeBoundaryRpc']['pendingBetweenQuery'],'reservedGameId':reserved_game,'openingGameId':next(sample['request']['p_game'] for sample in result['nativeBoundaryRpc']['wireSamples'] if sample['operation']=='read_native_opening_source')}).encode())
            try:
                realm.command(['cargo','test','--locked','--manifest-path',str(ROOT/'Cargo.toml'),
                    '--lib','native_game::preparation_source::live::managed_preparation_sources_use_actual_postgrest','--','--ignored'],
                    timeout=180,extra_environment={'SPELLWAKE_MANAGED_PREPARATION_INPUT':str(fixture)})
            finally:
                fixture.unlink(missing_ok=True)
            result['nativePreparationRustRpc']={'passed':True,'positiveFamilies':['room','between','reserved','opening'],
                'unavailableFamilies':['room','between','reserved','opening'],'actualRustHttpToManagedPostgrest':True,
                'sqlMetadataOnly':True,'nativeBytesQualified':False,'providerExecutionQualified':False}

        definition = str(uuid.uuid4())
        realm.sql("INSERT INTO app.quest_definition_revision(id,definition_key,revision,objective_ref,target,reward_coins,fact_kind,fact_schema_revision,objective_qualification_ref) "
            "VALUES('" + definition + "','" + definition + "','fixture','fixture:objective',3,500,'fixture:casts','fixture:v1','fixture:qualified')")
        assigned = json.loads(realm.sql("SELECT app.generate_daily_quest('" + owner + "','" + definition + "',to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD'),'" + str(uuid.uuid4()) + "')"))
        assignment = str(uuid.UUID(assigned['data']['resourceId']))
        path = '/rest/v1/quest_assignment?select=id&account_id=eq.' + owner
        expect('owner-quest', request(path, owner_token), 200, [{'id': assignment}])
        expect('stranger-quest', request(path, other_token), 200, [])
        expect('owner-progress-view', request('/rest/v1/quest_progress?select=id&assignment_id=eq.' + assignment, owner_token), 200, [{'id': assignment}])
        expect('stranger-progress-view', request('/rest/v1/quest_progress?select=id&assignment_id=eq.' + assignment, other_token), 200, [])
        for label, response in [('anonymous', request(path)), ('private-column', request('/rest/v1/quest_assignment?select=reward_source_fact_id', owner_token)),
                                ('raw-write', request('/rest/v1/quest_assignment', owner_token, {})),
                                ('private-function', request('/rest/v1/rpc/generate_daily_quest', owner_token, {}))]:
            checks.append({'case': label, 'status': response[0], 'passed': response[0] in (400, 401, 403, 404)})
            if not checks[-1]['passed']:
                raise RuntimeError('Managed boundary unexpectedly allowed ' + label)
        if options.api_import:
            from api_import_acceptance import qualify_import_api
            result['apiImport'] = qualify_import_api(realm,owner_token,other_token,owner,ids,
                lambda: authenticate('rls-api-' + uuid.uuid4().hex + '@example.invalid')[0],
                temporal_preparation=options.temporal_preparation,native_receipt_fixture=result['nativeBoundaryRpc'])
            # Current PostgREST void functions return204 without a JSON body.
            # This aged synthetic metadata never represents an uploaded object
            # or real provider evidence; it qualifies only the trusted RPC wire.
            event = str(uuid.uuid4())
            digest = 'a' * 64
            capture = json.dumps({'version':1,'kind':'webhook','bodyHash':digest,
                'apiVersion':'2026-10-28.endive','live':False,'targetId':'cs_fixture_void'})
            realm.sql("INSERT INTO app.provider_event(id,provider,external_event_ref,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state) VALUES('"+event+"','stripe','evt_"+event.replace('-','')+"',clock_timestamp()-interval '31 days','fixture.unsupported','payments/stripe/"+digest+".json','received','"+capture+"'::jsonb,'expired')")
            fixture = realm.stage / 'private-void-rpc-input.json'
            private_write(fixture, json.dumps({'scope':'spellwake-owned-managed-void-v1',
                'base':base,'secret':secret,'eventId':event}).encode())
            try:
                realm.command(['cargo','test','--locked','--manifest-path',str(ROOT/'Cargo.toml'),
                    '--lib','backend::tests::managed_void_rpc_uses_actual_postgrest_no_content','--','--ignored'],
                    timeout=180,extra_environment={'SPELLWAKE_MANAGED_VOID_RPC_INPUT':str(fixture)})
            finally:
                fixture.unlink(missing_ok=True)
            if realm.sql("SELECT raw_deleted_at IS NOT NULL FROM app.provider_event WHERE id='"+event+"'") != 't':
                raise RuntimeError('Owned void RPC did not record its canonical metadata')
            result['voidRpc']={'passed':True,'protocol':'actual204-no-content',
                'canonicalMetadataRecorded':True,'nativeOrProviderQualified':False}
        parts = owner_token.split('.')
        signature = bytearray(base64.urlsafe_b64decode(parts[2] + '=' * (-len(parts[2]) % 4)))
        signature[0] ^= 1
        forged = '.'.join([parts[0], parts[1], base64.urlsafe_b64encode(signature).decode().rstrip('=')])
        if request(path, forged)[0] != 401:
            raise RuntimeError('Gateway accepted altered JWT signature')
        realm.sql("UPDATE auth.sessions SET not_after=clock_timestamp()-interval '1 second' WHERE id='" + str(uuid.UUID(owner_claims['session_id'])) + "'")
        expect('expired-managed-session', request(path, owner_token), 200, [])
        expect('expired-session-catalogue', request('/rest/v1/cosmetic?select=id', owner_token), 200, [])
        if [hashlib.sha256((ROOT / 'supabase/migrations' / item['file']).read_bytes()).hexdigest() for item in realm.manifest] != [item['sha256'] for item in realm.manifest]:
            raise RuntimeError('Source changed during managed acceptance')
        if source_hashes != hash_sources():
            raise RuntimeError('RLS harness or capability evidence changed during acceptance')
        result.update(passed=True, checks=checks, migrations=realm.manifest, runtime=realm.runtime)
    except BaseException as error:
        failure = error
        result['failureType'] = type(error).__name__
        private_write(realm.stage / 'private-rls-failure.txt', str(error).encode())
    finally:
        try:
            realm.__exit__()
            result['cleanupVerified'] = realm.cleanup_verified
        except BaseException as error:
            result['cleanupFailure'] = type(error).__name__
        private_write(realm.stage / 'whole-rls-acceptance.json', json.dumps(result, indent=2).encode())
    print('Owned RLS evidence: ' + str(realm.stage / 'whole-rls-acceptance.json'))
    if failure is not None or not result['cleanupVerified']:
        raise RuntimeError('Owned whole-RLS acceptance failed; private diagnostics retained') from None


if __name__ == '__main__':
    main()
