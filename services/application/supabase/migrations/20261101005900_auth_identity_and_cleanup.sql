BEGIN;

-- Immutable authority readers do not add locks to managed identity mutations.
CREATE FUNCTION app.read_bound_auth_method(p_subject uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding app.command_receipt;
BEGIN
 SELECT * INTO binding FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash(p_subject)
  AND scope='auth_binding_v1' AND request_ref='primary';
 IF NOT FOUND THEN RETURN NULL; END IF;
 PERFORM app.check_auth_receipt_shape(binding);
 IF binding.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth binding requires retry'; END IF;
 RETURN (binding.result_ref::jsonb)->>'method';
END $$;

CREATE FUNCTION app.read_auth_retirement(p_subject uuid) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE retirement app.command_receipt;
BEGIN
 SELECT * INTO retirement FROM app.command_receipt WHERE principal_key='auth:'||app.auth_uuid_hash(p_subject)
  AND scope='auth_retirement_v1' AND request_ref='primary';
 IF FOUND THEN
  PERFORM app.check_auth_receipt_shape(retirement);
  IF retirement.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth retirement requires retry'; END IF;
 END IF;
 RETURN retirement;
END $$;

-- Cleanup has an administrative actor and a distinct deleted target. Issuance's
-- actor=subject reader must not be reused by pretending these are the same UUID.
CREATE FUNCTION app.read_auth_cleanup_proof(p_target uuid) RETURNS text
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; source auth.audit_log_entries; fingerprint jsonb; form jsonb;
BEGIN
 IF session_user<>'supabase_auth_admin' OR p_target IS NULL OR p_target='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified Auth cleanup';
 END IF;
 context:=nullif(current_setting('spellwake.auth_provenance',true),'')::jsonb;
 IF context IS NULL OR context->'version' IS DISTINCT FROM '1'::jsonb
 OR context->>'kind' IS DISTINCT FROM 'cleanup'
 OR context->>'xact' IS DISTINCT FROM pg_current_xact_id()::text
 OR context->>'targetHash' IS DISTINCT FROM app.auth_uuid_hash(p_target) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='missing current Auth cleanup provenance';
 END IF;
 SELECT * INTO source FROM auth.audit_log_entries WHERE id=(context->>'sourceId')::uuid;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth cleanup source absent'; END IF;
 fingerprint:=app.auth_audit_fingerprint(source.id,source.payload::jsonb);
 form:=app.auth_audit_form(source.payload::jsonb);
 IF form->>'kind' IS DISTINCT FROM 'cleanup'
 OR fingerprint->>'targetHash' IS DISTINCT FROM app.auth_uuid_hash(p_target)
 OR context->>'actorHash' IS DISTINCT FROM fingerprint->>'actorHash'
 OR fingerprint->>'actorHash' IS NULL
 OR context->>'targetHash' IS DISTINCT FROM fingerprint->>'targetHash'
 OR context->>'candidate' IS DISTINCT FROM form->>'candidate'
 OR context->>'sourceHash' IS DISTINCT FROM app.auth_json_hash(fingerprint) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth cleanup source differs';
 END IF;
 RETURN app.auth_json_hash(fingerprint);
EXCEPTION WHEN invalid_text_representation THEN
 RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='invalid Auth cleanup encoding';
END $$;

CREATE FUNCTION app.check_registered_auth_retirement(p_account uuid,p_operation uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.account a JOIN app.ledger_operation o ON o.id=a.retirement_operation_id
  WHERE a.id=p_account AND a.state='deleted' AND a.retirement_operation_id=p_operation
   AND o.state='committed' AND o.kind='retirement' AND o.source_scope='account_retirement'
   AND o.source_ref=a.id::text AND o.actor_account_id=a.id) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='source-bound application retirement required';
 END IF;
END $$;

-- Only a future private finalizer may call this. It writes no asset, account or
-- deletion state and does not substitute for that finalizer's obligation checks.
CREATE FUNCTION app.record_registered_auth_retirement(p_subject uuid,p_account uuid,p_operation uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE account_row app.account; retirement app.command_receipt; principal text; payload jsonb; result jsonb;
BEGIN
 IF p_subject IS NULL OR p_subject='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retirement subject required';
 END IF;
 SELECT * INTO account_row FROM app.account WHERE id=p_account FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retirement account absent'; END IF;
 PERFORM app.check_registered_auth_retirement(p_account,p_operation);
 IF account_row.auth_subject IS NOT NULL AND account_row.auth_subject<>p_subject THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retirement subject differs';
 END IF;
 principal:='auth:'||app.auth_uuid_hash(p_subject);
 PERFORM pg_advisory_xact_lock(hashtextextended('auth_authority_v1:'||app.auth_uuid_hash(p_subject),0));
 payload:=jsonb_build_object('version',1,'scope','auth_retirement_v1','subjectHash',app.auth_uuid_hash(p_subject),'target',p_account::text);
 result:=jsonb_build_object('version',1,'proofHash',app.auth_json_hash(jsonb_build_object('version',1,'source','application_retirement',
  'accountId',p_account::text,'retirementOperationId',p_operation::text)));
 retirement:=app.read_auth_retirement(p_subject);
 IF retirement.id IS NOT NULL THEN
  IF retirement.target_ref<>p_account::text OR retirement.payload_hash<>app.auth_json_hash(payload)
   OR retirement.result_ref::jsonb IS DISTINCT FROM result THEN
   RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retirement proof differs';
  END IF;
  RETURN retirement.id;
 END IF;
 IF account_row.auth_subject IS DISTINCT FROM p_subject THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='first retirement requires current subject';
 END IF;
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES(principal,'auth_retirement_v1','primary',p_account::text,app.auth_json_hash(payload),'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO retirement;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='retirement winner requires retry'; END IF;
 PERFORM app.accept_auth_receipt(retirement.id,result);
 RETURN retirement.id;
END $$;

CREATE FUNCTION app.guard_auth_identity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE old_method text; new_method text; retirement app.command_receipt;
BEGIN
 IF TG_OP IN ('UPDATE','DELETE') THEN
  old_method:=app.read_bound_auth_method(OLD.user_id);
  retirement:=app.read_auth_retirement(OLD.user_id);
  IF TG_OP='DELETE' THEN
   IF old_method IS NOT NULL OR retirement.id IS NOT NULL THEN
    PERFORM app.read_auth_cleanup_proof(OLD.user_id);
    IF retirement.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='identity cleanup requires retirement'; END IF;
   END IF;
   RETURN OLD;
  END IF;
  IF retirement.id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retired identity cannot change'; END IF;
 END IF;
 new_method:=app.read_bound_auth_method(NEW.user_id);
 retirement:=app.read_auth_retirement(NEW.user_id);
 IF retirement.id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retired identity cannot change'; END IF;
 IF TG_OP='UPDATE' AND (old_method IS NOT NULL OR new_method IS NOT NULL) AND (
  NEW.user_id IS DISTINCT FROM OLD.user_id OR NEW.provider IS DISTINCT FROM OLD.provider
  OR NEW.provider_id IS DISTINCT FROM OLD.provider_id OR NEW.id IS DISTINCT FROM OLD.id) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='bound identity key cannot change';
 END IF;
 IF new_method IS NOT NULL AND NEW.provider IS DISTINCT FROM (CASE WHEN new_method='email_code' THEN 'email' ELSE new_method END) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='identity uses another bound family';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER application_auth_identity BEFORE INSERT OR UPDATE OR DELETE ON auth.identities
 FOR EACH ROW EXECUTE FUNCTION app.guard_auth_identity();

CREATE FUNCTION app.guard_auth_user_cleanup() RETURNS trigger
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
  RETURN OLD;
 END IF;
 payload:=jsonb_build_object('version',1,'scope','auth_retirement_v1','subjectHash',app.auth_uuid_hash(OLD.id),'target','unregistered');
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES('auth:'||app.auth_uuid_hash(OLD.id),'auth_retirement_v1','primary','unregistered',app.auth_json_hash(payload),'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO retirement;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='cleanup retirement winner requires retry'; END IF;
 PERFORM app.accept_auth_receipt(retirement.id,jsonb_build_object('version',1,'proofHash',proof));
 RETURN OLD;
END $$;
CREATE TRIGGER application_auth_user_cleanup BEFORE UPDATE OR DELETE ON auth.users
 FOR EACH ROW EXECUTE FUNCTION app.guard_auth_user_cleanup();

REVOKE EXECUTE ON FUNCTION app.read_bound_auth_method(uuid),app.read_auth_retirement(uuid),
 app.read_auth_cleanup_proof(uuid),app.check_registered_auth_retirement(uuid,uuid),
 app.record_registered_auth_retirement(uuid,uuid,uuid),app.guard_auth_identity(),app.guard_auth_user_cleanup()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
