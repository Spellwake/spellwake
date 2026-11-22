"""Read canonical native originals through real Rust HTTP/Auth/managed SQL.

The retained SQL fixture establishes receipt ownership, not native game execution.
"""
import copy
import uuid
from typing import Callable
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def qualify_native_receipt_api(realm: DisposableSupabase, request: Callable,
                              request_with_headers: Callable, expect: Callable,
                              owner_token: str, peer_token: str, fresh_identity: Callable,
                              fixture: dict) -> dict:
    """Verify historical data/correlation/denials and no new game work or receipts."""
    samples=[item for item in fixture['wireSamples'] if item['operation']=='read_native_human_receipts' and item['response'].get('status')=='success']
    if len(samples)<2:
        raise RuntimeError('Owned native receipt source cases are missing')
    original_query=copy.deepcopy(samples[0]['request']['p_query'])
    game=str(uuid.UUID(original_query['gameId']))
    path='/v1/games/'+game+'/receipts'
    snapshot=("SELECT jsonb_build_object('game',(SELECT to_jsonb(g) FROM app.game g WHERE id='"+game+"'),"
              "'events',(SELECT count(*) FROM app.game_event WHERE game_id='"+game+"'),"
              "'checkpoints',(SELECT count(*) FROM app.game_checkpoint WHERE game_id='"+game+"'),"
              "'receipts',(SELECT count(*) FROM app.command_receipt WHERE target_ref='"+game+"'))")
    before=realm.sql(snapshot)
    cases=0
    for sample in samples:
        query=sample['request']['p_query']
        expected=[]
        for raw in sample['response']['data']['receipts']:
            response=copy.deepcopy(raw['response'])
            if raw['state']=='accepted':
                ack=response['data']
                response['data']={'requestId':ack['requestId'],'actorSeatId':ack['actorSeatId'],
                    'basis':{key:ack[key] for key in ('gameId','contextId','epoch','revision')}}
            expected.append({'actorSeatId':raw['actorSeatId'],'original':raw['original'],
                'disposition':{'state':raw['state'],'response':response}})
        status,payload,headers=request_with_headers(path,owner_token,query)
        actual=expect('native-receipts-historical-'+sample['response']['data']['receipts'][0]['state'],(status,payload),200)
        if object_json(actual['data'])!={'receipts':expected} or headers.get('cache-control')!='no-store':
            raise RuntimeError('Owned native receipt HTTP changed original data or caching policy')
        cases+=1
    unknown=copy.deepcopy(original_query)
    unknown['outstanding'][0]['requestId']=str(uuid.uuid4())
    result=expect('native-receipts-unknown',request(path,owner_token,unknown),200)
    if result['data']['receipts'][0]['disposition']!={'state':'unknown','response':None}:
        raise RuntimeError('Unknown native original was treated as committed')
    peer=expect('native-receipts-peer-namespace',request(path,peer_token,original_query),200)
    if peer['data']['receipts'][0]['disposition']!={'state':'unknown','response':None}:
        raise RuntimeError('Peer recovered another participant original')
    changed=copy.deepcopy(original_query)
    kind=changed['outstanding'][0]['action']['kind']
    changed['outstanding'][0]['action']={'kind':'pass' if kind!='pass' else 'concede'}
    expect('native-receipts-conflict',request(path,owner_token,changed),409)
    expect('native-receipts-anonymous',request(path,None,original_query),401)
    invalid=copy.deepcopy(original_query);invalid['actorId']=str(uuid.uuid4())
    expect('native-receipts-actor-injection',request(path,owner_token,invalid),400)
    mismatch=copy.deepcopy(original_query);mismatch['gameId']=str(uuid.uuid4())
    expect('native-receipts-path-mismatch',request(path,owner_token,mismatch),400)
    context=copy.deepcopy(original_query);context['contextId']=str(uuid.uuid4())
    for original in context['outstanding']:
        original['basis']['contextId']=context['contextId']
    expect('native-receipts-unavailable-context',request(path,owner_token,context),404)
    outsider=fresh_identity()
    expect('native-receipts-register-outsider',request('/v1/account',outsider,{'username':'Receipt'+uuid.uuid4().hex[:12]}),200)
    expect('native-receipts-outsider',request(path,outsider,original_query),404)
    cases+=9
    if realm.sql(snapshot)!=before:
        raise RuntimeError('Read-only native receipt HTTP changed game work or command receipts')
    return {'passed':True,'executed':True,'cases':cases,'historicalReadOnly':True,
            'actualRustHttpAuthAndPostgrest':True,'nativeExecutionQualified':False}
