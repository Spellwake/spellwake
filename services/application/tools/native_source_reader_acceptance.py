"""Canonical SQL metadata fixture for the private native-source RPC.

Opaque fixture artifacts and release labels qualify RPC provenance/ACLs only,
not native rules, persistent object bytes or playable deck admission.
"""
import json
from datetime import datetime, timezone
import uuid
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def prepare_source_reader_reservation(realm: DisposableSupabase, owner: str, peer: str,
                                      *, oracle_ref: str | None = None, quantity: int = 2, start_match: bool = True, structure: str = 'bo1', unlimited_basic_land: bool = True, mixed_ai: bool = False, mode: str = 'open', additional_oracle_ref: str | None = None, additional_quantity: int = 0, third_human: str | None = None, competitive: bool = False, release_content: dict | None = None, publication: dict | None = None, candidate_command: list[str] | None = None, admission_api: dict | None = None) -> dict:
    """Use maintained deck/room/admission/opening producers in the owned realm."""
    if competitive and (mode != 'collection' or mixed_ai):
        raise ValueError('Ranked fixture requires human Collection participants')
    if mode not in ('open', 'collection'):
        raise ValueError('Invalid fixture collection mode')
    if structure not in ('bo1','bo3'):
        raise ValueError("Invalid native fixture structure")
    if type(mixed_ai) is not bool or mixed_ai and not start_match:
        raise ValueError("Mixed controller fixture requires an actually started reservation")
    if type(unlimited_basic_land) is not bool:
        raise ValueError("Invalid basic-land fixture metadata")
    owner, peer = str(uuid.UUID(owner)), str(uuid.UUID(peer))
    people = (owner, peer) if third_human is None else (owner, peer, str(uuid.UUID(third_human)))
    if len(set(people)) != len(people) or third_human is not None and mixed_ai:
        raise ValueError("Expected distinct human participants")
    if type(quantity) is not int or not 1 <= quantity <= 1000:
        raise ValueError("Invalid native fixture deck quantity")
    format_id, profile, definition, card_set, printing, body_revision, release, legality = (
        str(uuid.uuid4()) for _ in range(8))

    format_key = "legacy" if third_human is not None or admission_api is not None else format_id
    if type(start_match) is not bool:
        raise ValueError("Invalid native fixture phase")
    revisions: dict[str, str] = {}

    oracle = definition if oracle_ref is None else str(uuid.UUID(oracle_ref))
    extra = None
    if additional_oracle_ref is not None:
        extra = {'oracle': str(uuid.UUID(additional_oracle_ref)), 'definition': str(uuid.uuid4()),
                 'printing': str(uuid.uuid4()), 'revision': str(uuid.uuid4())}
        if extra['oracle'] == oracle or type(additional_quantity) is not int or not 1 <= additional_quantity <= 1000:
            raise ValueError('Invalid additional controlled native card')
    elif additional_quantity != 0:
        raise ValueError('Additional quantity requires an exact oracle identity')


    scope = (admission_api or {}).get('qualifiedScope',{})
    if scope:
        from scoped_evidence import identity
        assert publication and release_content and structure=='bo3' and len(people)==2 and competitive and not mixed_ai
        release=identity('release',[publication['publicationId'],scope,release_content['releaseContent']])
        format_id=identity('format','legacy')
        profile=identity('profile',scope['profileEvidence'])
        legality=identity('legality',scope['legalitySource'])
    release_refs = {k:'fixture' for k in ('kernelRef','rulesRef','programManifestRef')}
    fallback_card = {'definitionRevisionRef':'fixture','rulesTextRef':'fixture:sql-only','programRef':'fixture:sql-only'}
    def selected_refs(oracle_id: str) -> dict:
        if release_content is None:
            return fallback_card
        refs, = [row['sourceRefs'] for row in release_content['bindings'] if row['oracleRef']==oracle_id]
        return refs
    card_refs = selected_refs(oracle)
    extra_refs = selected_refs(extra['oracle']) if extra else None
    if release_content is not None:
        release_refs = release_content['releaseContent']
        for ref in [*release_refs.values(), *card_refs.values(), *(extra_refs or {}).values()]:
            if len(ref)!=71 or not ref.startswith('sha256:') or any(c not in '0123456789abcdef' for c in ref[7:]):
                raise ValueError('Expected content-addressed native source')
    def reuse_definition(oracle_id: str, proposed_definition: str, proposed_revision: str, refs: dict) -> tuple[str, str]:
        """Reuse the same controlled immutable card across owned match fixtures."""
        existing = realm.sql(f"SELECT coalesce((SELECT id::text FROM app.card_definition WHERE oracle_ref='{oracle_id}'),'')")
        if not existing:
            return proposed_definition, proposed_revision
        selected = str(uuid.UUID(existing))
        revision = realm.sql(f"SELECT coalesce((SELECT id::text FROM app.card_definition_revision WHERE definition_id='{selected}' AND revision_ref='{refs['definitionRevisionRef']}' AND rules_text_ref='{refs['rulesTextRef']}'),'')")
        return selected, str(uuid.UUID(revision)) if revision else proposed_revision

    if publication is not None:
        from qualify_catalogue_match import selected_card
        card_refs = selected_card(realm, publication, release, release_content, oracle)
        definition, body_revision, printing = (card_refs[k] for k in ('definition', 'revision', 'printing'))
        if extra is not None:
            extra_refs = selected_card(realm, publication, release, release_content, extra['oracle'])
            extra.update({k: extra_refs[k] for k in ('definition', 'revision', 'printing')})
    else:
        definition, body_revision = reuse_definition(oracle, definition, body_revision, card_refs)
        if extra is not None:
            extra['definition'], extra['revision'] = reuse_definition(extra['oracle'], extra['definition'], extra['revision'], extra_refs)

    def sql_json(statement: str) -> dict:
        return object_json(json.loads(realm.sql(statement)))

    def accepted(statement: str) -> dict:
        response = sql_json(statement)
        if response.get('status') != 'success':
            raise RuntimeError('Native source SQL fixture producer did not accept')
        return object_json(response['data'])

    def literal(value: object) -> str:
        return "'" + json.dumps(value).replace("'", "''") + "'::jsonb"

    release_evidence = profile_evidence = 'fixture:sql-only'
    execute_http = None
    if admission_api is not None:
        if publication is None or extra is None or not competitive:
            raise ValueError('HTTP admission qualification requires published ranked decks')
        from qualify_admission_api import start_room_api
        release_evidence, profile_evidence, execute_http = start_room_api(realm,admission_api,release,legality,profile,format_id,
            format_key,[(oracle,card_refs),(extra['oracle'],extra_refs)],len(people))
    match_config = literal({'version':'matchmaking-profile-v1','influence':'none','planechase':False,
        'qualificationRef':profile_evidence,'configurations':[{'key':'constructed-duel','formatId':format_id,
        'seatCount':'2','structure':'bo3','teams':[None,None]}]}) if scope else 'NULL'
    realm.sql(f"""
      INSERT INTO app.format(id,key,name) VALUES('{format_id}','{format_key}','Native RPC fixture');
      INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config)
        VALUES('{profile}','{profile}','{scope.get("profileRevision","fixture")}','{scope.get("profileRules","fixture")}',2,{len(people)},'{profile_evidence}',{match_config});
    """)
    if publication is None:
        realm.sql(f"""
      INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('{definition}','{oracle}','Native RPC fixture') ON CONFLICT(id) DO NOTHING;
      INSERT INTO app.card_set(id,external_set_ref,name) VALUES('{card_set}','{card_set}','Native RPC fixture');
      INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref)
        VALUES('{printing}','{definition}','{card_set}','{printing}','common','nonfoil','fixture:sql-only');
      INSERT INTO app.card_definition_revision(id,definition_id,revision_ref,rules_text_ref)
        VALUES('{body_revision}','{definition}','{card_refs['definitionRevisionRef']}','{card_refs['rulesTextRef']}') ON CONFLICT(id) DO NOTHING;
        """)
    realm.sql(f"""
      INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref)
        VALUES('{release}','{release}','{release_refs['kernelRef']}','{release_refs['rulesRef']}','{release_refs['programManifestRef']}','{release_evidence}');
      INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land)
        VALUES('{release}','{body_revision}','{card_refs['programRef']}',{'true' if unlimited_basic_land else 'false'});
      INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at)
        VALUES('{legality}','{scope.get("legalitySource","fixture:sql-only")}',clock_timestamp(),clock_timestamp());
      INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status)
        VALUES('{definition}','{format_id}','{legality}','legal');
    """)
    if extra is not None:
        # Controlled second source for actual selected native cast qualification;
        # no public release/card/format claim and no unlimited nonbasic shortcut.
        if publication is None:
            realm.sql(f"""
          INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('{extra['definition']}','{extra['oracle']}','Native cast RPC fixture') ON CONFLICT(id) DO NOTHING;
          INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref)
            VALUES('{extra['printing']}','{extra['definition']}','{card_set}','{extra['printing']}','common','nonfoil','fixture:sql-only');
          INSERT INTO app.card_definition_revision(id,definition_id,revision_ref,rules_text_ref)
            VALUES('{extra['revision']}','{extra['definition']}','{extra_refs['definitionRevisionRef']}','{extra_refs['rulesTextRef']}') ON CONFLICT(id) DO NOTHING;
            """)
        realm.sql(f"""
          INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land)
            VALUES('{release}','{extra['revision']}','{extra_refs['programRef']}',false);
          INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status)
            SELECT '{extra['definition']}','{format_id}','{legality}','legal' WHERE {'false' if execute_http is not None else 'true'};
        """)
    room = str(uuid.UUID(accepted(f"SELECT app.create_room('{owner}','{format_id}','{profile}','{mode}',{str(competitive).lower()},'public','{uuid.uuid4()}')")['resourceId']))

    def basis() -> str:
        value = realm.sql(f"SELECT revision::text FROM app.room WHERE id='{room}'")
        if not value.isascii() or not value.isdecimal():
            raise RuntimeError('Fixture room revision is not decimal')
        return value

    for player in ((owner,) if mixed_ai else people):
        deck = str(uuid.UUID(accepted(f"SELECT app.create_deck('{player}','Native RPC fixture','{mode}','{format_id}','{uuid.uuid4()}')")['resourceId']))
        entries = [{'definitionId': definition, 'printingId': printing, 'section': 'main', 'quantity': str(quantity)}]
        if competitive:
            entries.append({'definitionId': definition, 'printingId': printing, 'section': 'sideboard', 'quantity': '4'})
        if extra is not None:
            entries.append({'definitionId': extra['definition'], 'printingId': extra['printing'], 'section': 'main', 'quantity': str(additional_quantity)})
            if mode == 'collection':
                # Explicit owned-fixture inventory funding, separate from native
                # cast truth. Preserve the real finite-card ownership check.
                operation = str(uuid.uuid4())
                funded_quantity = additional_quantity * (2 if mixed_ai else 1)
                realm.sql(f"""
                  INSERT INTO app.asset_definition(kind,printing_id) VALUES('card','{extra['printing']}') ON CONFLICT(printing_id) DO NOTHING;
                  INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state)
                    VALUES('{operation}','fixture','{operation}','fixture','{player}',clock_timestamp(),'pending');
                  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
                    SELECT '{operation}',app.ensure_inventory(o.id,d.id),'credit',{funded_quantity},'fixture','{operation}'
                    FROM app.asset_owner o CROSS JOIN app.asset_definition d WHERE o.account_id='{player}' AND o.state='active' AND d.printing_id='{extra['printing']}';
                  UPDATE app.ledger_operation SET state='committed' WHERE id='{operation}';
                """)

        revision = str(uuid.UUID(accepted(f"SELECT app.save_deck_contents('{player}','{deck}',NULL,{literal(entries)},'{uuid.uuid4()}')")['resourceId']))
        revisions[player] = revision
        if player != owner:
            accepted(f"SELECT app.join_room('{player}','{room}',NULL,'{uuid.uuid4()}')")
        accepted(f"SELECT app.select_room_deck('{player}','{room}',{basis()},'{revision}','{uuid.uuid4()}')")
    if mixed_ai:
        # Canonical self-sponsored room slot; no human/AI roster substitution.
        slot = str(uuid.UUID(accepted(f"SELECT app.add_room_ai_slot('{owner}','{room}',{basis()},'{uuid.uuid4()}')")['resourceId']))
    if structure == 'bo3':
        teams = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('slotId',id,'teamRef',NULL) ORDER BY seat_index) FROM app.room_slot WHERE room_id='{room}' AND retired_at IS NULL"))
        accepted(f"SELECT app.configure_room('{owner}','{room}',{basis()},'{profile}','bo3',{literal(teams)},'{uuid.uuid4()}')")
    if mixed_ai:
        boot = accepted(f"SELECT app.bootstrap_ai_slot('{owner}','{slot}',{basis()},'{revisions[owner]}','{uuid.uuid4()}')")
        binding, start = str(uuid.UUID(boot['resourceId'])), str(uuid.UUID(boot['startKey']))
        work = sql_json(f"SELECT app.read_ai_start_work('{binding}',1)")
        if boot.get('epoch') != '1' or work != {'bindingId':binding,'epoch':'1','startId':start,'startKey':start,'seatId':None}:
            raise RuntimeError('AI bootstrap startup work changed exact binding/epoch/start/room scope')
        accepted(f"SELECT app.observe_ai_provider('{binding}',1,'{start}','running','fixture:mixed-replay-process','fixture:mixed-provider-observation','{uuid.uuid4()}')")
        grant = str(uuid.UUID(accepted(f"SELECT app.issue_ai_grant('{binding}',1,'fixture:mixed-secret-locator-not-possession:{binding}','{uuid.uuid4()}')")['resourceId']))
        accepted(f"SELECT app.observe_ai_grant_delivery('{grant}',0,'delivered','fixture:mixed-delivery-observation','{uuid.uuid4()}')")
    if mixed_ai:
        accepted(f"SELECT app.commit_ai_ready_confirmation('{grant}',1,'{revisions[owner]}','fixture:deck-qualified','fixture:authentication-observation',(SELECT configuration_id FROM app.room WHERE id='{room}'),'{uuid.uuid4()}')")
    if not start_match:
        return {'roomId': room, 'roomRevision': basis(), 'releaseId': release,
                'legalityId': legality, 'profileId': profile, 'revisions': revisions}
    receipt_evidence = None
    if competitive:
        actor = sql_json(f"""SELECT json_build_object('subject',a.auth_subject,'session',s.id)
            FROM app.account a JOIN auth.sessions s ON s.user_id=a.auth_subject
            WHERE a.id='{owner}' ORDER BY s.created_at DESC, s.id LIMIT 1""")
        subject, session = (uuid.UUID(actor[key]) for key in ('subject','session'))
        receipts = []
        for kind in ('ready','countdown'):
            revision = basis()
            original = {'requestId':str(uuid.uuid4()),'expectedRevision':revision}
            if execute_http is not None:
                if kind == 'ready':
                    rejected = {missing_kind: {**original, 'requestId': str(uuid.uuid4())} for missing_kind in ('ready','countdown')}
                    for missing_kind, refused in rejected.items():
                        execute_http(room,missing_kind,refused,missing_legality=True)
                    if basis()!=revision:
                        raise RuntimeError('Missing database legality changed room state')
                    realm.sql(f"INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status) VALUES('{extra['definition']}','{format_id}','{legality}','legal')")
                    for missing_kind, refused in rejected.items():
                        execute_http(room,missing_kind,refused,missing_legality=True)
                ack = execute_http(room,kind,original)
            else:
                preparation_source = accepted(f"SELECT app.read_native_room_source('{room}',{revision},'{release}','{legality}')")
                prepared = {'version':f'native-{kind}-prepared-v1','sourceHash':preparation_source['sourceHash'],
                            'releaseId':release,'legalityRevisionId':legality,'qualificationRef':'fixture:sql-only'}
                if kind == 'ready':
                    prepared['selectedRevisionId'] = revisions[owner]
                ack = accepted(f"SELECT app.qualify_room_{kind}_command('{subject}','{session}','{room}',{literal(original)},{literal(prepared)})")
            receipts.append({'original':original,'ack':ack})
        receipt_evidence = {'room':room,'ready':receipts[0],'countdown':receipts[1]}
    else:
        accepted(f"SELECT app.start_room_countdown('{owner}','{room}',{basis()},'fixture:sql-only','{uuid.uuid4()}')")
    if scope:
        from urllib.request import Request
        from qualification_http import request_json, make_opener
        def start_rpc(request_id,revision):
            url=realm.status['API_URL']+'/rest/v1/rpc/start_validated_countdown'
            assert url.startswith('http://127.0.0.1:')
            args={'p_room':room,'p_expected':revision,'p_request':request_id}
            req=Request(url,data=json.dumps(args).encode(),headers={'Content-Type':'application/json','Content-Profile':'app',
                'Accept-Profile':'app','apikey':realm.status['SECRET_KEY']})
            status,outcome=request_json(make_opener(),req,timeout=60)
            if status!=200:raise ValueError(f'Validated countdown returned {status}: {outcome}')
            return outcome
        rejection={'status':'fail','data':{'room':'stale, not due or already reserved'}}
        early=str(uuid.uuid4())
        realm.sql(f"UPDATE app.room SET countdown_at=clock_timestamp()+interval '1 hour' WHERE id='{room}'")
        assert start_rpc(early,basis())==rejection
    realm.sql(f"UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='{room}'")
    copies = f"(SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:sql-only') ORDER BY seat_index) FROM app.room_slot WHERE room_id='{room}' AND retired_at IS NULL)"
    policy = 'fixture:sql-only'
    if release_content is not None:
        if execute_http is not None:
            # The public force-start already validated these decks. Carry its
            # server-owned proof into this exact room revision and release.
            request_id = str(uuid.UUID(receipts[1]['original']['requestId']))
            policy = realm.sql(f"""SELECT normalized_source->'prepared'->>'qualificationRef'
                FROM app.command_receipt WHERE scope='qualified_countdown_intent_v1' AND state='accepted'
                AND account_id='{owner}' AND target_ref='{room}' AND request_ref='{request_id}'
                AND response_payload->'data'->>'revision'='{basis()}'
                AND normalized_source->'prepared'->>'releaseId'='{release}'
                AND normalized_source->'constructedPolicy'->>'formatId'='{format_id}'
                AND normalized_source->'constructedPolicy'->>'legalityRevisionId'='{legality}'""")
        else:
            if not candidate_command:
                raise ValueError('Missing retained room candidate command')
            ref = realm.command([*candidate_command, room, basis(), release, legality], timeout=240, extra_environment={
                    'SPELLWAKE_SUPABASE_URL':realm.status['API_URL'], 'SPELLWAKE_SUPABASE_SECRET_KEY':realm.status['SECRET_KEY']})
            policy = ref.decode('ascii').strip()
        import re
        if not re.fullmatch(r'native-constructed-ffa-candidate-v1/legacy/sha256:[0-9a-f]{64}', policy):
            raise RuntimeError('Expected the server-derived ordinary room candidate reference')
    if scope:
        expected=basis()
        request_id=str(uuid.uuid4())
        assert realm.sql("SELECT array_agg(has_function_privilege(role,'app.start_validated_countdown(uuid,numeric,uuid)','EXECUTE') ORDER BY i)::text FROM unnest(ARRAY['anon','authenticated','supabase_auth_admin','service_role']) WITH ORDINALITY a(role,i)")=='{f,f,f,t}'
        realm.sql(f"""BEGIN;
          UPDATE app.format SET key='changed-countdown-format' WHERE id='{format_id}';
          DO $$ BEGIN BEGIN PERFORM app.start_validated_countdown('{room}',{expected},'{request_id}');
            RAISE EXCEPTION 'changed format accepted';
            EXCEPTION WHEN check_violation THEN IF SQLERRM<>'countdown deck policy changed' THEN RAISE; END IF;
          END; END $$;
          ROLLBACK;""")
        assert start_rpc(early,expected)==rejection
        assert realm.sql(f"SELECT count(*) FROM app.match WHERE room_id='{room}'")=='0'
        outcome=start_rpc(request_id,expected)
        assert outcome['status']=='success',outcome
        started=outcome['data']
        assert start_rpc(request_id,expected)==outcome
        assert start_rpc(str(uuid.uuid4()),expected)==rejection
        snapshots=json.loads(realm.sql(f"SELECT jsonb_agg(s.deck_snapshot_ref ORDER BY s.seat_index) FROM app.match_seat s WHERE s.match_id='{uuid.UUID(started['resourceId'])}'"))
        assert len(snapshots)==len(people) and len(set(snapshots))==len(people) and all(re.fullmatch(r'native-constructed-deck-v1/[a-f0-9-]{36}/[a-f0-9-]{36}/sha256:[a-f0-9]{64}',x) for x in snapshots)
        admission_api['validatedStart']={'requestId':request_id,'reserved':started,'snapshots':snapshots,'retryIdentical':True,'earlyOriginalRejectedAfterDue':True,'changedFormatRolledBack':True,'transport':'service PostgREST RPC'}
    else:
        started = accepted(f"SELECT app.start_room_match('{room}',{basis()},'{release}','{legality}','{policy}',{copies},'{uuid.uuid4()}')")
    matched, game = (str(uuid.UUID(started[key])) for key in ('resourceId', 'gameId'))
    if execute_http is not None:
        original_state = realm.sql(f"SELECT jsonb_build_array(r.revision,r.state,(SELECT count(*) FROM app.command_receipt WHERE scope IN ('qualified_ready_intent_v1','qualified_countdown_intent_v1','room_ready','room_countdown') AND target_ref=r.id::text)) FROM app.room r WHERE r.id='{room}'")
        for kind, saved in zip(('ready','countdown'), receipts, strict=True):
            execute_http(room,kind,saved['original'],saved['ack'])
        if original_state != realm.sql(f"SELECT jsonb_build_array(r.revision,r.state,(SELECT count(*) FROM app.command_receipt WHERE scope IN ('qualified_ready_intent_v1','qualified_countdown_intent_v1','room_ready','room_countdown') AND target_ref=r.id::text)) FROM app.room r WHERE r.id='{room}'"):
            raise RuntimeError('Post-start execution changed the accepted room or receipt count')
    if mixed_ai:
        adopted = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('grant',g.id,'epoch',b.binding_epoch::text,'revision',s.deck_revision_id,'configuration',r.configuration_id,'binding',b.id,'seat',seat.id,'sponsor',s.sponsor_id) ORDER BY g.id) FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id JOIN app.match_seat seat ON seat.id=b.match_seat_id WHERE seat.match_id='{matched}' AND g.state='active' AND g.binding_epoch=b.binding_epoch AND g.delivery_state='pending'"))
        if not isinstance(adopted,list) or len(adopted) != 1:
            raise RuntimeError('Mixed fixture must adopt exactly one current AI grant')
        for item in adopted:
            new_grant, selected_revision, configuration = (str(uuid.UUID(item[key])) for key in ('grant','revision','configuration'))
            epoch = item['epoch']
            if str(uuid.UUID(item['binding'])) != binding or str(uuid.UUID(item['sponsor'])) != owner or selected_revision != revisions[owner] or new_grant == grant:
                raise RuntimeError('Adoption changed original binding/sponsor/deck or reused room grant')
            str(uuid.UUID(item['seat']))
            if not isinstance(epoch,str) or not epoch.isascii() or not epoch.isdecimal() or epoch != '2':
                raise RuntimeError('Invalid canonical adopted epoch')
            accepted(f"SELECT app.observe_ai_grant_delivery('{new_grant}',0,'delivered','fixture:mixed-adopted-delivery','{uuid.uuid4()}')")
            accepted(f"SELECT app.commit_ai_ready_confirmation('{new_grant}',{epoch},'{selected_revision}','fixture:deck-qualified','fixture:authentication-observation','{configuration}','{uuid.uuid4()}')")
        if realm.sql(f"SELECT state='revoked' FROM app.controller_grant WHERE id='{grant}'") != 't':
            raise RuntimeError('Original room grant must be revoked by actual adoption')
    revision = realm.sql(f"SELECT revision::text FROM app.match WHERE id='{matched}'")
    if not revision.isascii() or not revision.isdecimal():
        raise RuntimeError('Fixture match revision is not decimal')
    seats = json.loads(realm.sql(f"SELECT jsonb_agg(id ORDER BY seat_index) FROM app.match_seat WHERE match_id='{matched}'"))
    return {**({'roomReceipts':receipt_evidence} if receipt_evidence else {}),'gameId':game,'matchId':matched,'releaseId':release,'profileId':profile,
            'contextId':str(uuid.uuid4()),'expectedMatchRevision':revision,'seatIds':seats,'structure':structure}


def prepare_source_reader_fixture(realm: DisposableSupabase, owner: str, peer: str) -> tuple[str, dict]:
    """Observe the existing opaque opening fixture for SQL/managed-access checks."""
    source = prepare_source_reader_reservation(realm, owner, peer)
    opening = {'contextId': source['contextId'], 'snapshotRef': 'fixture:context', 'contextRevision': 'fixture:v1',
               'stateRef': 'fixture:state', 'factsRef': 'fixture:facts', 'eventRef': 'fixture:event', 'qualificationRef': 'fixture:sql-only'}
    literal = "'" + json.dumps(opening).replace("'", "''") + "'::jsonb"
    accepted = object_json(json.loads(realm.sql(f"SELECT app.accept_reserved_game('{source['matchId']}',{source['expectedMatchRevision']},'{source['gameId']}','fixture:source-rpc',{literal})")))
    if accepted.get('status') != 'success':
        raise RuntimeError('Fixture native opening did not accept')
    expected = object_json(json.loads(realm.sql(f"SELECT app.read_native_game_source('{source['gameId']}')")))
    if expected.get('status') != 'success' or object_json(expected['data']).get('contextId') != opening['contextId']:
        raise RuntimeError('Fixture native source frontier is unavailable')
    return source['gameId'], expected


def qualify_initial_clock_recovery(realm: DisposableSupabase, reservation: dict, opening: dict) -> dict:
    """Exercise actual SQL incident accounting after genuine native initialization.

    The caller has retained/accepted real native bytes. SQL is the trusted incident
    boundary here; subsequent Rust phases cold-recover those same original bytes.
    This is not native timeout, complete event or release/admission qualification.
    """
    game = str(uuid.UUID(reservation['gameId']))
    matched = str(uuid.UUID(reservation['matchId']))
    chooser = str(uuid.UUID(object_json(opening['clockObservation'])['seatId']))

    def success(statement: str) -> dict:
        value = object_json(json.loads(realm.sql(statement)))
        if value.get('status') != 'success':
            raise RuntimeError('Initial clock recovery SQL did not accept')
        return object_json(value['data'])

    def snapshot() -> dict:
        return object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('revision',accepted_revision::text,'clocks',app.match_clock_snapshot('{matched}'),'original',app.read_native_opening_source('{game}')->'data'->'opening') FROM app.game WHERE id='{game}'")))

    before = snapshot()
    if before['revision'] != '0' or before['original'] != opening:
        raise RuntimeError('Initial clock control has wrong native original')
    for row in before['clocks']:
        row = object_json(row)
        if row['remainingMs'] != '1800000' or row['paused'] != (row['seatId'] != chooser):
            raise RuntimeError('Actual initial BO3 bank did not select exactly its chooser')
    paused = success(f"SELECT app.pause_game_incident('{game}','fixture:source-reader',1,0,clock_timestamp(),'storage','fixture:actual-initial-clock-incident','{uuid.uuid4()}')")
    stopped = snapshot()
    if not all(object_json(row)['paused'] is True for row in stopped['clocks']):
        raise RuntimeError('Initial incident did not pause every bank')
    def millis(value: str) -> int:
        delta = datetime.fromisoformat(value) - datetime(1970, 1, 1, tzinfo=timezone.utc)
        return delta.days * 86400000 + delta.seconds * 1000 + delta.microseconds // 1000

    for old, current in zip(before['clocks'], stopped['clocks'], strict=True):
        old, current = object_json(old), object_json(current)
        elapsed = millis(current['lastAccountedAt']) - millis(old['lastAccountedAt'])
        expected = int(old['remainingMs']) if old['paused'] else max(0, int(old['remainingMs']) - elapsed)
        if old['seatId'] != current['seatId'] or int(current['remainingMs']) != expected or elapsed < 0:
            raise RuntimeError('Initial incident debit differs from actual database boundaries')
    realm.sql('SELECT pg_sleep(0.005)')
    checkpoint = str(uuid.UUID(paused['checkpointId']))
    request = str(uuid.uuid4())
    statement = f"SELECT app.restore_game_checkpoint('{game}',{int(paused['epoch'])},{int(paused['operationalRevision'])},'{checkpoint}','fixture:source-reader','fixture:actual-initial-clock-restored','{request}')"
    restored = success(statement)
    after = snapshot()
    for row, stopped_row in zip(after['clocks'], stopped['clocks'], strict=True):
        row, stopped_row = object_json(row), object_json(stopped_row)
        if row['seatId'] != stopped_row['seatId'] or row['remainingMs'] != stopped_row['remainingMs'] or row['paused'] != (row['seatId'] != chooser):
            raise RuntimeError('Initial bank restoration charged downtime or changed actor')
    if success(statement) != restored or snapshot() != after:
        raise RuntimeError('Initial restoration retry changed banks or source')
    if after['revision'] != '0' or after['original'] != opening:
        raise RuntimeError('Initial recovery manufactured native input or rewrote original')
    return {'actualInitialChooserRunning': True, 'actualSqlIncidentBeforeFirstMove': True,
            'noIncidentTimeDebit': True, 'exactInitialElapsedDebit': True, 'exactRestorationRetry': True,
            'unchangedNativeRevision': '0', 'nativeTimeoutQualified': False}
