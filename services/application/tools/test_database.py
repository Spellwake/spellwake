"""Run migrations and access/race tests in a unique local Postgres database."""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[1]
CONTAINER = 'spellwake-application-db'

# These fixtures replace shared identities or install triggers on shared tables.
# The UUID-named acl_probe function fixture is deliberately ordinary: it neither
# replaces an existing function nor changes locks on an application relation.
SCHEMA_MUTATING_TESTS = (
    'native_opening_context::late_finalization_failure_rolls_back_context_artifacts_receipt_and_clocks',
    'retirement::retirement_rejects_stale_isolation_and_rolls_back_all_effects_on_late_failure',
    'patreon_intake::shared_evidence_scan_rediscovers_late_objects_and_preserves_stripe_nonwebhook_rules',
    'patreon_intake::stripe_shared_evidence_wrappers_preserve_work_nulls_timezone_and_late_object_recovery',
    'native_foundation::transition_sources::transition_upgrade_preserves_unmarked_originals_and_classifies_a_mixed_prefix',
    'native_foundation::transition_sources::upgrade_recovers_unmarked_human_controller_and_system_originals_after_host_fencing',
    'native_foundation::clock_opening::forward_migration_preserves_legacy_native_bo3_originals_and_exact_recovery',
    'native_foundation::presentation::presentation_keeps_original_match_across_authenticated_mode_format_and_cosmetic_edits',
)

# These cases assert database lock schedules with short observation deadlines.
LOCK_SCHEDULE_TESTS = (
    'native_foundation::races::concurrent_same_original_installs_one_native_frame_and_both_callers_recover_it',
    'native_foundation::races::concurrent_view_and_cursor_issuers_retain_the_first_semantic_winner',
    'native_foundation::races::controller_ante::committed_writer_precedes_rebind_and_funds_exactly_once',
    'native_foundation::races::controller_ante::known_pending_and_same_original_wait_never_repeat_the_economic_write',
    'native_foundation::races::controller_ante::lower_sorted_other_participant_wait_does_not_lock_controller_sponsor_first',
)
PRIVATE_DATABASE_TESTS = SCHEMA_MUTATING_TESTS + LOCK_SCHEDULE_TESTS


def execute(arguments: list[str], data: str | None = None) -> None:
    """Fail visibly without interpolating command text into a shell."""
    subprocess.run(arguments, input=data, text=True, check=True)


@contextmanager
def owned_database(docker: list[str]):
    """Drop only a successfully created UUID realm, including setup failures."""
    database = 'spellwake_application_test_' + uuid.uuid4().hex
    execute(docker + ['createdb', '-U', 'postgres', database])
    primary_error = None
    try:
        psql = docker + ['psql', '-U', 'postgres', '-d', database, '-v', 'ON_ERROR_STOP=1', '--quiet']
        execute(psql, (ROOT / 'tests/bootstrap.sql').read_text())
        for migration in sorted((ROOT / 'supabase/migrations').glob('*.sql')):
            execute(psql, migration.read_text())
        yield database, psql
    except BaseException as error:
        primary_error = error
        raise
    finally:
        try:
            execute(docker + ['dropdb', '--force', '-U', 'postgres', database])
        except BaseException as cleanup_error:
            cleanup_error.add_note('Owned database requiring cleanup: ' + database)
            if primary_error is not None:
                raise BaseExceptionGroup('Test and owned database cleanup failed', [primary_error, cleanup_error])
            raise


def export_rows(psql: list[str]) -> list[dict]:
    """Retain one realm's rows; numeric and int8 values stay strings."""
    query = """SELECT jsonb_agg(jsonb_build_object('table',c.relname,'query',
      format('SELECT coalesce(json_agg(json_build_object(%s)),''[]''::json) FROM app.%I',
        (SELECT string_agg(format('%L,%I%s',a.attname,a.attname,
          CASE WHEN t.typname IN ('numeric','int8') THEN '::text' ELSE '' END),',' ORDER BY a.attnum)
         FROM pg_attribute a JOIN pg_type t ON t.oid=a.atttypid
         WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped),c.relname)))
      FROM pg_class c WHERE c.relnamespace='app'::regnamespace AND c.relkind IN ('r','v');"""
    metadata = json.loads(subprocess.check_output(psql + ['-tA'], input=query, text=True))
    retained = []
    for item in metadata:
        rows = json.loads(subprocess.check_output(psql + ['-tA'], input=item['query'], text=True))
        retained.append({'table': item['table'], 'rows': rows})
    return retained


def main() -> None:
    """Keep ordinary tests parallel and give shared-schema fixtures private realms."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--filter', action='append', help='Literal test-name substring; repeat for ordered execution, including overlaps. Private-database fixtures always use private databases')
    parser.add_argument('--row-contracts-output', type=Path, help='Primary realm row array; separate .isolated.json companion retains private realm rows and selection provenance')
    options = parser.parse_args()
    info = json.loads(subprocess.check_output(['docker', 'inspect', CONTAINER], text=True))[0]
    if (info['Config']['Image'] != 'postgres:17'
            or info['HostConfig']['PortBindings'] != {'5432/tcp': [{'HostIp': '127.0.0.1', 'HostPort': '55439'}]}
            or not info['State']['Running']):
        raise RuntimeError('Expected running known PostgreSQL17 loopback container on port55439')
    environment = os.environ.copy()
    environment['CARGO_TARGET_DIR'] = str(ROOT / 'target')
    environment.setdefault('RUST_TEST_THREADS', '2')
    # Inventory cannot accidentally inherit a caller's existing database URL.
    environment.pop('SPELLWAKE_TEST_DATABASE_URL', None)
    arguments = ['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--test', 'database']
    listing = subprocess.check_output(arguments + ['--', '--list', '--ignored'], env=environment, text=True)
    inventory = [line.removesuffix(': test') for line in listing.splitlines() if line.endswith(': test')]
    if not inventory or len(inventory) != len(set(inventory)):
        raise RuntimeError('Empty or duplicate actual ignored database inventory')
    if not set(PRIVATE_DATABASE_TESTS).issubset(inventory):
        raise RuntimeError('Private-database test classification no longer matches actual inventory')
    # libtest --skip is a substring. Do not silently exclude another test if a
    # future name embeds one of these full names.
    if any(private in name and private != name for private in PRIVATE_DATABASE_TESTS for name in inventory):
        raise RuntimeError('Private-database skip would hide another actual test')
    selections = []
    for selected in options.filter or [None]:
        matches = [name for name in inventory if selected is None or selected in name]
        if not matches:
            parser.error('No ignored database tests match literal filter: ' + repr(selected))
        # Real Rust test identifiers cannot start with an option prefix.
        if selected is not None and selected.startswith('-'):
            parser.error('Filter must be a literal Rust test-name substring')
        selections.append((selected, matches))
    docker = ['docker', 'exec', '-i', CONTAINER]
    with owned_database(docker) as (primary, primary_psql):
        companion = {'version': 1, 'primaryDatabase': primary, 'isolatedRealms': []}
        output = options.row_contracts_output
        def save_companion() -> None:
            if output is not None:
                output.with_name(output.name + '.isolated.json').write_text(json.dumps(companion))
        save_companion()
        def run(database: str, selected: str | None, private: bool) -> None:
            case_environment = environment.copy()
            case_environment['SPELLWAKE_TEST_DATABASE_URL'] = f'host=127.0.0.1 port=55439 user=postgres dbname={database}'
            command = arguments + ([] if selected is None else [selected]) + ['--', '--ignored']
            if private:
                command.append('--exact')
            else:
                for name in PRIVATE_DATABASE_TESTS:
                    command.extend(['--skip', name])
            subprocess.run(command, env=case_environment, check=True)
        for occurrence, (selected, matches) in enumerate(selections):
            ordinary = [name for name in matches if name not in PRIVATE_DATABASE_TESTS]
            if ordinary:
                print(f'Database selection {occurrence}: {len(ordinary)} ordinary tests in shared realm', flush=True)
                run(primary, selected, False)
            for name in matches:
                if name not in PRIVATE_DATABASE_TESTS:
                    continue
                with owned_database(docker) as (private, private_psql):
                    print(f'Database selection {occurrence}: isolated fixture {name}', flush=True)
                    run(private, name, True)
                    if output is not None:
                        companion['isolatedRealms'].append({'selectionOccurrence': occurrence, 'filter': selected, 'test': name, 'database': private, 'tables': export_rows(private_psql)})
                        save_companion()
        if output is not None:
            output.write_text(json.dumps(export_rows(primary_psql)))


if __name__ == '__main__':
    main()
