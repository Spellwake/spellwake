"""Real local Supabase/PostgREST/Temporal cancellation of owner-seeded pending work.

Owner-only fixture issuance and teardown are not production acquisition/deletion.
Only this run's UUID-linked rows are removed, never an application reset.
"""
import json
import os
from pathlib import Path
from typing import Callable
import urllib.error
import urllib.request
import uuid

from test_local_storage import ROOT, BASE, NoRedirect
from test_local_extraction import sql, literal
from test_local_deletion_delivery import qualify_worker


def read_rpc(opener: urllib.request.OpenerDirector, state: dict, name: str, payload: dict) -> object:
    request = urllib.request.Request(BASE + '/rest/v1/rpc/' + name, method='POST', data=json.dumps(payload).encode(),
        headers={'apikey':state['SECRET_KEY'],'Content-Type':'application/json','Content-Profile':'app','Accept-Profile':'app'})
    try:
        with opener.open(request, timeout=60) as response:
            body = response.read(1048577)
            if len(body)>1048576:
                raise RuntimeError('Oversized fixture RPC response')
            result = json.loads(body)
    except urllib.error.HTTPError as error:
        raise RuntimeError('Local deletion fixture RPC failed: '+str(error.code)) from None
    return result

def qualify_database(root: Path, stage: Path, namespace: str, cli: Callable[..., dict], stop: Callable) -> None:
    """Require an empty owned project, exercise normal RPC writers, then clean up."""
    if root != ROOT or 'project_id = "spellwake-application-local"' not in (root / 'supabase/config.toml').read_text():
        raise RuntimeError('Unexpected local project')
    state = json.loads((root / 'target/local-supabase-status.json').read_text())
    if state.get('API_URL') != BASE or not state.get('SECRET_KEY', '').startswith('sb_secret_'):
        raise RuntimeError('Refusing a nonlocal backend target')
    if sql("SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM auth.users)+(SELECT count(*) FROM app.prediction_market)+(SELECT count(*) FROM app.inventory_asset)+(SELECT count(*) FROM app.price_observation)") != '0':
        raise RuntimeError('Deletion qualification requires an empty owned application')
    ids = {name: str(uuid.uuid4()) for name in ['actor','bob','subject','bob_subject','set','definition','printing','card','cosmetic','purchase','market']}
    q = lambda name: literal(ids[name])
    actors = q('actor') + ',' + q('bob')
    prior_coin = sql("SELECT id FROM app.asset_definition WHERE currency_key='coin'")
    coin = prior_coin or str(uuid.uuid4())
    opener = urllib.request.build_opener(NoRedirect())

    def rpc(name: str, payload: dict) -> dict:
        result=read_rpc(opener,state,name,payload)
        if not isinstance(result,dict) or result.get('status')!='success':
            raise RuntimeError('Local deletion fixture command rejected')
        return result['data']

    try:
        seed = ['BEGIN;']
        for actor,subject in [('actor','subject'),('bob','bob_subject')]:
            seed += [f'INSERT INTO auth.users(id) VALUES({q(subject)});',
                f"INSERT INTO app.account(id,auth_subject,username,sign_in_method,created_at,state) VALUES({q(actor)},{q(subject)},'fixture_'||left(replace({q(actor)},'-',''),16),'email_code',clock_timestamp(),'active');",
                f"INSERT INTO app.asset_owner(id,account_id,state) VALUES({q(actor)},{q(actor)},'active');"]
        seed += [f"INSERT INTO app.card_set(id,external_set_ref,name) VALUES({q('set')},{q('set')},'Synthetic deletion fixture');",
            f"INSERT INTO app.card_definition(id,oracle_ref,name) VALUES({q('definition')},{q('definition')},'Synthetic fixture');",
            f"INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES({q('printing')},{q('definition')},{q('set')},{q('printing')},'rare','nonfoil','fixture-only');",
            f"INSERT INTO app.asset_definition(id,kind,printing_id) VALUES({q('card')},'card',{q('printing')});"]
        if not prior_coin:
            seed.append(f"INSERT INTO app.asset_definition(id,kind,currency_key) VALUES({literal(coin)},'coin','coin');")
        for actor in ['actor','bob']:
            operation = str(uuid.uuid4())
            seed += [f"INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES({literal(operation)},'test',{literal(operation)},'test',{q(actor)},clock_timestamp(),'pending');",
                f"INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) SELECT {literal(operation)},app.ensure_inventory({q(actor)},{literal(coin)}),'credit',14500,'test',{literal(operation)};",
                f"UPDATE app.ledger_operation SET state='committed' WHERE id={literal(operation)};"]
        operation = str(uuid.uuid4())
        seed += [f"INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES({literal(operation)},'test',{literal(operation)},'test',{q('actor')},clock_timestamp(),'pending');",
            f"INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) SELECT {literal(operation)},app.ensure_inventory({q('actor')},{q('card')}),'credit',2,'test',{literal(operation)};",
            f"UPDATE app.ledger_operation SET state='committed' WHERE id={literal(operation)};",
            f"INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES({q('cosmetic')},'back','fixture-only','active');",
            f"INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES({q('purchase')},{q('actor')},{q('cosmetic')},{q('purchase')},clock_timestamp(),'creating');",
            f"INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state) VALUES({q('market')},{q('bob')},{q('printing')},'fixture-only:usd','10.00','reached_by',clock_timestamp()+interval '2 days',clock_timestamp()+interval '1 day',clock_timestamp()+interval '25 hours','fixture-only:sampling','fixture-only:terminal','fixture-only:v1',100,1,'open');",
            'COMMIT;']
        sql('\n'.join(seed))
        rpc('change_friendship',{'p_actor':ids['actor'],'p_other':ids['bob'],'p_action':'request','p_request':str(uuid.uuid4())})
        rpc('change_friendship',{'p_actor':ids['bob'],'p_other':ids['actor'],'p_action':'accept','p_request':str(uuid.uuid4())})
        bundle=[{'assetId':coin,'quantity':'100'}]
        outgoing=rpc('create_trade',{'p_actor':ids['actor'],'p_recipient':ids['bob'],'p_give':bundle,'p_want':bundle,'p_request':str(uuid.uuid4())})['resourceId']
        incoming=rpc('create_trade',{'p_actor':ids['bob'],'p_recipient':ids['actor'],'p_give':bundle,'p_want':bundle,'p_request':str(uuid.uuid4())})['resourceId']
        listing=rpc('create_listing',{'p_actor':ids['actor'],'p_asset':ids['card'],'p_quantity':'1','p_price':'10','p_request':str(uuid.uuid4())})['resourceId']
        order=rpc('place_prediction_order',{'p_actor':ids['actor'],'p_market':ids['market'],'p_side':'yes','p_limit':'60','p_quantity':'3','p_request':str(uuid.uuid4())})['resourceId']
        rpc('place_prediction_order',{'p_actor':ids['bob'],'p_market':ids['market'],'p_side':'no','p_limit':'40','p_quantity':'1','p_request':str(uuid.uuid4())})
        # Synthetic pending marker only; authenticated request acceptance belongs to the owned Auth/API driver.
        sql(f"UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id={q('actor')}")
        verify_sql=f"""SELECT (SELECT state='canceled' FROM app.trade_offer WHERE id={literal(outgoing)})
 AND (SELECT state='rejected' FROM app.trade_offer WHERE id={literal(incoming)})
 AND (SELECT state='canceled' FROM app.market_listing WHERE id={literal(listing)})
 AND EXISTS(SELECT 1 FROM app.prediction_order o JOIN app.asset_hold h ON h.id=o.hold_id JOIN app.hold_line l ON l.hold_id=h.id WHERE o.id={literal(order)} AND o.state='canceled' AND o.filled_quantity=1 AND h.state='held' AND l.quantity=60)
 AND (SELECT count(*)=1 FROM app.prediction_pair WHERE market_id={q('market')})
 AND (SELECT state='creating' FROM app.purchase WHERE id={q('purchase')})
 AND (SELECT state='deletion_pending' AND retirement_operation_id IS NULL AND deletion_finalized_at IS NULL AND auth_subject={q('subject')} FROM app.account WHERE id={q('actor')})
 AND EXISTS(SELECT 1 FROM app.inventory_asset i JOIN app.asset_availability a ON a.id=i.id WHERE i.owner_id={q('actor')} AND i.asset_definition_id={literal(coin)} AND a.owned=14500 AND a.held=60 AND a.available=14440)
 AND EXISTS(SELECT 1 FROM app.inventory_asset i JOIN app.asset_availability a ON a.id=i.id WHERE i.owner_id={q('actor')} AND i.asset_definition_id={q('card')} AND a.owned=2 AND a.held=0 AND a.available=2)"""
        environment=os.environ.copy()
        environment.update(SPELLWAKE_SUPABASE_URL=BASE,SPELLWAKE_SUPABASE_SECRET_KEY=state['SECRET_KEY'])
        qualify_worker(root,stage,namespace,cli,stop,environment,lambda:sql(verify_sql)=='t')
        if sql(verify_sql)!='t' or sql(f"SELECT app.read_deletion_work({q('actor')},100)")!='[]':
            raise RuntimeError('Real deletion cleanup did not preserve exact obligations')
    finally:
        # Privileged teardown only of UUID-owned fixture roots and their children.
        # Runtime worker has already stopped before immutable history is removed.
        cleanup=["BEGIN; SET LOCAL session_replication_role=replica;",
            f'DELETE FROM app.notification WHERE recipient_id IN ({actors});',
            f'DELETE FROM app.trade_line WHERE trade_id IN (SELECT id FROM app.trade_offer WHERE sender_id IN ({actors}) OR recipient_id IN ({actors}));',
            f'DELETE FROM app.trade_offer WHERE sender_id IN ({actors}) OR recipient_id IN ({actors});',
            f'DELETE FROM app.market_listing WHERE seller_id IN ({actors});',
            f"DELETE FROM app.prediction_pair WHERE market_id={q('market')};",
            f"DELETE FROM app.prediction_order WHERE market_id={q('market')};",
            f"DELETE FROM app.prediction_market WHERE id={q('market')};",
            f'DELETE FROM app.hold_line WHERE hold_id IN (SELECT id FROM app.asset_hold WHERE owner_id IN ({actors}));',
            f'DELETE FROM app.asset_hold WHERE owner_id IN ({actors});',
            f'DELETE FROM app.command_receipt WHERE account_id IN ({actors});',
            f'DELETE FROM app.ledger_posting WHERE operation_id IN (SELECT id FROM app.ledger_operation WHERE actor_account_id IN ({actors}));',
            f'DELETE FROM app.ledger_operation WHERE actor_account_id IN ({actors});',
            f'DELETE FROM app.inventory_asset WHERE owner_id IN ({actors});',
            f'DELETE FROM app.asset_owner WHERE account_id IN ({actors});',
            f'DELETE FROM app.friendship WHERE account_low_id IN ({actors}) OR account_high_id IN ({actors});',
            f'DELETE FROM app.block WHERE blocker_id IN ({actors}) OR blocked_id IN ({actors});',
            f"DELETE FROM app.purchase WHERE id={q('purchase')};",
            f"DELETE FROM app.cosmetic WHERE id={q('cosmetic')};",
            f'DELETE FROM app.account WHERE id IN ({actors});',
            f"DELETE FROM auth.users WHERE id IN ({q('subject')},{q('bob_subject')});",
            f"DELETE FROM app.asset_definition WHERE id={q('card')};",
            f"DELETE FROM app.printing WHERE id={q('printing')};",
            f"DELETE FROM app.card_definition WHERE id={q('definition')};",
            f"DELETE FROM app.card_set WHERE id={q('set')};"]
        if not prior_coin:
            cleanup.append(f'DELETE FROM app.asset_definition WHERE id={literal(coin)};')
        cleanup.append('COMMIT;')
        sql('\n'.join(cleanup))
    if sql("SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM auth.users)+(SELECT count(*) FROM app.prediction_market)+(SELECT count(*) FROM app.inventory_asset)+(SELECT count(*) FROM app.price_observation)")!='0':
        raise RuntimeError('Owned deletion fixture cleanup left application rows')
    print('Real Supabase deletion delivery passed: domain HTTP writers, owner-seeded pending marker, four cancellations, exact held backing/payment/account preservation, repeat installation/timer/cancellation and fixture cleanup. No Auth request/final deletion claim.')
