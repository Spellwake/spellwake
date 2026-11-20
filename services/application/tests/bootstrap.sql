-- Local Postgres test harness only. Production uses Supabase-managed Auth and roles.
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='anon') THEN CREATE ROLE anon NOLOGIN; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='authenticated') THEN CREATE ROLE authenticated NOLOGIN; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='service_role') THEN CREATE ROLE service_role NOLOGIN BYPASSRLS; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='supabase_auth_admin') THEN CREATE ROLE supabase_auth_admin NOLOGIN; END IF;
END $$;
CREATE SCHEMA auth;
CREATE TABLE auth.users (
 id uuid PRIMARY KEY,
 deleted_at timestamptz,
 banned_until timestamptz,
 email text,
 email_confirmed_at timestamptz,
 phone text
);
CREATE TABLE auth.identities (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 user_id uuid NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
 provider text NOT NULL,
 provider_id text NOT NULL,
 identity_data jsonb NOT NULL
);
CREATE TABLE auth.audit_log_entries (id uuid PRIMARY KEY DEFAULT gen_random_uuid(),payload json NOT NULL);
GRANT USAGE ON SCHEMA auth TO supabase_auth_admin;
GRANT INSERT ON auth.audit_log_entries TO supabase_auth_admin;
-- Test-only managed role privileges for canonical cleanup/identity producers.
GRANT SELECT,DELETE ON auth.users TO supabase_auth_admin;
GRANT SELECT,INSERT,UPDATE,DELETE ON auth.identities TO supabase_auth_admin;
-- Minimal managed-shape fixtures only; GoTrue supplies real tables in its realm.
CREATE TABLE auth.sessions (
 id uuid PRIMARY KEY,
 user_id uuid NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
 not_after timestamptz,
 refresh_token_hmac_key text,
 refresh_token_counter bigint
);
CREATE TABLE auth.mfa_amr_claims (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 session_id uuid NOT NULL REFERENCES auth.sessions(id) ON DELETE CASCADE,
 authentication_method text NOT NULL
);
CREATE TABLE auth.refresh_tokens (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 user_id text,
 session_id uuid REFERENCES auth.sessions(id) ON DELETE CASCADE,
 revoked boolean
);
CREATE FUNCTION auth.uid() RETURNS uuid LANGUAGE sql STABLE AS $$
  SELECT nullif(current_setting('request.jwt.claim.sub', true), '')::uuid
$$;
GRANT USAGE ON SCHEMA auth TO authenticated, service_role;
GRANT EXECUTE ON FUNCTION auth.uid() TO authenticated, service_role;

-- Privileged fixture-only authority. Never installed by application migrations.
CREATE SCHEMA test_fixture;
REVOKE ALL ON SCHEMA test_fixture FROM PUBLIC,anon,authenticated,service_role;
CREATE FUNCTION test_fixture.claims_for_subject(p_subject uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 RETURN (SELECT jsonb_build_object('sub',p_subject::text,'session_id',s.id::text,
  'role','authenticated','aud','authenticated','exp',extract(epoch FROM statement_timestamp()+interval '1 day')::bigint,
  'spellwake_auth',jsonb_build_object('version',1,'method','email_code','bindingId',b.id::text,'sessionGrantId',g.id::text))
 FROM auth.sessions s JOIN app.command_receipt b ON b.principal_key='auth:'||app.auth_uuid_hash(p_subject)
  AND b.scope='auth_binding_v1' AND b.state='accepted'
 JOIN app.command_receipt g ON g.principal_key=b.principal_key AND g.scope='auth_session_v1'
  AND g.request_ref=app.auth_uuid_hash(s.id) AND g.state='accepted'
 WHERE s.user_id=p_subject ORDER BY s.id LIMIT 1);
END;
$$;
CREATE FUNCTION test_fixture.seed_auth_authority(p_subject uuid,p_session uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding_id uuid; grant_id uuid; subject_hash text; principal text;
BEGIN
 subject_hash:=app.auth_uuid_hash(p_subject); principal:='auth:'||subject_hash;
 INSERT INTO auth.sessions(id,user_id,refresh_token_hmac_key,refresh_token_counter)
 VALUES(p_session,p_subject,'fixture-only-key',0);
 INSERT INTO auth.mfa_amr_claims(session_id,authentication_method) VALUES(p_session,'otp');
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES(principal,'auth_binding_v1','primary','primary',app.auth_json_hash(jsonb_build_object('version',1,
  'scope','auth_binding_v1','subjectHash',subject_hash)),'pending','') RETURNING id INTO binding_id;
 PERFORM app.accept_auth_receipt(binding_id,jsonb_build_object('version',1,'method','email_code','proofHash',repeat('a',64)));
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES(principal,'auth_session_v1',app.auth_uuid_hash(p_session),binding_id::text,
  app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_session_v1','subjectHash',subject_hash,
   'sessionHash',app.auth_uuid_hash(p_session),'bindingId',binding_id::text)),'pending','') RETURNING id INTO grant_id;
 PERFORM app.accept_auth_receipt(grant_id,jsonb_build_object('version',1,'method','email_code','initialClass','otp',
  'bindingId',binding_id::text,'proofHash',repeat('a',64)));
END $$;
REVOKE EXECUTE ON ALL FUNCTIONS IN SCHEMA test_fixture FROM PUBLIC,anon,authenticated,service_role;
