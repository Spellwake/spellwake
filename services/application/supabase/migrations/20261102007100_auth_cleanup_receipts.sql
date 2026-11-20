-- Canonical cleanup proof shares the actual Auth hard-delete transaction.
-- This does not expose a finalizer or accept caller assertions of completion.
BEGIN;

DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='auth_cleanup_v1') THEN
  RAISE EXCEPTION 'reserved Auth cleanup namespace requires explicit migration review';
 END IF;
END $$;

ALTER TABLE app.account ADD COLUMN auth_cleanup_subject uuid
 REFERENCES auth.users(id) ON DELETE SET NULL;
ALTER TABLE app.account ADD CONSTRAINT account_cleanup_subject_unique UNIQUE(auth_cleanup_subject);
ALTER TABLE app.account ADD CONSTRAINT account_cleanup_subject_lifecycle
 CHECK(auth_cleanup_subject IS NULL OR state='deleted');

CREATE OR REPLACE FUNCTION app.check_auth_receipt_shape(p_row app.command_receipt) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE subject_hash text; expected_hash text; result jsonb; binding app.command_receipt; retirement app.command_receipt;
BEGIN

 -- Cleanup is a distinct canonical source, bound to accepted retirement.
 IF p_row.scope='auth_cleanup_v1' THEN
  IF p_row.principal_key !~ '^auth:[0-9a-f]{64}$' OR p_row.request_ref<>'primary'
  OR p_row.account_id IS NOT NULL OR p_row.controller_grant_id IS NOT NULL OR p_row.operation_id IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth cleanup receipt identity';
  END IF;
  SELECT * INTO retirement FROM app.command_receipt
   WHERE principal_key=p_row.principal_key AND scope='auth_retirement_v1' AND request_ref='primary';
  IF NOT FOUND OR retirement.state<>'accepted' OR p_row.target_ref IS DISTINCT FROM retirement.target_ref THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth cleanup requires matching accepted retirement';
  END IF;
  PERFORM app.check_auth_receipt_shape(retirement);
  expected_hash:=app.auth_json_hash(jsonb_build_object('version',1,'scope','auth_cleanup_v1',
   'subjectHash',substring(p_row.principal_key FROM 6),'retirementId',retirement.id::text));
  IF p_row.payload_hash IS DISTINCT FROM expected_hash THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth cleanup payload hash differs';
  END IF;
  IF p_row.state='pending' THEN
   IF p_row.result_ref<>'' OR p_row.response_payload IS NOT NULL THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending Auth cleanup contains a result';
   END IF;
   RETURN;
  END IF;
  result:=p_row.result_ref::jsonb;
  IF p_row.state<>'accepted' OR jsonb_typeof(result->'proofHash') IS DISTINCT FROM 'string'
  OR (result->>'proofHash' ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
  OR result IS DISTINCT FROM jsonb_build_object('version',1,'retirementId',retirement.id::text,'proofHash',result->>'proofHash')
  OR p_row.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_row.id::text)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid Auth cleanup result or acknowledgment';
  END IF;
  RETURN;
 END IF;
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

CREATE OR REPLACE FUNCTION app.guard_auth_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF left(NEW.principal_key,5)<>'auth:' AND NEW.scope NOT IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1','auth_cleanup_v1') THEN
  RETURN NEW;
 END IF;
 PERFORM app.check_auth_receipt_shape(NEW);
 IF (TG_OP='INSERT' AND NEW.state<>'pending') OR (TG_OP='UPDATE' AND NEW.state<>'accepted') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Auth receipts require atomic pending to accepted transition';
 END IF;
 RETURN NEW;
END $$;

DROP TRIGGER auth_receipt_commit ON app.command_receipt;
CREATE CONSTRAINT TRIGGER auth_receipt_commit AFTER INSERT OR UPDATE ON app.command_receipt
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
 WHEN (left(NEW.principal_key,5)='auth:' OR NEW.scope IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1','auth_cleanup_v1'))
 EXECUTE FUNCTION app.check_auth_receipt_commit();


-- Preserve the common exact acknowledgment and the private completion capability.
CREATE OR REPLACE FUNCTION app.accept_auth_receipt(p_id uuid,p_result jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.command_receipt SET state='accepted',result_ref=p_result::text,
  response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',id::text))
 WHERE id=p_id AND state='pending' AND scope IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1','auth_cleanup_v1')
  AND principal_key~'^auth:[0-9a-f]{64}$';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth receipt disposition changed'; END IF;
END $$;

-- Only the canonical user-delete guard calls this. No caller-supplied proof.
CREATE FUNCTION app.record_auth_cleanup(p_subject uuid,p_retirement uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE proof text; retirement app.command_receipt; cleanup app.command_receipt; payload jsonb;
BEGIN
 proof:=app.read_auth_cleanup_proof(p_subject);
 retirement:=app.read_auth_retirement(p_subject);
 IF retirement.id IS NULL OR retirement.id IS DISTINCT FROM p_retirement THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth cleanup retirement differs';
 END IF;
 payload:=jsonb_build_object('version',1,'scope','auth_cleanup_v1','subjectHash',app.auth_uuid_hash(p_subject),
  'retirementId',retirement.id::text);
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES(retirement.principal_key,'auth_cleanup_v1','primary',retirement.target_ref,app.auth_json_hash(payload),'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO cleanup;
 IF NOT FOUND THEN
  SELECT * INTO STRICT cleanup FROM app.command_receipt WHERE principal_key=retirement.principal_key
   AND scope='auth_cleanup_v1' AND request_ref='primary';
  PERFORM app.check_auth_receipt_shape(cleanup);
  IF cleanup.state<>'accepted' OR cleanup.payload_hash<>app.auth_json_hash(payload) THEN
   RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth cleanup proof differs';
  END IF;
  -- A privileged UUID reuse cannot replace the first accepted cleanup source.
  -- This invocation still required its own current canonical delete audit above.
  RETURN cleanup.id;
 END IF;
 PERFORM app.accept_auth_receipt(cleanup.id,jsonb_build_object('version',1,'retirementId',retirement.id::text,'proofHash',proof));
 RETURN cleanup.id;
END $$;
REVOKE ALL ON FUNCTION app.record_auth_cleanup(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;


CREATE OR REPLACE FUNCTION app.guard_auth_user_cleanup() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE proof text; retirement app.command_receipt; payload jsonb; result jsonb; operation uuid;
BEGIN
 IF TG_OP='UPDATE' THEN
  IF NEW.deleted_at IS NOT NULL AND NEW.deleted_at IS DISTINCT FROM OLD.deleted_at THEN
   RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth cleanup requires hard deletion';
  END IF;
  IF NEW.id IS DISTINCT FROM OLD.id AND (app.read_bound_auth_method(OLD.id) IS NOT NULL
    OR (app.read_auth_retirement(OLD.id)).id IS NOT NULL) THEN
   RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='bound Auth subject cannot change';
  END IF;
  RETURN NEW;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth cleanup requires read committed retry';
 END IF;
 proof:=app.read_auth_cleanup_proof(OLD.id);
 -- Parent DELETE lock is already held. Never take application row locks after
 -- the authority fence; the registered finalizer committed before this request.
 PERFORM pg_advisory_xact_lock(hashtextextended('signup:'||OLD.id::text,0));
 PERFORM pg_advisory_xact_lock(hashtextextended('auth_authority_v1:'||app.auth_uuid_hash(OLD.id),0));
 IF EXISTS(SELECT 1 FROM app.account WHERE auth_subject=OLD.id) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='application account must finish retirement first';
 END IF;
 retirement:=app.read_auth_retirement(OLD.id);
 IF retirement.id IS NOT NULL THEN
  IF retirement.target_ref<>'unregistered' THEN
   SELECT retirement_operation_id INTO operation FROM app.account WHERE id=retirement.target_ref::uuid;
   PERFORM app.check_registered_auth_retirement(retirement.target_ref::uuid,operation);
   result:=jsonb_build_object('version',1,'proofHash',app.auth_json_hash(jsonb_build_object('version',1,'source','application_retirement',
    'accountId',retirement.target_ref,'retirementOperationId',operation::text)));
   IF retirement.result_ref::jsonb IS DISTINCT FROM result THEN
    RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='application retirement proof differs';
   END IF;
  END IF;
  PERFORM app.record_auth_cleanup(OLD.id,retirement.id);
  RETURN OLD;
 END IF;
 payload:=jsonb_build_object('version',1,'scope','auth_retirement_v1','subjectHash',app.auth_uuid_hash(OLD.id),'target','unregistered');
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES('auth:'||app.auth_uuid_hash(OLD.id),'auth_retirement_v1','primary','unregistered',app.auth_json_hash(payload),'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO retirement;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='cleanup retirement winner requires retry'; END IF;
 PERFORM app.accept_auth_receipt(retirement.id,jsonb_build_object('version',1,'proofHash',proof));
 PERFORM app.record_auth_cleanup(OLD.id,retirement.id);
  RETURN OLD;
END $$;

COMMIT;

