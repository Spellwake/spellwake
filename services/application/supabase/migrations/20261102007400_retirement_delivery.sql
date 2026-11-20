-- Dedicated deletion-worker capabilities. No public/client Auth target reader.
BEGIN;

CREATE FUNCTION app.account_auth_cleanup_receipt(p_account uuid) RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; retirement app.command_receipt; cleanup app.command_receipt;
BEGIN
 SELECT * INTO player FROM app.account WHERE id=p_account;
 IF NOT FOUND OR player.state<>'deleted' OR player.retirement_operation_id IS NULL
 OR player.auth_subject IS NOT NULL OR player.auth_cleanup_subject IS NOT NULL THEN RETURN NULL; END IF;
 retirement:=app.read_account_retirement_proof(player.id,player.retirement_operation_id);
 IF retirement.id IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO cleanup FROM app.command_receipt WHERE principal_key=retirement.principal_key
 AND scope='auth_cleanup_v1' AND request_ref='primary';
 IF cleanup.id IS NULL OR cleanup.state<>'accepted' OR cleanup.target_ref IS DISTINCT FROM player.id::text THEN RETURN NULL; END IF;
 PERFORM app.check_auth_receipt_shape(cleanup);
 RETURN cleanup.id;
EXCEPTION WHEN check_violation OR invalid_text_representation OR invalid_authorization_specification THEN
 -- Invalid proof is incomplete, never terminal. Keep the account discoverable;
 -- the strict work reader will report the source failure to the worker.
 RETURN NULL;
END $$;

CREATE FUNCTION app.read_account_retirement_work(p_account uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; retirement app.command_receipt; cleanup uuid; result jsonb;
BEGIN
 SELECT * INTO player FROM app.account WHERE id=p_account;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='retirement account unavailable'; END IF;
 result:=jsonb_build_object('accountId',player.id);
 IF player.state='deletion_pending' THEN RETURN result||jsonb_build_object('state','pending'); END IF;
 IF player.state<>'deleted' OR player.retirement_operation_id IS NULL OR player.auth_subject IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='retirement account unavailable'; END IF;
 retirement:=app.read_account_retirement_proof(player.id,player.retirement_operation_id);
 IF retirement.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canonical retirement proof unavailable'; END IF;
 result:=result||jsonb_build_object('retirementOperationId',player.retirement_operation_id);
 cleanup:=app.account_auth_cleanup_receipt(player.id);
 IF cleanup IS NOT NULL THEN
  RETURN result||jsonb_build_object('state','complete','authCleanupReceiptId',cleanup); END IF;
 IF player.auth_cleanup_subject IS NULL THEN
  -- Absence without the canonical cleanup source cannot authorize completion.
  RETURN result||jsonb_build_object('state','unproven'); END IF;
 IF retirement.principal_key IS DISTINCT FROM 'auth:'||app.auth_uuid_hash(player.auth_cleanup_subject) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement cleanup target differs'; END IF;
 RETURN result||jsonb_build_object('state','cleanup','subject',player.auth_cleanup_subject);
END $$;

CREATE OR REPLACE FUNCTION app.read_deletion_account_page(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion account page bounds'; END IF;
 SELECT coalesce(jsonb_agg(page.id ORDER BY page.id),'[]') INTO result FROM (
  SELECT id FROM app.account WHERE (p_after IS NULL OR id>p_after)
   AND (state='deletion_pending' OR state='deleted' AND app.account_auth_cleanup_receipt(id) IS NULL)
  ORDER BY id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('items',result,'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->>-1 ELSE NULL END);
END $$;

REVOKE ALL ON FUNCTION app.account_auth_cleanup_receipt(uuid),app.read_account_retirement_work(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_account_retirement_work(uuid),app.retire_account(uuid) TO service_role;
CREATE OR REPLACE FUNCTION app.observe_account_deletion(p_account uuid,p_request uuid,p_status_key_hash text)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; player app.account; retirement app.command_receipt;
 cleanup app.command_receipt; unavailable jsonb; result jsonb;
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
 -- Retirement is a shared private proof; capability and external cleanup
 -- remain this observer's distinct concerns.
 IF player.auth_subject IS NOT NULL OR player.auth_cleanup_subject IS NOT NULL THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 retirement:=app.read_account_retirement_proof(player.id,player.retirement_operation_id);
 IF retirement.id IS NULL THEN RETURN jsonb_build_object('status','success','data',result); END IF;
 result:=result||CASE WHEN app.account_auth_cleanup_receipt(player.id) IS NULL THEN '{}'::jsonb
  ELSE jsonb_build_object('phase','deleted','authCleanupReceiptId',app.account_auth_cleanup_receipt(player.id)) END;
 RETURN jsonb_build_object('status','success','data',result);
END $$;

COMMIT;
