BEGIN;

-- The creation instant fixes elapsed-age policy; no later command may manufacture maturity.
CREATE FUNCTION app.guard_account_creation_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.created_at) IS DISTINCT FROM (OLD.id,OLD.created_at) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account identity and creation instant are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER account_creation_identity BEFORE UPDATE ON app.account
 FOR EACH ROW EXECUTE FUNCTION app.guard_account_creation_identity();

-- App instants cross finite ISO/RFC3339 boundaries. Managed Auth schemas are outside this loop.
DO $$
DECLARE column_record record;
BEGIN
 FOR column_record IN
  SELECT c.relname,a.attname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  JOIN pg_attribute a ON a.attrelid=c.oid
  WHERE n.nspname='app' AND c.relkind='r' AND a.attnum>0 AND NOT a.attisdropped
   AND a.atttypid='timestamptz'::regtype ORDER BY c.relname,a.attnum
 LOOP
  EXECUTE format('ALTER TABLE app.%I ADD CONSTRAINT %I CHECK (%I IS NULL OR pg_catalog.isfinite(%I))',
   column_record.relname,'finite_'||column_record.attname,column_record.attname,column_record.attname);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION app.guard_account_creation_identity()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
