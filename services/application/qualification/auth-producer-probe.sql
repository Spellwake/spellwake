-- Real-server provenance observation only. Not a binding policy or migration.
CREATE SCHEMA probe;
REVOKE ALL ON SCHEMA probe FROM PUBLIC;
CREATE TABLE probe.control(reject_token boolean NOT NULL,delay_milliseconds integer NOT NULL DEFAULT 0 CHECK(delay_milliseconds BETWEEN 0 AND 500));
INSERT INTO probe.control(reject_token) VALUES(false);
CREATE TABLE probe.observation(
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 value jsonb NOT NULL
);
REVOKE ALL ON ALL TABLES IN SCHEMA probe FROM PUBLIC;
-- Test-only delivery sink. Codes never enter retained result/log output.
CREATE TABLE probe.mailbox(token text NOT NULL, action text NOT NULL, token_hash text NOT NULL);
REVOKE ALL ON probe.mailbox FROM PUBLIC;
CREATE FUNCTION probe.capture_mail(event jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF session_user<>'__TRUSTED_ROLE__'
 OR (event->'email_data'->>'token' ~ '^[0-9]{6}$') IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'unexpected test delivery';
 END IF;
 DELETE FROM probe.mailbox;
 INSERT INTO probe.mailbox VALUES(event->'email_data'->>'token',
  event->'email_data'->>'email_action_type',event->'email_data'->>'token_hash');
 RETURN '{}'::jsonb;
END $$;
REVOKE ALL ON FUNCTION probe.capture_mail(jsonb) FROM PUBLIC;

CREATE FUNCTION probe.capture() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE normalized jsonb;
BEGIN
 IF session_user<>'__TRUSTED_ROLE__' THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unexpected source login';
 END IF;
 normalized:=jsonb_build_object('actor',NEW.payload->>'actor_id',
  'action',NEW.payload->>'action','provider',NEW.payload->'traits'->>'provider',
  'providerType',NEW.payload->'traits'->>'provider_type');
 PERFORM set_config('spellwake.probe_producer',jsonb_build_object(
  'xact',pg_current_xact_id()::text,'source',NEW.id,'normalized',normalized,
  'hash',encode(sha256(convert_to(normalized::text,'UTF8')),'hex'))::text,true);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION probe.capture() FROM PUBLIC;
CREATE TRIGGER producer_probe AFTER INSERT ON auth.audit_log_entries
 FOR EACH ROW EXECUTE FUNCTION probe.capture();

CREATE FUNCTION probe.token_hook(event jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; source auth.audit_log_entries; normalized jsonb;
 selected_session uuid; source_matches boolean; session_matches boolean; session_live boolean; response jsonb;
BEGIN
 IF session_user<>'__TRUSTED_ROLE__' THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unexpected hook login';
 END IF;
 IF __APPLICATION_AUTH__ THEN
  response:=app.custom_access_token_hook(event);
 ELSE
  response:=jsonb_build_object('claims',event->'claims');
 END IF;
 PERFORM pg_sleep((SELECT delay_milliseconds FROM probe.control)::double precision/1000.0);
 IF (SELECT reject_token FROM probe.control) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='intentional qualification rejection';
 END IF;
 context:=nullif(current_setting('spellwake.probe_producer',true),'')::jsonb;
 IF context IS NULL OR context->>'xact' IS DISTINCT FROM pg_current_xact_id()::text THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='missing current producer context';
 END IF;
 SELECT * INTO source FROM auth.audit_log_entries WHERE id=(context->>'source')::uuid;
 IF NOT FOUND THEN RAISE EXCEPTION 'producer row absent'; END IF;
 normalized:=jsonb_build_object('actor',source.payload->>'actor_id',
  'action',source.payload->>'action','provider',source.payload->'traits'->>'provider',
  'providerType',source.payload->'traits'->>'provider_type');
 source_matches:=normalized->>'actor'=event->>'user_id'
  AND context->>'hash'=encode(sha256(convert_to(normalized::text,'UTF8')),'hex');
 selected_session:=(event->'claims'->>'session_id')::uuid;
 SELECT EXISTS(SELECT 1 FROM auth.sessions s WHERE s.id=selected_session
  AND s.user_id=(event->>'user_id')::uuid) INTO session_matches;
 IF source_matches IS DISTINCT FROM true OR session_matches IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'producer or session mismatch';
 END IF;
 session_live:=app.auth_session_is_live((event->>'user_id')::uuid,selected_session,'otp')
  OR app.auth_session_is_live((event->>'user_id')::uuid,selected_session,'oauth');
 IF session_live IS DISTINCT FROM true THEN RAISE EXCEPTION 'application session predicate denied'; END IF;
 INSERT INTO probe.observation(value) VALUES(jsonb_build_object(
  'action',normalized->>'action','provider',normalized->>'provider',
  'providerType',normalized->>'providerType','method',event->>'authentication_method',
  'sourceMatches',source_matches,'sessionMatches',session_matches,'sessionLive',session_live,
  'authority',response->'claims'->'spellwake_auth',
  'xact',context->>'xact','backendPid',pg_backend_pid(),'login',session_user,
  'sessionHash',encode(sha256(convert_to(selected_session::text,'UTF8')),'hex'),
  'amr',(SELECT jsonb_agg(authentication_method ORDER BY authentication_method)
   FROM auth.mfa_amr_claims WHERE session_id=selected_session)));
 RETURN response;
END $$;
REVOKE ALL ON FUNCTION probe.token_hook(jsonb) FROM PUBLIC;
GRANT USAGE ON SCHEMA probe TO __TRUSTED_ROLE__;
GRANT EXECUTE ON FUNCTION probe.token_hook(jsonb) TO __TRUSTED_ROLE__;
GRANT EXECUTE ON FUNCTION probe.capture_mail(jsonb) TO __TRUSTED_ROLE__;
