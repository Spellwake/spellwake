"""Disposable Postgres role/context probe, not GoTrue or login qualification."""
import hashlib
import json
from pathlib import Path
import uuid

from disposable_postgres import DisposablePostgres

ROOT=Path(__file__).resolve().parents[1]


def main() -> None:
    """Create/drop only this invocation's UUID database and two temporary roles."""
    source=ROOT/'qualification/auth-context-probe.sql'
    checks=[]
    realm=DisposablePostgres('auth_context_probe',('auth_probe','gateway_probe'))
    trusted=realm.roles['auth_probe'];gateway=realm.roles['gateway_probe']
    psql=realm.sql

    def denied(expression: str, code: str='28000') -> str:
        return f"DO $test$ BEGIN BEGIN PERFORM {expression}; RAISE EXCEPTION 'unexpected probe acceptance'; EXCEPTION WHEN SQLSTATE '{code}' THEN NULL; END; END $test$;"

    with realm:
        psql(f'GRANT authenticated TO {gateway};',database='postgres')
        psql(source.read_text().replace('__TRUSTED_ROLE__',trusted).replace('__GATEWAY_ROLE__',gateway))
        actor=str(uuid.uuid4());first=str(uuid.uuid4());second=str(uuid.uuid4())
        payload=json.dumps({'actor_id':actor,'provider':'google'})
        insert=f"INSERT INTO probe.audit_source(id,payload) VALUES('{first}','{payload}'::jsonb);"
        fresh=json.loads(psql(f"BEGIN; {insert} SELECT probe.check_context('{actor}'); COMMIT;",trusted))
        if fresh['source']!=first or fresh['provider']!='google' or fresh['login']!=trusted:
            raise RuntimeError('Fresh source context differs')
        checks.append({'case':'actual_login_and_fresh_source','passed':True,'observed':fresh})
        psql(denied(f"probe.check_context('{actor}')"),trusted)
        checks.append({'case':'next_connection_without_event_denied','passed':True})
        # Same connection commit/reset and savepoint rollback are the important
        # pool/nested-transaction cases, not only opening a fresh connection.
        payload2=json.dumps({'actor_id':actor,'provider':'apple'})
        sequence=f"""BEGIN;
INSERT INTO probe.audit_source(id,payload) VALUES('{second}','{payload2}'::jsonb);
SELECT probe.check_context('{actor}');
COMMIT;
{denied(f"probe.check_context('{actor}')")}
BEGIN; SAVEPOINT before_event;
INSERT INTO probe.audit_source(id,payload) VALUES('{uuid.uuid4()}','{payload}'::jsonb);
ROLLBACK TO SAVEPOINT before_event;
{denied(f"probe.check_context('{actor}')")}
COMMIT;"""
        json.loads(psql(sequence,trusted))
        checks.append({'case':'same_connection_commit_and_savepoint_reset','passed':True})
        outer=str(uuid.uuid4());inner=str(uuid.uuid4())
        nested=f"""BEGIN;
INSERT INTO probe.audit_source(id,payload) VALUES('{outer}','{payload}'::jsonb);
SAVEPOINT inner_event;
INSERT INTO probe.audit_source(id,payload) VALUES('{inner}','{payload2}'::jsonb);
ROLLBACK TO SAVEPOINT inner_event;
SELECT probe.check_context('{actor}');
COMMIT;"""
        if json.loads(psql(nested,trusted))['source']!=outer:
            raise RuntimeError('Savepoint did not restore prior valid context')
        checks.append({'case':'nested_rollback_restores_outer_source','passed':True})
        stale_id=str(uuid.uuid4())
        stale=psql(f"BEGIN; INSERT INTO probe.audit_source(id,payload) VALUES('{stale_id}','{payload}'::jsonb); SELECT current_setting('spellwake.probe_auth_context'); COMMIT;",trusted)
        stale_literal=stale.replace("'","''")
        psql(f"BEGIN; SELECT set_config('spellwake.probe_auth_context','{stale_literal}',true); {denied(f'probe.check_context(\'{actor}\')')} COMMIT;",trusted)
        checks.append({'case':'prior_transaction_context_replay_denied','passed':True})
        wrong=str(uuid.uuid4());source_id=str(uuid.uuid4())
        psql(f"BEGIN; INSERT INTO probe.audit_source(id,payload) VALUES('{source_id}','{payload}'::jsonb); {denied(f'probe.check_context(\'{wrong}\')')} ROLLBACK;",trusted)
        checks.append({'case':'different_subject_denied','passed':True})
        psql(f'GRANT UPDATE,DELETE ON probe.audit_source TO {trusted};')
        for label,mutation in [('changed_source_payload',f"UPDATE probe.audit_source SET payload='{{\"actor_id\":\"{actor}\",\"provider\":\"discord\"}}'::jsonb WHERE id='SOURCE_ID';"),('missing_source',"DELETE FROM probe.audit_source WHERE id='SOURCE_ID';")]:
            source_id=str(uuid.uuid4())
            psql(f"BEGIN; INSERT INTO probe.audit_source(id,payload) VALUES('{source_id}','{payload}'::jsonb); {mutation.replace('SOURCE_ID',source_id)} {denied(f'probe.check_context(\'{actor}\')')} ROLLBACK;",trusted)
            checks.append({'case':label+'_denied','passed':True})
        psql(denied(f"probe.check_context('{actor}')",'42501'),gateway)
        checks.append({'case':'untrusted_login_execute_denied','passed':True})
        # Deliberately widen only this disposable probe's hook ACL to show that
        # caller-controlled settings/SET ROLE still cannot change session_user.
        psql('GRANT EXECUTE ON FUNCTION probe.check_context(uuid) TO authenticated;')
        forged=f"jsonb_set('{stale_literal}'::jsonb,'{{xact}}',to_jsonb(pg_current_xact_id()::text))::text"
        psql(f"SET ROLE authenticated; BEGIN; SELECT set_config('spellwake.probe_auth_context',{forged},true); {denied(f'probe.check_context(\'{actor}\')')} COMMIT;",gateway)
        checks.append({'case':'forged_setting_and_set_role_denied_by_actual_login','passed':True})
        psql('REVOKE EXECUTE ON FUNCTION probe.check_context(uuid) FROM authenticated;')
        report={'scope':'synthetic Postgres context primitive only','sourceSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'runnerSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'checks':checks,'excluded':['GoTrue producer truth','real provider/email proof','binding/session issuance','concurrent callbacks','registration/RLS/final deletion','hosted support']}
    report['cleanupVerified']=realm.cleanup_verified
    report['lifecycleSha256']=hashlib.sha256((ROOT/'tools/disposable_postgres.py').read_bytes()).hexdigest()
    path=ROOT/'target/auth-context-probe-result.json';path.write_text(json.dumps(report,indent=2)+'\n')
    print('Disposable Auth context probe passed:',len(checks),'cases and cleanup; no GoTrue/hook activation claim')


if __name__=='__main__':
    main()
