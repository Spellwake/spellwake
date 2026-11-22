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
                                      *, oracle_ref: str | None = None, quantity: int = 2, start_match: bool = True, structure: str = 'bo1', unlimited_basic_land: bool = True, mixed_ai: bool = False, mode: str = 'open', additional_oracle_ref: str | None = None, additional_quantity: int = 0) -> dict:
    """Use maintained deck/room/admission/opening producers in the owned realm."""
    if mode not in ('open', 'collection'):
        raise ValueError('Invalid fixture collection mode')
    if structure not in ('bo1','bo3'):
        raise ValueError("Invalid native fixture structure")
    if type(mixed_ai) is not bool or mixed_ai and not start_match:
        raise ValueError("Mixed controller fixture requires an actually started reservation")
    if type(unlimited_basic_land) is not bool:
        raise ValueError("Invalid basic-land fixture metadata")
    owner, peer = str(uuid.UUID(owner)), str(uuid.UUID(peer))
    if type(quantity) is not int or not 1 <= quantity <= 1000:
        raise ValueError("Invalid native fixture deck quantity")
    format_id, profile, definition, card_set, printing, body_revision, release, legality = (
        str(uuid.uuid4()) for _ in range(8))

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


    def reuse_definition(oracle_id: str, proposed_definition: str, proposed_revision: str) -> tuple[str, str]:
        """Reuse the same controlled immutable card across owned match fixtures."""
        existing = realm.sql(f"SELECT coalesce((SELECT id::text FROM app.card_definition WHERE oracle_ref='{oracle_id}'),'')")
        if not existing:
            return proposed_definition, proposed_revision
        selected = str(uuid.UUID(existing))
        revision = realm.sql(f"SELECT coalesce((SELECT id::text FROM app.card_definition_revision WHERE definition_id='{selected}' AND revision_ref='fixture' AND rules_text_ref='fixture:sql-only'),'')")
        return selected, str(uuid.UUID(revision)) if revision else proposed_revision

    definition, body_revision = reuse_definition(oracle, definition, body_revision)
    if extra is not None:
        extra['definition'], extra['revision'] = reuse_definition(extra['oracle'], extra['definition'], extra['revision'])

    def sql_json(statement: str) -> dict:
        return object_json(json.loads(realm.sql(statement)))

    def accepted(statement: str) -> dict:
        response = sql_json(statement)
        if response.get('status') != 'success':
            raise RuntimeError('Native source SQL fixture producer did not accept')
        return object_json(response['data'])

    def literal(value: object) -> str:
        return "'" + json.dumps(value).replace("'", "''") + "'::jsonb"

    realm.sql(f"""
      INSERT INTO app.format(id,key,name) VALUES('{format_id}','{format_id}','Native RPC fixture');
      INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref)
        VALUES('{profile}','{profile}','fixture','fixture',2,2,'fixture:sql-only');
      INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('{definition}','{oracle}','Native RPC fixture') ON CONFLICT(id) DO NOTHING;
      INSERT INTO app.card_set(id,external_set_ref,name) VALUES('{card_set}','{card_set}','Native RPC fixture');
      INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref)
        VALUES('{printing}','{definition}','{card_set}','{printing}','common','nonfoil','fixture:sql-only');
      INSERT INTO app.card_definition_revision(id,definition_id,revision_ref,rules_text_ref)
        VALUES('{body_revision}','{definition}','fixture','fixture:sql-only') ON CONFLICT(id) DO NOTHING;
      INSERT INTO app.qualified_release(id,release_ref,kernel_ref,rules_ref,program_manifest_ref,evidence_ref)
        VALUES('{release}','{release}','fixture','fixture','fixture','fixture:sql-only');
      INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land)
        VALUES('{release}','{body_revision}','fixture:sql-only',{'true' if unlimited_basic_land else 'false'});
      INSERT INTO app.legality_revision(id,source_ref,effective_at,imported_at)
        VALUES('{legality}','fixture:sql-only',clock_timestamp(),clock_timestamp());
      INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status)
        VALUES('{definition}','{format_id}','{legality}','legal');
    """)
    if extra is not None:
        # Controlled second source for actual selected native cast qualification;
        # no public release/card/format claim and no unlimited nonbasic shortcut.
        realm.sql(f"""
          INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('{extra['definition']}','{extra['oracle']}','Native cast RPC fixture') ON CONFLICT(id) DO NOTHING;
          INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref)
            VALUES('{extra['printing']}','{extra['definition']}','{card_set}','{extra['printing']}','common','nonfoil','fixture:sql-only');
          INSERT INTO app.card_definition_revision(id,definition_id,revision_ref,rules_text_ref)
            VALUES('{extra['revision']}','{extra['definition']}','fixture','fixture:sql-only') ON CONFLICT(id) DO NOTHING;
          INSERT INTO app.release_card_program(release_id,definition_revision_id,program_ref,unlimited_basic_land)
            VALUES('{release}','{extra['revision']}','fixture:sql-only',false);
          INSERT INTO app.card_legality(definition_id,format_id,legality_revision_id,status)
            VALUES('{extra['definition']}','{format_id}','{legality}','legal');
        """)
    room = str(uuid.UUID(accepted(f"SELECT app.create_room('{owner}','{format_id}','{profile}','{mode}',false,'public','{uuid.uuid4()}')")['resourceId']))

    def basis() -> str:
        value = realm.sql(f"SELECT revision::text FROM app.room WHERE id='{room}'")
        if not value.isascii() or not value.isdecimal():
            raise RuntimeError('Fixture room revision is not decimal')
        return value

    for player in ((owner,) if mixed_ai else (owner, peer)):
        deck = str(uuid.UUID(accepted(f"SELECT app.create_deck('{player}','Native RPC fixture','{mode}','{format_id}','{uuid.uuid4()}')")['resourceId']))
        entries = [{'definitionId': definition, 'printingId': printing, 'section': 'main', 'quantity': str(quantity)}]
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
        if player == peer:
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
    accepted(f"SELECT app.start_room_countdown('{owner}','{room}',{basis()},'fixture:sql-only','{uuid.uuid4()}')")
    realm.sql(f"UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='{room}'")
    copies = f"(SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:sql-only') ORDER BY seat_index) FROM app.room_slot WHERE room_id='{room}' AND retired_at IS NULL)"
    started = accepted(f"SELECT app.start_room_match('{room}',{basis()},'{release}','{legality}','fixture:sql-only',{copies},'{uuid.uuid4()}')")
    matched, game = (str(uuid.UUID(started[key])) for key in ('resourceId', 'gameId'))
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
    return {'gameId':game,'matchId':matched,'releaseId':release,'profileId':profile,
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
