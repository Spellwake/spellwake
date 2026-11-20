"""Project verified real GoTrue claims through SQL roles, not HTTP middleware."""
import json
from typing import Callable
import uuid


def qualify_admission(claims: dict, sql: Callable[[str], str],
                      register_via_api: Callable[[list[str], str], str] | None = None) -> tuple[dict, Callable[[], None]]:
    """Exercise exact registration/RLS with actual signed-claim source facts."""
    subject = str(uuid.UUID(claims['sub']))
    session = str(uuid.UUID(claims['session_id']))
    ids = [str(uuid.uuid4()) for _ in range(3)]

    def literal(value: str) -> str:
        return "'" + value.replace("'", "''") + "'"

    def as_role(role: str, command: str) -> str:
        if role not in ('authenticated', 'service_role'):
            raise ValueError('Unexpected qualification role')
        return sql('BEGIN; SET LOCAL ROLE ' + role + '; ' + command + '; COMMIT;')

    def read(value: dict, expression: str) -> str:
        setting = 'SET LOCAL "request.jwt.claims" TO ' + literal(json.dumps(value)) + '; '
        return as_role('authenticated', setting + 'SELECT ' + expression)

    for identifier, kind in zip(ids, ('back', 'sleeve', 'dice'), strict=True):
        sql("INSERT INTO app.cosmetic(id,kind,asset_ref,state,is_free) VALUES("
            + literal(identifier) + ',' + literal(kind) + ",'test-only:auth-admission','active',true)")
    arguments = ','.join(literal(value) for value in (subject, session, 'auth_qualified', *ids))
    register = 'app.register_account(' + arguments + ')'
    setup = json.loads(read(claims, 'app.read_current_session()'))
    expected_setup = {'status': 'success', 'data': {'state': 'needs_registration', 'method': claims['spellwake_auth']['method'],
                       'subjectId': subject, 'sessionId': session, 'accountId': None}}
    if setup != expected_setup:
        raise RuntimeError('Verified session was not ready for registration')
    account = str(uuid.UUID(as_role('service_role', 'SELECT ' + register) if register_via_api is None
                            else register_via_api(ids, 'auth_qualified')))
    expected_setup['data'].update(state='active', accountId=account)
    if json.loads(read(claims, 'app.read_current_session()')) != expected_setup:
        raise RuntimeError('Verified session did not transition to registered account')
    if as_role('service_role', 'SELECT ' + register) != account:
        raise RuntimeError('Registration replay changed the account')
    if read(claims, 'app.current_account_id()') != account:
        raise RuntimeError('Signed authority did not admit its registered account')
    if read(claims, "coalesce((SELECT sum(quantity) FROM app.coin_balance),0)::text") != '14500':
        raise RuntimeError('Registered owner did not receive exactly one signup reward')
    counts = json.loads(sql("SELECT jsonb_build_object('accounts',(SELECT count(*) FROM app.account),"
                            "'signupOperations',(SELECT count(*) FROM app.ledger_operation WHERE source_scope='signup'),"
                            "'cardOrPackHoldings',(SELECT count(*) FROM app.inventory_asset i JOIN app.asset_definition d "
                            "ON d.id=i.asset_definition_id WHERE d.kind IN ('card','pack')))"))
    if counts != {'accounts': 1, 'signupOperations': 1, 'cardOrPackHoldings': 0}:
        raise RuntimeError('Registration created duplicate or starter assets')
    missing = dict(claims)
    missing.pop('spellwake_auth')
    if read(missing, 'app.current_account_id()') or read(missing, '(SELECT count(*) FROM app.coin_balance)') != '0':
        raise RuntimeError('Subject-only authority read registered inventory')

    def after_revocation() -> None:
        if read(claims, 'app.current_account_id()') or read(claims, '(SELECT count(*) FROM app.coin_balance)') != '0':
            raise RuntimeError('Unexpired signed token retained database access after native revocation')
        as_role('service_role', "DO $check$ BEGIN BEGIN PERFORM " + register
                + "; RAISE EXCEPTION 'unexpected registration after revocation'; "
                "EXCEPTION WHEN invalid_authorization_specification THEN NULL; END; END $check$")

    return {'signupCoins': '14500', 'counts': counts, 'replaySameAccount': True,
            'signedClaimsAdmitted': True, 'subjectOnlyDenied': True, 'sameSubjectSqlSetupTransition': register_via_api is None,
            'sameSubjectApiSetupTransition': register_via_api is not None}, after_revocation
