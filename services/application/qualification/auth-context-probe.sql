-- Disposable qualification probe, not a migration or production Auth hook.
-- Answers only role/transaction-context questions using synthetic audit rows.
CREATE SCHEMA probe;
REVOKE ALL ON SCHEMA probe FROM PUBLIC;
CREATE TABLE probe.audit_source(id uuid PRIMARY KEY,payload jsonb NOT NULL);
REVOKE ALL ON probe.audit_source FROM PUBLIC;

CREATE FUNCTION probe.capture() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE normalized jsonb;
BEGIN
 IF session_user<>'__TRUSTED_ROLE__' THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified source login'; END IF;
 normalized:=jsonb_build_object('actor',NEW.payload->>'actor_id','provider',NEW.payload->>'provider');
 PERFORM set_config('spellwake.probe_auth_context',jsonb_build_object('version','probe-v1','xact',pg_current_xact_id()::text,
 'source',NEW.id,'actor',normalized->>'actor','hash',encode(sha256(convert_to(normalized::text,'UTF8')),'hex'))::text,true);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION probe.capture() FROM PUBLIC;
CREATE TRIGGER captured AFTER INSERT ON probe.audit_source FOR EACH ROW EXECUTE FUNCTION probe.capture();

CREATE FUNCTION probe.check_context(p_actor uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; source probe.audit_source; normalized jsonb;
BEGIN
 IF session_user<>'__TRUSTED_ROLE__' THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified hook login'; END IF;
 context:=nullif(current_setting('spellwake.probe_auth_context',true),'')::jsonb;
 IF context IS NULL OR context->>'version' IS DISTINCT FROM 'probe-v1' OR context->>'xact' IS DISTINCT FROM pg_current_xact_id()::text
 OR context->>'actor' IS DISTINCT FROM p_actor::text THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='missing or mismatched current source';
 END IF;
 SELECT * INTO source FROM probe.audit_source WHERE id=(context->>'source')::uuid;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='source row absent'; END IF;
 normalized:=jsonb_build_object('actor',source.payload->>'actor_id','provider',source.payload->>'provider');
 IF normalized->>'actor' IS DISTINCT FROM p_actor::text
 OR context->>'hash' IS DISTINCT FROM encode(sha256(convert_to(normalized::text,'UTF8')),'hex') THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='source payload differs';
 END IF;
 RETURN jsonb_build_object('source',source.id,'provider',normalized->>'provider','xact',context->>'xact','login',session_user);
END $$;
REVOKE ALL ON FUNCTION probe.check_context(uuid) FROM PUBLIC;
GRANT USAGE ON SCHEMA probe TO __TRUSTED_ROLE__,__GATEWAY_ROLE__,authenticated;
GRANT SELECT,INSERT ON probe.audit_source TO __TRUSTED_ROLE__;
GRANT EXECUTE ON FUNCTION probe.check_context(uuid) TO __TRUSTED_ROLE__;
