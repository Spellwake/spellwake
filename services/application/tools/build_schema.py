"""Generate the reviewed initial DDL. Never rerun against an applied migration silently."""

import json
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]


def identifier(value: str) -> str:
    """Quote a schema-controlled identifier, never a request value."""
    return '"' + value.replace('"', '""') + '"'


def qualified(value: str) -> str:
    """Managed Auth stays external; application relations use the private app schema."""
    return '.'.join(identifier(piece) for piece in (value.split('.') if '.' in value else ['app', value]))


def main() -> None:
    """Emit all base records first, then cyclic references and closed-by-default RLS."""
    catalogue: dict[str, Any] = json.loads((ROOT / 'schema/tables.json').read_text())
    tables = [record for record in catalogue['tables'] if record['kind'] == 'table']
    if len(tables) != 90:
        raise ValueError('Initial approved schema must contain exactly 90 application tables')
    statements = [
        '-- Generated from schema/tables.json by tools/build_schema.py; review changes before application.',
        '-- This migration requires managed auth.users and the standard Supabase roles.',
        'BEGIN;',
        'CREATE SCHEMA app;',
        'CREATE SCHEMA IF NOT EXISTS api;',
        'REVOKE ALL ON SCHEMA app FROM PUBLIC, anon, authenticated;',
        'GRANT USAGE ON SCHEMA app TO service_role;',
    ]
    types = {'id': 'uuid', 'quantity': 'numeric(78,0)', 'time': 'timestamptz', 'bool': 'boolean', 'number': 'double precision', 'integer': 'bigint', 'text': 'text'}
    for record in tables:
        columns: list[str] = []
        for column in record['columns']:
            name = identifier(column['name'])
            definition = f"{name} {types[column['type']]}"
            if column['name'] == 'id':
                definition += ' PRIMARY KEY DEFAULT gen_random_uuid()'
            elif not column['nullable']:
                definition += ' NOT NULL'
            if column['type'] == 'quantity':
                definition += f" CHECK ({name} >= 0 AND {name} <> 'NaN'::numeric)"
            if column['type'] == 'number':
                definition += f" CHECK ({name} > '-Infinity'::float8 AND {name} < 'Infinity'::float8)"
            columns.append(definition)
        for unique in record['unique']:
            columns.append('UNIQUE (' + ', '.join(identifier(column) for column in unique) + ')')
        statements.append('CREATE TABLE ' + qualified(record['name']) + ' (\n  ' + ',\n  '.join(columns) + '\n);')
    for record in tables:
        name = record['name']
        for column in record['columns']:
            reference = column.get('reference')
            if not reference:
                continue
            suffix = ' ON DELETE SET NULL' if reference == 'auth.users' else ' ON DELETE RESTRICT'
            statements.append(f"ALTER TABLE {qualified(name)} ADD FOREIGN KEY ({identifier(column['name'])}) REFERENCES {qualified(reference)} (id){suffix} DEFERRABLE INITIALLY IMMEDIATE;")
            statements.append(f"CREATE INDEX ON {qualified(name)} ({identifier(column['name'])});")
        statements.extend([
            f'ALTER TABLE {qualified(name)} ENABLE ROW LEVEL SECURITY;',
            f'ALTER TABLE {qualified(name)} FORCE ROW LEVEL SECURITY;',
            f'REVOKE ALL ON {qualified(name)} FROM PUBLIC, anon, authenticated;',
            f'GRANT SELECT, INSERT, UPDATE, DELETE ON {qualified(name)} TO service_role;',
        ])
    statements.extend([
        'ALTER DEFAULT PRIVILEGES IN SCHEMA app REVOKE ALL ON TABLES FROM PUBLIC, anon, authenticated;',
        'ALTER DEFAULT PRIVILEGES IN SCHEMA app REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC, anon, authenticated;',
        'COMMIT;',
    ])
    destination = ROOT / 'supabase/migrations/20261101000100_application_tables.sql'
    destination.write_text('\n\n'.join(statements) + '\n')
    print(json.dumps({'tables': len(tables), 'migration': str(destination)}))


if __name__ == '__main__':
    main()
