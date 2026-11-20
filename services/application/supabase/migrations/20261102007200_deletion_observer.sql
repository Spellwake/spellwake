BEGIN;

CREATE FUNCTION app.request_account_deletion_command(p_subject uuid,p_session uuid,p_request uuid,p_status_key_hash text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; terms jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR (p_status_key_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 terms:=jsonb_build_object('statusKeyHash',p_status_key_hash);
 receipt:=app.begin_account_command(player.id,p_request,'account_delete',player.id::text,terms);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state NOT IN ('active','deletion_pending') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable'));
 END IF;
 IF player.state='active' THEN
  UPDATE app.account SET state='deletion_pending',deletion_requested_at=clock_timestamp(),social_revision=social_revision+1 WHERE id=player.id;
 END IF;
 UPDATE app.command_receipt SET result_ref=jsonb_build_object('version',1,'statusKeyHash',p_status_key_hash)::text WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',player.id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

-- Receives only the digest computed by the bounded private HTTP observer.
CREATE FUNCTION app.observe_account_deletion(p_account uuid,p_request uuid,p_status_key_hash text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; player app.account; retirement app.command_receipt;
 cleanup app.command_receipt; finalization app.command_receipt; unavailable jsonb; result jsonb; terms jsonb;
BEGIN
 unavailable:=jsonb_build_object('status','fail','data',jsonb_build_object('observer','unavailable'));
 IF p_account IS NULL OR p_request IS NULL
 OR p_account='00000000-0000-0000-0000-000000000000'::uuid OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR (p_status_key_hash ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true THEN RETURN unavailable; END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||p_account::text
 AND scope='account_delete' AND request_ref=p_request::text;
 IF NOT FOUND OR receipt.account_id IS DISTINCT FROM p_account OR receipt.target_ref IS DISTINCT FROM p_account::text
 OR receipt.state<>'accepted'
 OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('statusKeyHash',p_status_key_hash))
 OR receipt.result_ref IS DISTINCT FROM jsonb_build_object('version',1,'statusKeyHash',p_status_key_hash)::text
 OR receipt.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account)) THEN
  RETURN unavailable;
 END IF;
 SELECT * INTO player FROM app.account WHERE id=p_account;
 IF NOT FOUND THEN RETURN unavailable; END IF;
 result:=jsonb_build_object('accountId',p_account,'requestId',p_request);
 IF player.state='deletion_pending' THEN
  RETURN jsonb_build_object('status','success','data',result||jsonb_build_object('phase','pending')); END IF;
 IF player.state<>'deleted' OR player.retirement_operation_id IS NULL THEN RETURN unavailable; END IF;
 PERFORM app.check_registered_auth_retirement(player.id,player.retirement_operation_id);
 result:=result||jsonb_build_object('phase','finalizing','retirementOperationId',player.retirement_operation_id);
 -- A ledger label, Auth absence or cleanup proof alone cannot certify minimization.
 -- The canonical finalizer must produce this immutable, operation-bound receipt.
 SELECT * INTO finalization FROM app.command_receipt WHERE principal_key='human:'||p_account::text
 AND scope='account_retirement_v1' AND request_ref=p_account::text;
 terms:=jsonb_build_object('version',1,'scope','account_retirement_v1','accountId',p_account);
 IF finalization.id IS NULL OR finalization.state<>'accepted' OR finalization.account_id IS DISTINCT FROM p_account
 OR finalization.target_ref IS DISTINCT FROM p_account::text OR finalization.operation_id IS DISTINCT FROM player.retirement_operation_id
 OR finalization.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
 OR finalization.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account))
 OR player.auth_subject IS NOT NULL OR player.auth_cleanup_subject IS NOT NULL THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 SELECT * INTO retirement FROM app.command_receipt WHERE id=(finalization.result_ref::jsonb->>'authRetirementId')::uuid;
 IF retirement.id IS NULL OR retirement.scope<>'auth_retirement_v1' OR retirement.state<>'accepted'
 OR retirement.target_ref IS DISTINCT FROM p_account::text THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 PERFORM app.check_auth_receipt_shape(retirement);
 IF retirement.result_ref::jsonb IS DISTINCT FROM jsonb_build_object('version',1,
  'proofHash',app.auth_json_hash(jsonb_build_object('version',1,'source','application_retirement',
   'accountId',p_account::text,'retirementOperationId',player.retirement_operation_id::text))) THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 IF finalization.result_ref::jsonb IS DISTINCT FROM jsonb_build_object('version',1,'policy','account_minimization_v1',
  'retirementOperationId',player.retirement_operation_id,'authRetirementId',retirement.id) THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 SELECT * INTO cleanup FROM app.command_receipt WHERE principal_key=retirement.principal_key AND scope='auth_cleanup_v1' AND request_ref='primary';
 IF cleanup.id IS NULL OR cleanup.state<>'accepted' OR cleanup.target_ref IS DISTINCT FROM p_account::text THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 PERFORM app.check_auth_receipt_shape(cleanup);
 RETURN jsonb_build_object('status','success','data',result||jsonb_build_object('phase','deleted','authCleanupReceiptId',cleanup.id));
END $$;

REVOKE ALL ON FUNCTION app.request_account_deletion_command(uuid,uuid,uuid,text),app.observe_account_deletion(uuid,uuid,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.request_account_deletion_command(uuid,uuid,uuid,text),app.observe_account_deletion(uuid,uuid,text) TO service_role;

-- No client consumed the actor-only prototype; upgrade fixture/operator callers.
DROP FUNCTION app.request_account_deletion(uuid,uuid);
COMMIT;
