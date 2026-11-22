"""Real managed Auth/PostgREST boundary checks over synthetic native SQL metadata.

This module never qualifies retained bytes, kernel execution or a playable release.
"""
import json
import uuid
from typing import Callable
from disposable_supabase import DisposableSupabase
from qualification_http import object_json
from native_source_reader_acceptance import prepare_source_reader_reservation

Request = Callable[..., tuple[int, object]]
Expect = Callable[[str, tuple[int, object], int, object], None]


def qualify_native_boundaries(realm: DisposableSupabase, request: Request, expect: Expect,
                              owner: str, peer: str, owner_claims: dict,
                              owner_token: str, peer_token: str) -> dict:
    """Exercise actual private RPC roles and managed session-derived ownership."""
    positive_bodies: dict[str, dict] = {}
    observed: list[str] = []
    wire_samples: list[dict] = []

    def rpc(name: str, body: dict) -> dict:
        status, response = request('/rest/v1/rpc/' + name, body=body, admin=True)
        value = object_json(response)
        if status != 200:
            raise RuntimeError('Managed native RPC failed: ' + name)
        observed.append(name)
        positive_bodies[name] = body
        # Disposable fixture metadata only. Never capture the HTTP bearer or
        # actual provider/Storage credentials in retained wire examples.
        wire_samples.append({'operation':name,'request':body,'response':value})
        return value

    def call(name: str, body: dict) -> dict:
        value = rpc(name, body)
        if value.get('status') != 'success':
            raise RuntimeError('Managed native positive case failed: ' + name)
        return object_json(value['data'])

    def sql_accepted(statement: str) -> dict:
        value = object_json(json.loads(realm.sql(statement)))
        if value.get('status') != 'success':
            raise RuntimeError('Managed SQL fixture producer failed')
        return object_json(value['data'])

    def literal(value: object) -> str:
        return "'" + json.dumps(value).replace("'", "''") + "'::jsonb"

    def expected(name: str, body: dict, value: dict, label: str) -> None:
        actual = request('/rest/v1/rpc/' + name, body=body, admin=True)
        expect('native-boundary-' + label, actual, 200, value)
        positive_bodies[name] = body
        wire_samples.append({'operation':name,'request':body,'response':object_json(actual[1])})

    def page(name: str, required: tuple[str, str] | None = None) -> dict:
        """Check actual service delivery and retain the same call for denials."""
        body = {'p_after':None,'p_limit':100}
        value = rpc(name,body)
        items = value.get('items')
        if not isinstance(items, list) or len(items) > 100 or len(json.dumps(value).encode()) > 1_048_576:
            raise RuntimeError('Managed native source page failed: ' + name)
        if required is not None and not any(object_json(item).get(required[0]) == required[1] for item in items):
            raise RuntimeError('Managed native source omitted admitted work: ' + name)
        return value

    source = prepare_source_reader_reservation(realm, owner, peer, start_match=False)
    room, release, legality = (str(uuid.UUID(source[key])) for key in ('roomId', 'releaseId', 'legalityId'))
    revisions = object_json(source['revisions'])
    subject, session = str(uuid.UUID(owner_claims['sub'])), str(uuid.UUID(owner_claims['session_id']))

    def room_basis() -> str:
        return realm.sql(f"SELECT revision::text FROM app.room WHERE id='{room}'")

    def room_source() -> dict:
        return call('read_native_room_source', {'p_room':room,'p_basis':room_basis(),'p_release':release,'p_legality':legality})

    teams = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('slotId',id,'teamRef',NULL) ORDER BY seat_index) FROM app.room_slot WHERE room_id='{room}' AND retired_at IS NULL"))
    teams_sql = json.dumps(teams).replace("'", "''")
    configured = object_json(json.loads(realm.sql(f"SELECT app.configure_room('{owner}','{room}',{room_basis()},'{source['profileId']}','bo3','{teams_sql}'::jsonb,'{uuid.uuid4()}')")))
    if configured.get('status') != 'success':
        raise RuntimeError('Managed native BO3 fixture configuration failed')
    ready_source = room_source()
    ready_original = {'requestId':str(uuid.uuid4()),'expectedRevision':room_basis()}
    ready_body = {'p_subject':subject,'p_session':session,'p_room':room,'p_original':ready_original,
                  'p_prepared':{'version':'native-ready-prepared-v1','sourceHash':ready_source['sourceHash'],
                                'releaseId':release,'legalityRevisionId':legality,
                                'selectedRevisionId':revisions[owner],'qualificationRef':'fixture:managed-SQL-only'}}
    ready = call('qualify_room_ready_command', ready_body)
    if ready.get('resourceId') != room:
        raise RuntimeError('Managed Ready acknowledgment has wrong room')
    expected('qualify_room_ready_command', {**ready_body,'p_prepared':None}, {'status':'success','data':ready}, 'ready-original-retry')
    read_ready = {key:value for key,value in ready_body.items() if key != 'p_prepared'}
    recovered = call('read_qualified_room_ready_command', read_ready)
    if recovered != {'state':'accepted','response':{'status':'success','data':ready}}:
        raise RuntimeError('Managed Ready recovery lost its original')
    countdown_source = room_source()
    countdown_body = {'p_subject':subject,'p_session':session,'p_room':room,
                      'p_original':{'requestId':str(uuid.uuid4()),'expectedRevision':room_basis()},
                      'p_prepared':{'version':'native-countdown-prepared-v1','sourceHash':countdown_source['sourceHash'],
                                    'releaseId':release,'legalityRevisionId':legality,'qualificationRef':'fixture:managed-SQL-only'}}
    countdown = call('qualify_room_countdown_command', countdown_body)
    read_countdown = {key:value for key,value in countdown_body.items() if key != 'p_prepared'}
    if call('read_qualified_room_countdown_command', read_countdown).get('response') != {'status':'success','data':countdown}:
        raise RuntimeError('Managed countdown recovery lost its original')
    realm.sql(f"UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='{room}'")
    page('read_native_countdown_work', ('id',room))
    slots = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:managed-SQL-only') ORDER BY seat_index) FROM app.room_slot WHERE room_id='{room}' AND retired_at IS NULL"))
    started = call('start_room_match', {'p_room':room,'p_expected':room_basis(),'p_release':release,'p_legality':legality,
                                       'p_qualification':'fixture:managed-SQL-only','p_inputs':slots,'p_request':str(uuid.uuid4())})
    matched, game = (str(uuid.UUID(started[key])) for key in ('resourceId','gameId'))
    page('read_native_match_recovery_work', ('id',matched))
    context = str(uuid.uuid4())
    seat = realm.sql(f"SELECT id::text FROM app.match_seat WHERE match_id='{matched}' AND account_id='{owner}'")
    peer_seat = realm.sql(f"SELECT id::text FROM app.match_seat WHERE match_id='{matched}' AND account_id='{peer}'")

    def artifact(role: str, value: int) -> str:
        return f'native-artifact-v1/{game}/{context}/{role}/1/{value:064x}.json'

    basis = {'gameId':game,'contextId':context,'epoch':'0','revision':'0'}
    opening = {'contextId':context,'snapshotRef':artifact('initial',1),'contextRevision':'fixture:managed-native-v1',
               'stateRef':artifact('state',2),'factsRef':artifact('facts',3),'eventRef':artifact('event',4),
               'qualificationRef':'fixture:managed-SQL-only','eventSchemaRevision':'native-transition-v1',
               'clockObservation':{'reason':'native_decision','seatId':seat,'acceptedRevision':'0','proofRef':artifact('state',2)}}
    opening_body = {'p_match':matched,'p_basis':realm.sql(f"SELECT revision::text FROM app.match WHERE id='{matched}'"),
                    'p_game':game,'p_owner':'fixture:managed-native','p_opening':opening,
                    'p_native_epoch':'0','p_runtime_id':'1'*64,'p_program_source_id':'2'*64}
    opened = call('accept_native_opening', opening_body)
    if opened.get('contextId') != context:
        raise RuntimeError('Managed opening has wrong context')
    opening_original = {'version':'native-opening-v1','matchId':matched,'gameId':game,
                        'owner':opening_body['p_owner'],'originalBasis':opening_body['p_basis'],
                        'opening':opening,'nativeEpoch':'0','runtimeId':'1'*64,'programSourceId':'2'*64}
    opening_disposition = call('read_native_system_disposition',
                              {'p_game':game,'p_request':game,'p_original':opening_original})
    if opening_disposition != {'state':'accepted','response':{'status':'success','data':opened}}:
        raise RuntimeError('Managed system opening disposition differs')
    initial_source = call('read_native_game_source',{'p_game':game})
    initial_history_body = {'p_game':game,'p_context':context,
                            'p_anchor_checkpoint':object_json(initial_source['checkpoint'])['id'],
                            'p_after_revision':None,'p_limit':1}
    initial_history = call('read_native_transition_sources',initial_history_body)
    if len(initial_history['groups']) != 1 or initial_history['groups'][0].get('eventSchemaRevision') != 'native-transition-v1':
        raise RuntimeError('Managed initial transition source differs')
    call('read_native_opening_source', {'p_game':game})
    page('read_native_recovery_work', ('gameId',game))
    for name in ('read_native_queue_work','read_native_ai_binding_work','read_native_unscheduled_decision_work'):
        page(name)
    access_body = {'p_subject':subject,'p_session':session,'p_game':game,'p_context':context}
    access = call('read_native_human_access', access_body)
    if object_json(access['actor']).get('seatId') != seat or object_json(access['source']).get('nativeEpoch') != '0':
        raise RuntimeError('Managed native scope lost copied seat or native epoch')
    free = object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('backId',default_back_id,'sleeveId',default_sleeve_id,'diceId',default_dice_id,'playmatId',NULL) FROM app.profile WHERE account_id='{owner}'")))
    presentation_body = {**access_body, 'p_free':free}
    presented = call('read_native_human_presentation', presentation_body)
    presentation_seats = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('seatId',s.id,'appearance',app.resolve_account_appearance(s.account_id,d.back_id,d.sleeve_id,d.dice_id,d.playmat_id)) ORDER BY s.seat_index) FROM app.match_seat s JOIN app.deck_revision r ON r.id=s.source_revision_id JOIN app.deck d ON d.id=r.deck_id WHERE s.match_id='{matched}'"))
    if presented != {'version':'native-presentation-v1','access':access,'presentation':{'seats':presentation_seats}}:
        raise RuntimeError('Managed current presentation differs from authorized roster')
    expected('read_native_human_presentation',{**presentation_body,'p_session':str(uuid.uuid4())},
             {'status':'fail','data':{'session':'unauthorized'}},'presentation-invalid-session')
    expected('read_native_human_presentation',{**presentation_body,'p_context':str(uuid.uuid4())},
             {'status':'fail','data':{'game':'unavailable'}},'presentation-wrong-context')
    expected('read_native_human_access',{**access_body,'p_session':str(uuid.uuid4())},
             {'status':'fail','data':{'session':'unauthorized'}},'invalid-session')
    view_body = {'p_game':game,'p_context':context,'p_epoch':'0','p_revision':'0','p_viewer':seat,'p_disclosure':'live','p_view_ref':artifact('view',5)}
    issued = call('publish_native_view_source', view_body)
    exact_view = call('read_native_view_source', {key:value for key,value in view_body.items() if key != 'p_view_ref'})
    if exact_view != issued:
        raise RuntimeError('Managed View recovery changed first winner')
    expected('publish_native_view_source',{**view_body,'p_view_ref':artifact('view',6)}, {'status':'success','data':issued},'view-first-winner')
    cursor_body = {'p_game':game,'p_context':context,'p_epoch':'0','p_revision':'0','p_viewer':seat,
                   'p_through_sequence':'0','p_purpose':'snapshot','p_source_receipt':issued['receiptId'],'p_scan':None}
    cursor = call('issue_native_cursor_source', cursor_body)
    recovered_cursor = call('read_native_cursor_source',{'p_game':game,'p_viewer':seat,'p_token':cursor['token']})
    if recovered_cursor != cursor:
        raise RuntimeError('Managed cursor recovery changed its issued source')
    expected('read_native_cursor_source',{'p_game':game,'p_viewer':peer_seat,'p_token':cursor['token']},
             {'status':'fail','data':{'cursor':'unavailable'}},'cursor-wrong-viewer')
    archive_body={'p_game':game,'p_context':context,'p_viewer':seat,'p_after_sequence':None,'p_limit':2,'p_disclosure':'live','p_source_view_receipt':issued['receiptId']}
    archive = rpc('read_native_archive_source',archive_body)
    if object_json(archive['items'][0]).get('sequence') != '0' or archive.get('scannedThrough') != '0':
        raise RuntimeError('Managed archive omitted opening sequence zero')
    original = {'requestId':str(uuid.uuid4()),'basis':basis,'action':{'kind':'concede'}}
    reject_body = {'p_subject':subject,'p_session':session,'p_original':original,
                   'p_prepared':{'version':'native-player-reject-v1','basis':basis,'actorSeatId':seat,'viewReceiptId':issued['receiptId'],
                                 'runtimeId':'1'*64,'programSourceId':'2'*64,'errors':{'action':'unsupported'}}}
    expected('submit_native_human_command',reject_body,{'status':'fail','data':{'action':'unsupported'}},'native-rejection')
    query={'gameId':game,'contextId':context,'outstanding':[original]}
    receipts = call('read_native_human_receipts',{'p_subject':subject,'p_session':session,'p_query':query})
    if object_json(receipts['receipts'][0]).get('state') != 'rejected':
        raise RuntimeError('Managed receipt recovery invented native success')
    call('read_native_host_receipt',{'p_game':game,'p_seat':seat,'p_original':original})

    def candidate_for(match_id: str, context_id: str, reference: Callable[[str, int], str]) -> dict:
        clocks = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('seatId',s.id,'clockRevision',c.clock_revision::text,'remainingMs',c.remaining_ms::text,'paused',true,'mustStopBank',c.policy_ref='bo3-overall-bank-v1' AND NOT c.paused) ORDER BY s.id) FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id='{match_id}'"))
        # Declared SQL fixture intent, not a native waiting-state derivation.
        revision = realm.sql(f"SELECT g.accepted_revision::text FROM app.game g JOIN app.game_context ctx ON ctx.game_id=g.id WHERE ctx.id='{context_id}'")
        for clock in clocks:
            if clock.pop('mustStopBank'):
                clock['eligibility'] = {'reason':'native_wait','acceptedRevision':revision,'proofRef':reference('state',10)}
        return {'contextId':context_id,'stateRef':reference('state',10),'factsRef':reference('facts',11),
                'events':[reference('event',12)],'facts':[],'clocks':clocks,'participation':[],
                'eventSchemaRevision':'native-transition-v1'}

    def candidate() -> dict:
        return candidate_for(matched,context,artifact)

    accepted_original = {'requestId':str(uuid.uuid4()),'basis':basis,'action':{'kind':'pass'}}
    accepted_body = {'p_subject':subject,'p_session':session,'p_original':accepted_original,
                     'p_prepared':{'version':'native-player-accept-v1','basis':basis,'actorSeatId':seat,
                                   'viewReceiptId':issued['receiptId'],'runtimeId':'1'*64,'programSourceId':'2'*64,
                                   'inputRef':artifact('input',20),'owner':'fixture:managed-native','fenceEpoch':'1',
                                   'candidate':candidate()}}
    accepted = call('submit_native_human_command',accepted_body)
    if accepted.get('revision') != '1':
        raise RuntimeError('Managed native commit failed to advance exactly once')
    expected('submit_native_human_command',{**accepted_body,'p_prepared':None},
             {'status':'success','data':accepted},'accepted-original-retry')
    host_basis = {**basis,'revision':'1'}
    host_body = {'p_game':game,'p_request':str(uuid.uuid4()),
                 'p_original':{'version':'native-host-input-v1','basis':host_basis,'kind':'entropy',
                               'inputRef':artifact('input',21),'runtimeId':'1'*64,'programSourceId':'2'*64},
                 'p_prepared':{'owner':'fixture:managed-native','fenceEpoch':'1','candidate':candidate()}}
    host = call('commit_native_host_input',host_body)
    expected('commit_native_host_input',host_body,{'status':'success','data':host},'host-original-retry')
    host_disposition = call('read_native_system_disposition',
                           {key:value for key,value in host_body.items() if key != 'p_prepared'})
    if host_disposition != {'state':'accepted','response':{'status':'success','data':host}}:
        raise RuntimeError('Managed system input disposition differs')
    if call('read_native_transition_sources',initial_history_body) != initial_history:
        raise RuntimeError('Managed anchored transition prefix changed after native play')
    recovered = call('read_native_human_receipts',{'p_subject':subject,'p_session':session,
                     'p_query':{'gameId':game,'contextId':context,'outstanding':[accepted_original]}})
    if object_json(recovered['receipts'][0]).get('response') != {'status':'success','data':accepted}:
        raise RuntimeError('Managed native recovery lost accepted historical result')

    def match_state() -> dict:
        return object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('revision',revision::text,'availabilityRevision',availability_revision::text,'incidentId',incident_receipt_id) FROM app.match WHERE id='{matched}'")))

    def timestamp() -> str:
        return realm.sql("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')")

    paused_body = {'p_game':game,'p_owner':'fixture:managed-native','p_epoch':'1','p_operational_revision':'0',
                   'p_incident_at':timestamp(),'p_kind':'storage','p_proof':'fixture:managed-incident','p_request':str(uuid.uuid4())}
    paused = call('pause_game_incident',paused_body)
    recovering = page('read_native_recovery_work',('gameId',game))
    if not any(object_json(item).get('gameId') == game and object_json(item).get('availability') == 'recovering' for item in recovering['items']):
        raise RuntimeError('Managed game incident is missing from recovery discovery')
    restored = call('restore_game_checkpoint',{'p_game':game,'p_epoch':paused['epoch'],
                    'p_operational_revision':paused['operationalRevision'],'p_checkpoint':paused['checkpointId'],
                    'p_new_owner':'fixture:managed-restored','p_restoration_ref':'fixture:SQL-codec-proof','p_request':str(uuid.uuid4())})
    # Host fencing advances independently of native lineage. The restored host
    # commits the next native revision using the original native epoch zero.
    terminal_candidate = candidate()
    for clock in terminal_candidate['clocks']:
        clock.pop('eligibility', None)
    terminal_candidate['terminal'] = {'outcome':'draw','winnerSeatIds':[],'winnersRef':'fixture:managed-draw','reason':'fixture'}
    terminal_candidate['participation'] = [{'seatId':player,'ownTurns':'3','reason':'fixture'} for player in (seat,peer_seat)]
    terminal = call('commit_native_host_input',{'p_game':game,'p_request':str(uuid.uuid4()),
                    'p_original':{'version':'native-host-input-v1','basis':{**basis,'revision':'2'},'kind':'native-work',
                                  'inputRef':artifact('input',22),'runtimeId':'1'*64,'programSourceId':'2'*64},
                    'p_prepared':{'owner':'fixture:managed-restored','fenceEpoch':restored['epoch'],'candidate':terminal_candidate}})
    result = realm.sql(f"SELECT id::text FROM app.game_result WHERE game_id='{game}'")
    consumed = call('consume_game_outcome',{'p_result':result,'p_expected':match_state()['revision']})
    if consumed.get('phase') != 'between_games':
        raise RuntimeError('Managed drawn BO3 game failed to open between-game decisions')
    initial_checkpoint = realm.sql(f"SELECT id::text FROM app.game_checkpoint WHERE game_id='{game}' AND accepted_revision=0")
    terminal_cut = object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('revision',accepted_revision::text,'through',(SELECT max(sequence)::text FROM app.game_event WHERE game_id='{game}')) FROM app.game WHERE id='{game}'")))
    call('publish_game_replay',{'p_game':game,'p_revision':terminal_cut['revision'],'p_checkpoint':initial_checkpoint,
                              'p_through':terminal_cut['through'],'p_artifact':artifact('replay',23),
                              'p_visibility':'fixture:managed-visibility','p_schema':'fixture:SQL-only','p_qualification':'fixture:SQL-only'})
    decisions = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('id',id,'seatId',seat_id) ORDER BY id) FROM app.between_game_decision WHERE match_id='{matched}' AND state='pending'"))
    page('read_native_unscheduled_decision_work',('id',decisions[0]['id']))
    state = match_state()
    paused_between = call('pause_match_between_games',{'p_match':matched,'p_expected':state['revision'],'p_availability':state['availabilityRevision'],
                          'p_incident_at':timestamp(),'p_kind':'storage','p_proof':'fixture:between-incident','p_request':str(uuid.uuid4())})
    page('read_native_match_recovery_work',('id',matched))
    state = match_state()
    call('resume_match_between_games',{'p_match':matched,'p_expected':state['revision'],'p_availability':state['availabilityRevision'],
                                      'p_incident':paused_between['incidentId'],'p_proof':'fixture:between-restored','p_request':str(uuid.uuid4())})
    own = next(item for item in decisions if item['seatId'] == seat)
    between_source = call('read_native_between_source',{'p_decision':own['id'],'p_basis':match_state()['revision'],'p_selected_revision':revisions[owner]})
    between_original = {'requestId':str(uuid.uuid4()),'decisionId':own['id'],'expectedRevision':match_state()['revision'],'selectedRevisionId':revisions[owner]}
    between_body = {'p_subject':subject,'p_session':session,'p_original':between_original,
                    'p_prepared':{'version':'native-between-prepared-v1','sourceHash':between_source['sourceHash'],
                                  'replyRef':'fixture:managed-between','qualificationRef':'fixture:SQL-only'}}
    between = call('qualify_between_game_command',between_body)
    if 'qualificationRef' in between:
        raise RuntimeError('Managed between reply exposed private qualification')
    if call('read_qualified_between_game_command',{key:value for key,value in between_body.items() if key != 'p_prepared'}).get('response') != {'status':'success','data':between}:
        raise RuntimeError('Managed between-game original recovery changed response')
    other = next(item for item in decisions if item['seatId'] == peer_seat)
    state = match_state()
    deadline = object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('clock',clock_revision::text,'revision',deadline_revision::text) FROM app.between_game_decision WHERE id='{other['id']}'")))
    scheduled = call('schedule_between_game_deadline',{'p_decision':other['id'],'p_match_revision':state['revision'],
                     'p_deadline_revision':deadline['revision'],'p_clock_revision':deadline['clock'],'p_availability_revision':state['availabilityRevision'],
                     'p_profile':source['profileId'],'p_policy':'fixture:managed-timer','p_due':timestamp(),
                     'p_qualification':'fixture:SQL-timing','p_request':str(uuid.uuid4())})
    page('read_due_between_game_deadlines')
    call('expire_between_game_deadline',{'p_decision':other['id'],'p_deadline_revision':scheduled['deadlineRevision'],
                                       'p_revision':revisions[peer],'p_reply':'fixture:managed-default',
                                       'p_qualification':'fixture:SQL-default','p_request':str(uuid.uuid4())})
    state = match_state()
    interrupted = call('pause_match_between_games',{'p_match':matched,'p_expected':state['revision'],'p_availability':state['availabilityRevision'],
                       'p_incident_at':timestamp(),'p_kind':'storage','p_proof':'fixture:opening-incident','p_request':str(uuid.uuid4())})
    state = match_state()
    call('finalize_unrecoverable_match',{'p_match':matched,'p_expected':state['revision'],'p_availability':state['availabilityRevision'],
                                       'p_incident':interrupted['incidentId'],'p_failure_ref':'fixture:verified-SQL-only-failure'})

    # Source acquisition remains a canonical fixture producer. The capabilities
    # being qualified below execute through actual service-role PostgREST calls.
    profile = str(uuid.uuid4())
    format_id = realm.sql(f"SELECT format_id::text FROM app.room WHERE id='{room}'")
    configuration = {'version':'matchmaking-profile-v1','influence':'none','planechase':False,'qualificationRef':'sha256:'+'3'*64,
                     'configurations':[{'key':'duel','formatId':format_id,'seatCount':'2','structure':'bo1','teams':[None,None]}]}
    realm.sql(f"INSERT INTO app.table_profile_revision(id,key,revision_ref,rules_ref,minimum_seats,seat_capacity,admission_qualification_ref,matchmaking_config) VALUES('{profile}','{profile}','fixture','fixture',2,2,'fixture:SQL-only',{literal(configuration)})")
    entries = []
    for account in (owner,peer):
        alternatives = [{'deckRevisionId':revisions[account],'choices':[{'profileId':profile,'configurationKey':'duel','maximumAiSeats':'0','maximumSponsoredAiSeats':'0'}]}]
        queued = sql_accepted(f"SELECT app.enqueue_matchmaking_intent('{account}','open',false,{literal(alternatives)},'{uuid.uuid4()}')")
        entries.append(queued['resourceId'])
        chunks = rpc('read_native_queue_intent_source',{'p_intent':queued['intentId'],'p_after_offset':None,'p_limit':8})
        if not chunks.get('items') or chunks.get('nextOffset') is not None:
            raise RuntimeError('Managed small queue source was not delivered completely')
    page('read_native_queue_work')
    selection = {'profileId':profile,'configurationKey':'duel','hostEntryId':entries[0],
                 'humanSeats':[{'queueEntryId':entry,'seatIndex':str(index)} for index,entry in enumerate(entries)],'aiSeats':[]}
    call('claim_matchmaking',{'p_selection':selection,'p_request':str(uuid.uuid4())})

    ai_source = prepare_source_reader_reservation(realm,owner,peer,start_match=False)
    ai_room = ai_source['roomId']
    ai_revision = ai_source['revisions'][owner]

    def ai_room_basis() -> str:
        return realm.sql(f"SELECT revision::text FROM app.room WHERE id='{ai_room}'")

    sql_accepted(f"SELECT app.leave_room('{peer}','{ai_room}',{ai_room_basis()},'{uuid.uuid4()}')")
    ai_slot = realm.sql(f"SELECT id::text FROM app.room_slot WHERE room_id='{ai_room}' AND seat_index=1 AND retired_at IS NULL AND account_id IS NULL")
    uuid.UUID(ai_slot)
    realm.sql(f"UPDATE app.room_slot SET sponsor_id='{owner}',deck_revision_id='{ai_revision}',controller_kind='ai',ready=false WHERE id='{ai_slot}'")
    ai_teams = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('slotId',id,'teamRef',NULL) ORDER BY seat_index) FROM app.room_slot WHERE room_id='{ai_room}' AND retired_at IS NULL"))
    sql_accepted(f"SELECT app.configure_room('{owner}','{ai_room}',{ai_room_basis()},'{ai_source['profileId']}','bo3',{literal(ai_teams)},'{uuid.uuid4()}')")
    boot = sql_accepted(f"SELECT app.bootstrap_ai_slot('{owner}','{ai_slot}',{ai_room_basis()},'{ai_revision}','{uuid.uuid4()}')")
    binding = boot['resourceId']

    def confirm(grant_id: str, epoch: str) -> None:
        configuration_id = realm.sql(f"SELECT configuration_id::text FROM app.room WHERE id='{ai_room}'")
        call('commit_ai_ready_confirmation',{'p_grant':grant_id,'p_epoch':epoch,'p_revision':ai_revision,
             'p_qualification':'fixture:SQL-deck','p_authentication':'fixture:SQL-auth','p_configuration':configuration_id,'p_request':str(uuid.uuid4())})

    def deliver(grant_id: str) -> None:
        call('observe_ai_grant_delivery',{'p_grant':grant_id,'p_expected':'0','p_outcome':'delivered',
             'p_proof':'fixture:SQL-delivery','p_request':str(uuid.uuid4())})

    def start_runner(epoch: str) -> tuple[str, str]:
        start = rpc('read_ai_start_work',{'p_binding':binding,'p_epoch':epoch})
        call('observe_ai_provider',{'p_binding':binding,'p_epoch':epoch,'p_start':start['startId'],'p_outcome':'running',
             'p_provider_ref':'fixture:SQL-runner:'+str(uuid.uuid4()),'p_proof':'fixture:SQL-provider','p_request':str(uuid.uuid4())})
        secret = 'fixture:SQL-secret-ref:'+str(uuid.uuid4())
        grant = call('issue_ai_grant',{'p_binding':binding,'p_epoch':epoch,'p_secret_ref':secret,'p_request':str(uuid.uuid4())})['resourceId']
        deliver(grant)
        confirm(grant,epoch)
        return grant,secret

    start_runner('1')
    sql_accepted(f"SELECT app.start_room_countdown('{owner}','{ai_room}',{ai_room_basis()},'fixture:SQL-decks','{uuid.uuid4()}')")
    realm.sql(f"UPDATE app.room SET countdown_at=clock_timestamp()-interval '1 second' WHERE id='{ai_room}'")
    ai_slots = json.loads(realm.sql(f"SELECT jsonb_agg(jsonb_build_object('slotId',id,'revisionId',deck_revision_id,'snapshotRef','fixture:SQL-deck') ORDER BY seat_index) FROM app.room_slot WHERE room_id='{ai_room}' AND retired_at IS NULL"))
    ai_started = call('start_room_match',{'p_room':ai_room,'p_expected':ai_room_basis(),'p_release':ai_source['releaseId'],'p_legality':ai_source['legalityId'],
                      'p_qualification':'fixture:SQL-only','p_inputs':ai_slots,'p_request':str(uuid.uuid4())})
    ai_match,ai_game = ai_started['resourceId'],ai_started['gameId']
    grant_source = object_json(json.loads(realm.sql(f"SELECT jsonb_build_object('id',g.id,'epoch',g.binding_epoch::text,'secretRef',g.grant_secret_ref,'seatId',g.match_seat_id) FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id WHERE b.id='{binding}' AND g.binding_epoch=b.binding_epoch AND g.state='active'")))
    grant,epoch,secret,ai_seat = (grant_source[key] for key in ('id','epoch','secretRef','seatId'))
    deliver(grant)
    confirm(grant,epoch)
    ai_context = str(uuid.uuid4())

    def ai_artifact(role: str, value: int) -> str:
        return f'native-artifact-v1/{ai_game}/{ai_context}/{role}/1/{value:064x}.json'

    ai_basis = {'gameId':ai_game,'contextId':ai_context,'epoch':'0','revision':'0'}
    ai_opening = {'contextId':ai_context,'snapshotRef':ai_artifact('initial',1),'contextRevision':'fixture:managed-native-v1',
                  'stateRef':ai_artifact('state',2),'factsRef':ai_artifact('facts',3),'eventRef':ai_artifact('event',4),'qualificationRef':'fixture:SQL-only','eventSchemaRevision':'native-transition-v1',
                  'clockObservation':{'reason':'native_decision','seatId':ai_seat,'acceptedRevision':'0','proofRef':ai_artifact('state',2)}}
    call('accept_native_opening',{'p_match':ai_match,'p_basis':realm.sql(f"SELECT revision::text FROM app.match WHERE id='{ai_match}'"),
         'p_game':ai_game,'p_owner':'fixture:managed-ai','p_opening':ai_opening,'p_native_epoch':'0','p_runtime_id':'1'*64,'p_program_source_id':'2'*64})
    ai_presentation = call('read_native_human_presentation',
                           {'p_subject':subject,'p_session':session,'p_game':ai_game,'p_context':ai_context,'p_free':free})
    ai_seats = object_json(ai_presentation['presentation']).get('seats')
    if ai_presentation.get('version') != 'native-presentation-v1' or not isinstance(ai_seats,list) or len(ai_seats) != 2 or not any(object_json(row).get('seatId') == ai_seat for row in ai_seats):
        raise RuntimeError('Managed sponsored AI presentation omitted its canonical seat')
    controller_body = {'p_grant':grant,'p_epoch':epoch,'p_secret_ref':secret,'p_game':ai_game,'p_context':ai_context}
    call('read_native_controller_access',controller_body)
    expected('read_native_controller_access',{**controller_body,'p_secret_ref':'fixture:wrong'},
             {'status':'fail','data':{'actor':'unavailable'}},'controller-wrong-secret')
    ai_view = call('publish_native_view_source',{'p_game':ai_game,'p_context':ai_context,'p_epoch':'0','p_revision':'0','p_viewer':ai_seat,'p_disclosure':'live','p_view_ref':ai_artifact('view',5)})
    ai_original = {'requestId':str(uuid.uuid4()),'basis':ai_basis,'action':{'kind':'pass'}}
    ai_prepared = {'version':'native-player-accept-v1','basis':ai_basis,'actorSeatId':ai_seat,'viewReceiptId':ai_view['receiptId'],
                   'runtimeId':'1'*64,'programSourceId':'2'*64,'inputRef':ai_artifact('input',20),'owner':'fixture:managed-ai','fenceEpoch':'1',
                   'candidate':candidate_for(ai_match,ai_context,ai_artifact)}
    ai_accepted = call('submit_native_controller_command',{'p_grant':grant,'p_epoch':epoch,'p_secret_ref':secret,'p_original':ai_original,'p_prepared':ai_prepared})
    rebound = call('rebind_ai_runner',{'p_binding':binding,'p_epoch':epoch,'p_proof':'fixture:SQL-rebind','p_request':str(uuid.uuid4())})
    stops = rpc('read_ai_stop_work_page',{'p_binding':binding,'p_after':None,'p_limit':100})
    if not any(object_json(item).get('stopId') == rebound['previousStopId'] for item in stops.get('items',[])):
        raise RuntimeError('Managed controller rebind lost historical stop')
    expected('read_native_controller_access',controller_body,{'status':'fail','data':{'actor':'unavailable'}},'revoked-controller')
    call('observe_ai_stop',{'p_binding':binding,'p_epoch':epoch,'p_stop':rebound['previousStopId'],'p_proof':'fixture:SQL-stop','p_request':str(uuid.uuid4())})
    epoch = rebound['epoch']
    grant,secret = start_runner(epoch)
    recovered_ai = call('submit_native_controller_command',{'p_grant':grant,'p_epoch':epoch,'p_secret_ref':secret,'p_original':ai_original,'p_prepared':None})
    if recovered_ai != ai_accepted:
        raise RuntimeError('Managed replacement controller changed original acknowledgment')
    controller_query = {'gameId':ai_game,'contextId':ai_context,'outstanding':[ai_original]}
    ai_receipts = call('read_native_controller_receipts',{'p_grant':grant,'p_epoch':epoch,'p_secret_ref':secret,'p_query':controller_query})
    if object_json(ai_receipts['receipts'][0]).get('response') != {'status':'success','data':ai_accepted}:
        raise RuntimeError('Managed replacement controller lost original recovery')
    ai_terminal = candidate_for(ai_match,ai_context,ai_artifact)
    ai_terminal['terminal'] = {'outcome':'draw','winnerSeatIds':[],'winnersRef':'fixture:managed-draw','reason':'fixture'}
    human_seat = realm.sql(f"SELECT id::text FROM app.match_seat WHERE match_id='{ai_match}' AND controller_kind='human'")
    ai_terminal['participation'] = [{'seatId':human_seat,'ownTurns':'3','reason':'fixture'}]
    call('commit_native_host_input',{'p_game':ai_game,'p_request':str(uuid.uuid4()),
         'p_original':{'version':'native-host-input-v1','basis':{**ai_basis,'revision':'1'},'kind':'native-work','inputRef':ai_artifact('input',21),'runtimeId':'1'*64,'programSourceId':'2'*64},
         'p_prepared':{'owner':'fixture:managed-ai','fenceEpoch':'1','candidate':ai_terminal}})
    ai_result = realm.sql(f"SELECT id::text FROM app.game_result WHERE game_id='{ai_game}'")
    call('consume_game_outcome',{'p_result':ai_result,'p_expected':realm.sql(f"SELECT revision::text FROM app.match WHERE id='{ai_match}'")})
    ai_decision = realm.sql(f"SELECT id::text FROM app.between_game_decision WHERE match_id='{ai_match}' AND seat_id='{ai_seat}' AND state='pending'")
    ai_match_basis = realm.sql(f"SELECT revision::text FROM app.match WHERE id='{ai_match}'")
    ai_between = call('read_native_between_source',{'p_decision':ai_decision,'p_basis':ai_match_basis,'p_selected_revision':ai_revision})
    ai_reply_original = {'requestId':str(uuid.uuid4()),'decisionId':ai_decision,'expectedRevision':ai_match_basis,'selectedRevisionId':ai_revision}
    ai_reply_body = {'p_grant':grant,'p_epoch':epoch,'p_secret_ref':secret,'p_original':ai_reply_original,
                     'p_prepared':{'version':'native-between-prepared-v1','sourceHash':ai_between['sourceHash'],'replyRef':'fixture:AI-between','qualificationRef':'fixture:SQL-only'}}
    ai_reply = call('qualify_controller_between_game_command',ai_reply_body)
    if call('read_qualified_controller_between_game_command',{key:value for key,value in ai_reply_body.items() if key != 'p_prepared'}).get('response') != {'status':'success','data':ai_reply}:
        raise RuntimeError('Managed controller between-game recovery changed response')

    # Public clients, including a real participant JWT, never call these private
    # source/proof operations directly. The actual Rust API is a separate tier.
    for name, body in positive_bodies.items():
        for label, bearer in [('anonymous',None),('participant',owner_token),('peer',peer_token)]:
            status, _ = request('/rest/v1/rpc/'+name,bearer,body)
            if status not in (401,403,404):
                raise RuntimeError('Managed private native RPC exposed: '+name+'/'+label)
            observed.append(name+':denied-'+label)
    return {'passed':True,'cases':len(observed),'operations':sorted(positive_bodies),
            'managedAuthAndPostgrest':True,'sqlMetadataOnly':True,'retainedBytesQualified':False,'engineOrCardsQualified':False,
            'wireSamples':wire_samples,
            'pendingBetweenQuery':{'decisionId':realm.sql(f"SELECT id::text FROM app.between_game_decision WHERE match_id='{ai_match}' AND seat_id='{human_seat}' AND state='pending'"),
                                   'basis':realm.sql(f"SELECT revision::text FROM app.match WHERE id='{ai_match}'"),'selectedRevisionId':ai_revision}}
