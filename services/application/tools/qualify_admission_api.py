"""Exercise the normal HTTP command routes with a private, digest-pinned fixture manifest."""
from concurrent.futures import ThreadPoolExecutor
from threading import Barrier
import hashlib
import json
import socket
import time
import urllib.request
import uuid
from disposable_supabase import private_write
from qualification_http import make_opener, request_json_with_headers


def start_room_api(realm, fixture, release, legality, profile, format_id, key, cards, seats):
    digest = lambda data: 'sha256:' + hashlib.sha256(data).hexdigest()
    # This explicitly selected test profile is confined to the disposable database.
    # Public rollout requires its own approved profile and whole-card evidence.
    evidence = fixture['behaviorEvidence']
    profile_ref = digest(json.dumps(['qualification-only', profile, key, seats]).encode())
    scope = fixture.get('qualifiedScope', {})
    if scope:
        assert seats==2 and key=='legacy' and set(scope['cardEvidence'])=={k for k,_ in cards}
        profile_ref=scope['profileEvidence']
    manifest = {'schema':'spellwake-accepted-constructed-v1','releaseId':release,'legalityRevisionId':legality,
                'profiles':[{'formatId':format_id,'formatKey':key,'profileId':profile,'profileRevision':scope.get('profileRevision','fixture'),
                             'profileRules':scope.get('profileRules','fixture'),'profileEvidence':profile_ref,'minimumSeats':'2','seatCapacity':str(seats)}],
                'cards':[{'oracleId':oracle,'definitionRevisionId':r['revision'],'program':r['programRef'],
                          'behaviorEvidence':scope.get('cardEvidence',{}).get(oracle,evidence)} for oracle,r in cards]}
    if 'tablePolicy' in scope:
        manifest['schema']='spellwake-accepted-constructed-v2'
        manifest['profiles'][0]['tablePolicy']=scope['tablePolicy']
    content = json.dumps(manifest,sort_keys=True,separators=(',',':')).encode()
    file = realm.stage / 'qualified-room-manifest.json'
    private_write(file,content)
    pin = digest(content)
    with socket.socket() as available:
        available.bind(('127.0.0.1',0))
        port = available.getsockname()[1]
    child = realm.start_background([str(fixture['binary'])],fixture['environment'] | {
        'SPELLWAKE_API_BIND':f'127.0.0.1:{port}','SPELLWAKE_NATIVE_ADMISSION_MANIFEST':str(file),
        'SPELLWAKE_NATIVE_ADMISSION_MANIFEST_SHA256':pin})
    deadline = time.monotonic()+fixture.get('startupTimeoutSeconds',240)
    while True:
        if child.poll() is not None or time.monotonic()>deadline:
            raise RuntimeError('The owned admission API failed to start')
        try:
            with socket.create_connection(('127.0.0.1',port),timeout=1):
                break
        except OSError:
            time.sleep(.2)
    fixture['base'] = f'http://127.0.0.1:{port}'
    def post(room, operation, original, token_index, expected):
        uri = f'http://127.0.0.1:{port}/v1/rooms/{room}/{operation}'
        request = urllib.request.Request(uri,data=json.dumps(original).encode(),headers={
            'content-type':'application/json','authorization':'Bearer '+fixture['tokens'][token_index]})
        status, result, headers = request_json_with_headers(make_opener(),request,timeout=120)
        if status!=expected or headers.get('cache-control')!='no-store':
            raise RuntimeError(f'Owned admission API {operation} returned {status}, expected {expected}')
        return result
    def execute(room,kind,original,saved=None,missing_legality=False):
        path = 'ready' if kind=='ready' else 'force-start'
        signer = fixture.get('readyPlayer', 0) if kind == 'ready' else 0
        if missing_legality:
            expected={'deck':'qualified selected revision required'} if kind=='ready' else {'decks':'qualification required'}
            if post(room,path,original,signer,409)!={'status':'fail','data':expected}:
                raise RuntimeError('The database legality refusal did not reach the public room command')
            expected_disposition={'status':'success','data':{'state':'rejected','response':{'status':'fail','data':expected}}}
            if post(room,path+'/disposition',original,signer,200)!=expected_disposition:
                raise RuntimeError('Rejected room original was not durably recoverable')
            return None
        if saved is not None:
            if 'readyPlayer' in fixture:
                unknown = {'status':'success','data':{'state':'unknown','response':None}}
                for request, reader in [(original,2), (original | {'requestId':str(uuid.uuid4())},signer)]:
                    if post(room,path+'/disposition',request,reader,200) != unknown:
                        raise RuntimeError('An unrelated original returned private acknowledgement data')
                disposition = {'status':'success','data':{'state':'accepted','response':{'status':'success','data':saved}}}
                if post(room,path+'/disposition',original,signer,200) != disposition:
                    raise RuntimeError('Owner could not recover the original after match start')
            recovered=post(room,path,original,signer,202)
            if recovered!={'status':'success','data':saved}:
                raise RuntimeError('Post-start execution differed from the stored room acknowledgement')
            return saved
        if kind=='countdown':
            post(room,path,original | {'requestId':str(uuid.uuid4())},1,409)
        post(room,path,original | {'releaseId':release},signer,400)
        barrier=Barrier(2)
        def simultaneous():
            barrier.wait(timeout=10)
            return post(room,path,original,signer,202)
        with ThreadPoolExecutor(max_workers=2) as executor:
            left,right=[executor.submit(simultaneous) for _ in range(2)]
            first=left.result()
            if first!=right.result():
                raise RuntimeError('Concurrent public requests returned different acknowledgements')
        if first.get('status')!='success' or first!=post(room,path,original,signer,202):
            raise RuntimeError('Owned HTTP admission retry changed the original result')
        changed = original | {'expectedRevision':str(int(original['expectedRevision'])+1)}
        if post(room,path,changed,signer,409)!={'status':'fail','data':{'request':'conflict'}}:
            raise RuntimeError('A changed original did not produce a request conflict')
        if 'readyPlayer' in fixture:
            stale = original | {'requestId':str(uuid.uuid4())}
            def stored_room_state():
                return realm.sql(f"SELECT jsonb_build_array(r.revision,r.state,(SELECT count(*) FROM app.command_receipt WHERE target_ref=r.id::text)) FROM app.room r WHERE r.id='{uuid.UUID(room)}'")
            before = stored_room_state()
            expected_error = {'status':'error','code':'unavailable','message':'Application service is temporarily unavailable'}
            if post(room,path,stale,signer,503) != expected_error:
                raise RuntimeError('Unexpected response to a new command on a superseded room revision')
            if post(room,path+'/disposition',stale,signer,200) != {'status':'success','data':{'state':'unknown','response':None}}:
                raise RuntimeError('Unprepared stale command acquired a durable original')
            if stored_room_state() != before:
                raise RuntimeError('Unprepared stale command mutated the room or its receipts')
        if first['data'].get('resourceId')!=room:
            raise RuntimeError('Owned room acknowledgement changed target')
        return first['data']
    return pin,profile_ref,execute
