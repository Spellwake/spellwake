BEGIN;

-- Inert authority contracts only. No Auth hook, managed trigger or login is enabled.
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM app.command_receipt WHERE left(principal_key,5)='auth:'
  OR scope IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1')) THEN
  RAISE EXCEPTION 'reserved Auth namespaces require explicit migration review';
 END IF;
END $$;

CREATE FUNCTION app.auth_uuid_hash(p_value uuid) RETURNS text
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT encode(sha256(convert_to(p_value::text,'UTF8')),'hex')
$$;
CREATE FUNCTION app.auth_json_hash(p_value jsonb) RETURNS text
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT encode(sha256(convert_to(p_value::text,'UTF8')),'hex')
$$;

CREATE FUNCTION app.check_auth_receipt_shape(p_row app.command_receipt) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE subject_hash text; expected_hash text; result jsonb; binding app.command_receipt;
BEGIN
 IF p_row.principal_key !~ '^auth:[0-9a-f]{64}$'
 OR p_row.scope NOT IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1')
 OR p_row.account_id IS NOT NULL OR p_row.controller_grant_id IS NOT NULL OR p_row.operation_id IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid reserved Auth receipt identity';
 END IF;
 subject_hash:=substring(p_row.principal_key FROM 6);
 IF p_row.scope='auth_binding_v1' THEN
  IF p_row.request_ref<>'primary' OR p_row.target_ref<>'primary' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth binding identity';
  END IF;
  expected_hash:=app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_binding_v1','subjectHash',subject_hash));
 ELSIF p_row.scope='auth_session_v1' THEN
  IF p_row.request_ref !~ '^[0-9a-f]{64}$' OR p_row.target_ref IS DISTINCT FROM (p_row.target_ref::uuid)::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth session identity';
  END IF;
  SELECT * INTO binding FROM app.command_receipt WHERE id=p_row.target_ref::uuid;
  IF NOT FOUND OR binding.principal_key<>p_row.principal_key OR binding.scope<>'auth_binding_v1'
  OR binding.state<>'accepted' OR binding.request_ref<>'primary' OR binding.target_ref<>'primary' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth session requires matching accepted binding';
  END IF;
  PERFORM app.check_auth_receipt_shape(binding);
  expected_hash:=app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_session_v1',
   'subjectHash',subject_hash,'sessionHash',p_row.request_ref,'bindingId',p_row.target_ref));
 ELSE
  IF p_row.request_ref<>'primary' OR (p_row.target_ref<>'unregistered'
   AND p_row.target_ref IS DISTINCT FROM (p_row.target_ref::uuid)::text) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth retirement identity';
  END IF;
  expected_hash:=app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_retirement_v1',
   'subjectHash',subject_hash,'target',p_row.target_ref));
 END IF;
 IF p_row.payload_hash<>expected_hash THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth receipt payload hash differs';
 END IF;
 IF p_row.state='pending' THEN
  IF p_row.result_ref<>'' OR p_row.response_payload IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending Auth receipt contains a result';
  END IF;
  RETURN;
 END IF;
 IF p_row.state<>'accepted' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth authority cannot retain rejected disposition';
 END IF;
 result:=p_row.result_ref::jsonb;
 IF result->'version' IS DISTINCT FROM '1'::jsonb OR result->>'proofHash' IS NULL
 OR result->>'proofHash' !~ '^[0-9a-f]{64}$'
 OR jsonb_typeof(result->'proofHash') IS DISTINCT FROM 'string'
 OR p_row.response_payload IS DISTINCT FROM jsonb_build_object('status','success',
  'data',jsonb_build_object('resourceId',p_row.id::text)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth receipt result or acknowledgment';
 END IF;
 IF p_row.scope IN ('auth_binding_v1','auth_session_v1') THEN
  IF result->>'method' IS NULL OR result->>'method' NOT IN ('google','apple','discord','email_code') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth method family';
  END IF;
 END IF;
 IF p_row.scope='auth_binding_v1' THEN
  IF result IS DISTINCT FROM jsonb_build_object('version',1,'method',result->>'method','proofHash',result->>'proofHash') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unexpected Auth binding result fields';
  END IF;
 ELSIF p_row.scope='auth_session_v1' THEN
  IF result->>'method' IS DISTINCT FROM (binding.result_ref::jsonb)->>'method'
  OR result->>'initialClass' IS DISTINCT FROM (CASE WHEN result->>'method'='email_code' THEN 'otp' ELSE 'oauth' END)
  OR result IS DISTINCT FROM jsonb_build_object('version',1,'method',result->>'method',
   'initialClass',result->>'initialClass','bindingId',p_row.target_ref,'proofHash',result->>'proofHash') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth session result does not match binding';
  END IF;
 ELSE
  IF result IS DISTINCT FROM jsonb_build_object('version',1,'proofHash',result->>'proofHash') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unexpected Auth retirement result fields';
  END IF;
 END IF;
EXCEPTION WHEN invalid_text_representation THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth receipt encoding';
END $$;

CREATE FUNCTION app.guard_auth_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF left(NEW.principal_key,5)<>'auth:' AND NEW.scope NOT IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1') THEN
  RETURN NEW;
 END IF;
 PERFORM app.check_auth_receipt_shape(NEW);
 IF (TG_OP='INSERT' AND NEW.state<>'pending') OR (TG_OP='UPDATE' AND NEW.state<>'accepted') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth receipts require atomic pending to accepted transition';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER auth_receipt_contract BEFORE INSERT OR UPDATE ON app.command_receipt
 FOR EACH ROW EXECUTE FUNCTION app.guard_auth_receipt();

CREATE FUNCTION app.check_auth_receipt_commit() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actual app.command_receipt;
BEGIN
 SELECT * INTO STRICT actual FROM app.command_receipt WHERE id=NEW.id;
 IF actual.state<>'accepted' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth receipt cannot commit pending authority';
 END IF;
 PERFORM app.check_auth_receipt_shape(actual);
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER auth_receipt_commit AFTER INSERT OR UPDATE ON app.command_receipt
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
 WHEN (left(NEW.principal_key,5)='auth:' OR NEW.scope IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1'))
 EXECUTE FUNCTION app.check_auth_receipt_commit();

REVOKE EXECUTE ON FUNCTION app.auth_uuid_hash(uuid),app.auth_json_hash(jsonb),
 app.check_auth_receipt_shape(app.command_receipt),app.guard_auth_receipt(),app.check_auth_receipt_commit()
 FROM PUBLIC,anon,authenticated,service_role;

COMMIT;
