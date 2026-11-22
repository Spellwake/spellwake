"""Read pending diagnostics through real local PostgREST with owner-seeded pending state."""
from datetime import datetime
import fcntl
import json
import urllib.error
import urllib.request
import uuid
from test_local_storage import ROOT, BASE, NoRedirect
from test_local_extraction import sql, literal
from test_local_deletion_database import read_rpc


def main() -> None:
    """Own only four fixture roots; leave live source receipts/evidence untouched."""
    if 'project_id = "spellwake-application-local"' not in (ROOT/'supabase/config.toml').read_text():
        raise RuntimeError('Unexpected owned local project')
    state=json.loads((ROOT/'target/local-supabase-status.json').read_text())
    if state.get('API_URL')!=BASE or not state.get('SECRET_KEY','').startswith('sb_secret_'):
        raise RuntimeError('Unexpected owned local backend')
    with (ROOT/'target/local-storage-test.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        if sql('SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM auth.users)')!='0':
            raise RuntimeError('Status qualification requires an empty owned account realm')
        actor,subject,cosmetic,purchase=[str(uuid.uuid4()) for _ in range(4)]
        q=literal
        opener=urllib.request.build_opener(NoRedirect())
        try:
            sql(f"""BEGIN;
INSERT INTO auth.users(id) VALUES({q(subject)});
INSERT INTO app.account(id,auth_subject,username,sign_in_method,created_at,state) VALUES({q(actor)},{q(subject)},'fixture_'||left(replace({q(actor)},'-',''),16),'email_code',clock_timestamp(),'active');
INSERT INTO app.cosmetic(id,kind,asset_ref,state) VALUES({q(cosmetic)},'back','fixture-only','active');
INSERT INTO app.purchase(id,account_id,cosmetic_id,provider_request_ref,requested_at,state) VALUES({q(purchase)},{q(actor)},{q(cosmetic)},{q(purchase)},clock_timestamp(),'creating');
COMMIT;""")
            # Owner-only pending fixture: this reader test does not qualify Auth admission.
            sql(f"UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp() WHERE id={q(actor)}")
            status=read_rpc(opener,state,'read_account_deletion_status',{'p_actor':actor})
            expected={name:'0' for name in ['uncommittedWork','unsettledPredictionMarkets','purchasesInFlight','verifiedPatreonEvents','pendingStripeEvents','matchesWithoutResult','heldCommitments','pendingCommands']}
            expected['purchasesInFlight']='1'
            if not isinstance(status,dict) or set(status)!={'state','requestedAt','obligations'} or status['state']!='deletion_pending' or status['obligations']!=expected:
                raise RuntimeError('Unexpected exact status counters')
            if datetime.fromisoformat(status['requestedAt'].replace('Z','+00:00')).utcoffset().total_seconds()!=0:
                raise RuntimeError('Status request time is not UTC')
            public={'SECRET_KEY':state['PUBLISHABLE_KEY']}
            try:
                read_rpc(opener,public,'read_account_deletion_status',{'p_actor':actor})
            except RuntimeError as error:
                if str(error) not in ['Local deletion fixture RPC failed: 401','Local deletion fixture RPC failed: 403']:
                    raise
            else:
                raise RuntimeError('Public key accessed private deletion status')
            if sql(f'SELECT state FROM app.purchase WHERE id={q(purchase)}')!='creating':
                raise RuntimeError('Read changed a payment obligation')
        finally:
            sql(f"""BEGIN; SET LOCAL session_replication_role=replica;
DELETE FROM app.command_receipt WHERE account_id={q(actor)};
DELETE FROM app.purchase WHERE id={q(purchase)};
DELETE FROM app.cosmetic WHERE id={q(cosmetic)};
DELETE FROM app.account WHERE id={q(actor)};
DELETE FROM auth.users WHERE id={q(subject)};
COMMIT;""")
        if sql('SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM auth.users)')!='0':
            raise RuntimeError('Status fixture cleanup left account roots')
    print('Real pending diagnostics passed: exact pending purchase count, UTC, public-key denial, preserved obligation and scoped cleanup. No Auth/finalization claim.')


if __name__=='__main__':
    main()
