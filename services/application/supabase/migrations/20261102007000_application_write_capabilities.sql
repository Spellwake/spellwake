-- Domain RPCs own writes. Undeveloped domains must not inherit prototype DML.
-- Preserve existing SELECT grants/RLS and explicit function capabilities.
BEGIN;

REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN
 ON ALL TABLES IN SCHEMA app
 FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- A table-level REVOKE does not remove independent column privileges.
DO $$
DECLARE relation record; columns text;
BEGIN
 FOR relation IN
  SELECT c.oid,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='app' AND c.relkind IN ('r','v') ORDER BY c.relname
 LOOP
  SELECT string_agg(format('%I',a.attname),',' ORDER BY a.attnum) INTO columns
   FROM pg_attribute a WHERE a.attrelid=relation.oid AND a.attnum>0 AND NOT a.attisdropped;
  IF columns IS NOT NULL THEN
   EXECUTE format('REVOKE INSERT (%s), UPDATE (%s), REFERENCES (%s) ON TABLE app.%I FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin',
    columns,columns,columns,relation.relname);
  END IF;
 END LOOP;
END $$;

ALTER DEFAULT PRIVILEGES IN SCHEMA app
 REVOKE ALL ON TABLES FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

COMMIT;
