"""Read effective asset write authority in one fresh owned managed-local realm."""
import hashlib
import json
from typing import Callable

from disposable_supabase import DisposableSupabase, ROOT, private_write

ROLES = ['anon', 'authenticated', 'service_role', 'supabase_auth_admin']
TABLES = ['asset_owner', 'inventory_asset', 'ledger_operation', 'ledger_posting', 'asset_hold', 'hold_line']



def read_asset_authority(sql: Callable[[str],str]) -> dict:
    """Read only catalog metadata; collection alone does not assert lockdown."""
    result: dict = {}
    roles = 'ARRAY[' + ','.join("'" + role + "'" for role in ROLES) + ']'
    tables = 'ARRAY[' + ','.join("'" + table + "'" for table in TABLES) + ']'
    result['tablePrivileges'] = json.loads(sql("SELECT jsonb_agg(jsonb_build_object('role',r,'table',t,'privileges',"
        "jsonb_build_object('select',has_table_privilege(r,'app.'||t,'SELECT'),'insert',has_table_privilege(r,'app.'||t,'INSERT'),"
        "'update',has_table_privilege(r,'app.'||t,'UPDATE'),'delete',has_table_privilege(r,'app.'||t,'DELETE'),"
        "'truncate',has_table_privilege(r,'app.'||t,'TRUNCATE'),'references',has_table_privilege(r,'app.'||t,'REFERENCES'),"
        "'trigger',has_table_privilege(r,'app.'||t,'TRIGGER'),'maintain',has_table_privilege(r,'app.'||t,'MAINTAIN'))) ORDER BY r,t) "
        "FROM unnest("+roles+") r CROSS JOIN unnest("+tables+") t"))
    result['functionPrivileges'] = json.loads(sql("SELECT jsonb_agg(jsonb_build_object('role',r,'name',p.proname,"
        "'signature',pg_get_function_identity_arguments(p.oid),'execute',has_function_privilege(r,p.oid,'EXECUTE'),"
        "'owner',pg_get_userbyid(p.proowner),'definer',p.prosecdef) ORDER BY r,p.proname,p.oid) "
        "FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace CROSS JOIN unnest("+roles+") r WHERE n.nspname='app'"))
    result['defaultPrivileges'] = json.loads(sql("SELECT coalesce(jsonb_agg(jsonb_build_object('owner',pg_get_userbyid(d.defaclrole),"
        "'schema',n.nspname,'objectType',d.defaclobjtype,'grantee',CASE WHEN x.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x.grantee) END,"
        "'privilege',x.privilege_type,'grantable',x.is_grantable) ORDER BY d.oid,x.grantee,x.privilege_type),'[]') "
        "FROM pg_default_acl d LEFT JOIN pg_namespace n ON n.oid=d.defaclnamespace CROSS JOIN LATERAL aclexplode(d.defaclacl) x "
        "WHERE d.defaclnamespace=0 OR n.nspname='app'"))
    result['roleAuthority'] = json.loads(sql("SELECT jsonb_agg(jsonb_build_object('role',r.rolname,'superuser',r.rolsuper,"
        "'bypassRls',r.rolbypassrls,'memberships',(SELECT coalesce(jsonb_agg(parent.rolname ORDER BY parent.rolname),'[]') "
        "FROM pg_roles parent WHERE parent.oid<>r.oid AND pg_has_role(r.oid,parent.oid,'MEMBER'))) ORDER BY r.rolname) "
        "FROM pg_roles r WHERE r.rolname=ANY("+roles+")"))
    result['assetTriggers'] = json.loads(sql("SELECT jsonb_agg(jsonb_build_object('table',c.relname,'name',t.tgname,'enabled',t.tgenabled,'before',(t.tgtype&2)<>0,'row',(t.tgtype&1)<>0,'function',p.proname) ORDER BY c.relname,t.tgname) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_proc p ON p.oid=t.tgfoid WHERE n.nspname='app' AND c.relname IN ('asset_owner','inventory_asset','ledger_operation','ledger_posting','asset_hold','hold_line') AND NOT t.tgisinternal"))
    return result


def assert_retired_asset_authority(catalog: dict) -> None:
    """Require the complete060 matrix, including non-DML and helper authority."""
    rows=catalog['tablePrivileges']
    if len(rows)!=len(ROLES)*len(TABLES) or {(r['role'],r['table']) for r in rows}!={(r,t) for r in ROLES for t in TABLES}:
        raise RuntimeError('Asset table authority matrix is incomplete')
    privileges={'select','insert','update','delete','truncate','references','trigger','maintain'}
    for row in rows:
        expected_read=row['role']=='service_role' or (row['role']=='authenticated' and row['table'] in ('asset_owner','inventory_asset','asset_hold','hold_line'))
        actual=row['privileges']
        if set(actual)!=privileges or any(type(v) is not bool for v in actual.values()) or actual!={p:(p=='select' and expected_read) for p in privileges}:
            raise RuntimeError('Asset raw authority is not closed')
    private={'lock_asset_account_for_write','guard_asset_owner_lifecycle','guard_asset_write_lifecycle','ensure_inventory',
             'prepare_exchange_inventory','apply_exchange','lock_prediction_market','refresh_prediction_hold',
             'cancel_prediction_unfilled','apply_ledger_posting','check_hold_line','check_hold_transition'}
    functions=[r for r in catalog['functionPrivileges'] if r['name'] in private]
    if len(functions)!=len(ROLES)*len(private) or {(r['role'],r['name']) for r in functions}!={(r,n) for r in ROLES for n in private} or any(r['execute'] is not False for r in functions):
        raise RuntimeError('Private asset helper authority is not closed')
    if any(r['superuser'] for r in catalog['roleAuthority']):
        raise RuntimeError('Asset audit role has ownership bypass')
    triggers=catalog['assetTriggers']
    for table,name in [('asset_owner','asset_owner_lifecycle'),('inventory_asset','asset_lifecycle_inventory'),('ledger_posting','asset_lifecycle_posting'),('asset_hold','asset_lifecycle_hold'),('hold_line','asset_lifecycle_hold_line')]:
        before=[t for t in triggers if t['table']==table and t['before']]
        if not before or min(t['name'] for t in before)!=name or not any(t['name']==name and t['enabled']=='O' and t['row'] for t in before):
            raise RuntimeError('Asset lifecycle trigger ordering differs')
    if not any(t['table']=='ledger_posting' and t['name']=='ledger_posting_apply' and not t['before'] and t['enabled']=='O' and t['row'] for t in triggers):
        raise RuntimeError('Actual-insert-only ledger application was not preserved')


def main() -> None:
    """Keep CLI credentials private; never start or mutate a retained project."""
    realm = DisposableSupabase()
    paths = ['tools/audit_asset_authority.py', 'tools/disposable_supabase.py', 'tools/loopback_docker.py']
    paths += [str(path.relative_to(ROOT)) for path in sorted((ROOT / 'supabase/migrations').glob('*.sql'))]
    hashes = {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in paths}
    result: dict = {'scope': 'owned-managed-asset-effective-acl-read-only', 'passed': False,
                    'cleanupVerified': False, 'sourceHashes': hashes}
    failure = None
    try:
        realm.__enter__()
        result.update(read_asset_authority(realm.sql))
        versions = json.loads(realm.sql("SELECT jsonb_agg(version ORDER BY version) FROM supabase_migrations.schema_migrations"))
        if versions != [row['file'].split('_',1)[0] for row in realm.manifest]:
            raise RuntimeError('Owned audit migration history differs')
        if hashes != {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in paths}:
            raise RuntimeError('Owned asset audit inputs changed')
        result.update(passed=True, migrations=realm.manifest, runtime=realm.runtime)
    except BaseException as error:
        failure = error
        result['primaryFailure'] = type(error).__name__
        private_write(realm.stage / 'private-asset-audit-failure.txt', str(error).encode())
    finally:
        try:
            realm.__exit__()
            result['cleanupVerified'] = realm.cleanup_verified
        except BaseException as error:
            result['cleanupFailure'] = type(error).__name__
        private_write(realm.stage / 'asset-authority-audit.json', json.dumps(result,indent=2).encode())
    if failure is not None or not result['cleanupVerified']:
        raise RuntimeError('Owned asset authority audit failed; private result: '+str(realm.stage / 'asset-authority-audit.json')) from None
    print('Owned asset authority audit complete: '+str(realm.stage / 'asset-authority-audit.json'))


if __name__ == '__main__':
    main()
