-- Standalone GoTrue supplies the actual auth.uid() from its initial migration.
-- Test-only API role grants; this is not managed PostgREST qualification.
GRANT USAGE ON SCHEMA auth TO authenticated,service_role;
GRANT EXECUTE ON FUNCTION auth.uid() TO authenticated,service_role;
