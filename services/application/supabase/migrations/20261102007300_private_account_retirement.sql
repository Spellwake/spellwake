-- Private producer first: runtime execution is gated on the complete source review.
-- No current worker/API receives a finalization capability from this migration.
BEGIN;

CREATE FUNCTION app.read_account_retirement_proof(p_account uuid,p_operation uuid) RETURNS app.command_receipt
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE finalization app.command_receipt; retirement app.command_receipt; terms jsonb;
BEGIN
 PERFORM app.check_registered_auth_retirement(p_account,p_operation);
 SELECT * INTO finalization FROM app.command_receipt WHERE principal_key='human:'||p_account::text
 AND scope='account_retirement_v1' AND request_ref=p_account::text;
 terms:=jsonb_build_object('version',1,'scope','account_retirement_v1','accountId',p_account);
 IF finalization.id IS NULL OR finalization.state<>'accepted' OR finalization.account_id IS DISTINCT FROM p_account
 OR finalization.target_ref IS DISTINCT FROM p_account::text OR finalization.operation_id IS DISTINCT FROM p_operation
 OR finalization.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
 OR finalization.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account)) THEN
  RETURN NULL; END IF;
 SELECT * INTO retirement FROM app.command_receipt WHERE id=(finalization.result_ref::jsonb->>'authRetirementId')::uuid;
 IF retirement.id IS NULL OR retirement.scope<>'auth_retirement_v1' OR retirement.state<>'accepted'
 OR retirement.target_ref IS DISTINCT FROM p_account::text THEN RETURN NULL; END IF;
 PERFORM app.check_auth_receipt_shape(retirement);
 IF retirement.result_ref::jsonb IS DISTINCT FROM jsonb_build_object('version',1,
  'proofHash',app.auth_json_hash(jsonb_build_object('version',1,'source','application_retirement',
   'accountId',p_account::text,'retirementOperationId',p_operation::text))) THEN RETURN NULL; END IF;
 IF finalization.result_ref::jsonb IS DISTINCT FROM jsonb_build_object('version',1,'policy','account_minimization_v1',
  'retirementOperationId',p_operation,'authRetirementId',retirement.id) THEN RETURN NULL; END IF;
 RETURN retirement;
END $$;

-- Call only under the candidate's strongest account lock at READ COMMITTED.
-- Missing producer domains remain explicit blockers, even for completed labels.
CREATE FUNCTION app.account_retirement_blocked(p_account uuid,p_owner uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT
 EXISTS(SELECT 1 FROM app.trade_offer WHERE p_account IN (sender_id,recipient_id) AND state NOT IN ('accepted','rejected','canceled'))
 OR EXISTS(SELECT 1 FROM app.market_listing WHERE seller_id=p_account AND state NOT IN ('sold','canceled'))
 OR EXISTS(SELECT 1 FROM app.prediction_order WHERE account_id=p_account AND state='open')
 OR EXISTS(SELECT 1 FROM app.prediction_order o JOIN app.prediction_market m ON m.id=o.market_id
  WHERE o.account_id=p_account AND o.filled_quantity>0 AND (m.state<>'resolved' OR o.state<>'settled'
   OR NOT EXISTS(SELECT 1 FROM app.ledger_operation l WHERE l.id=m.settlement_operation_id AND l.state='committed'
    AND l.kind='prediction_settlement' AND l.source_scope='prediction_settlement' AND l.source_ref=m.id::text)))
 OR EXISTS(SELECT 1 FROM app.asset_hold WHERE owner_id=p_owner AND state='held')
 OR EXISTS(SELECT 1 FROM app.ledger_operation WHERE actor_account_id=p_account AND state='pending')
 OR EXISTS(SELECT 1 FROM app.command_receipt WHERE account_id=p_account AND state='pending')
 OR EXISTS(SELECT 1 FROM app.purchase p WHERE p.account_id=p_account AND (
  p.checkout_terms IS NULL OR p.state NOT IN ('paid','refunded','reversed','canceled')
  OR NOT (p.state='canceled' AND p.first_create_attempt_at IS NULL) AND NOT EXISTS(
   SELECT 1 FROM app.provider_event e WHERE e.id=p.provider_event_id AND e.provider='stripe' AND e.account_id=p_account
    AND e.processing_state='processed' AND e.effect->>'purchaseId'=p.id::text AND e.effect->>'disposition'='applied'
    AND e.effect->'result'->>'state'=p.state)))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='stripe' AND e.processing_state IN ('received','verified')
  AND (e.account_id=p_account OR EXISTS(SELECT 1 FROM app.purchase p WHERE p.account_id=p_account AND p.stripe_checkout_ref=e.capture->>'targetId')))
 -- Provider identity/role minimization needs the actual association producer.
 OR EXISTS(SELECT 1 FROM app.patreon_membership WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.provider_event WHERE account_id=p_account AND provider='patreon')
 -- A result row or a completed state is not a qualified host/effects certificate.
 OR EXISTS(SELECT 1 FROM app.match_seat WHERE p_account IN (account_id,sponsor_id))
 OR EXISTS(SELECT 1 FROM app.room WHERE host_id=p_account)
 OR EXISTS(SELECT 1 FROM app.room_slot WHERE p_account IN (account_id,sponsor_id))
 OR EXISTS(SELECT 1 FROM app.queue_entry WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.invitation WHERE issuer_id=p_account)
 OR EXISTS(SELECT 1 FROM app.ai_loss_bucket WHERE sponsor_id=p_account)
 OR EXISTS(SELECT 1 FROM app.game_participation WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.game_activity_fact WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.quest_assignment WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.competitive_result WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.rank WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.daily_win_bucket WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.daily_quest_state WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.monthly_account_activity WHERE account_id=p_account)
 OR EXISTS(SELECT 1 FROM app.set_ballot WHERE account_id=p_account)
$$;

CREATE FUNCTION app.retire_account(p_account uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE subject uuid; player app.account; owner app.asset_owner; receipt app.command_receipt;
 retirement app.command_receipt; operation uuid; auth_retirement uuid; item record;
 free_back uuid; free_sleeve uuid; free_dice uuid; requested boolean; terms jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='account retirement requires read committed retry'; END IF;
 IF p_account IS NULL OR p_account='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='account retirement identity required'; END IF;
 SELECT auth_subject INTO subject FROM app.account WHERE id=p_account;
 IF subject IS NOT NULL THEN
  PERFORM id FROM auth.users WHERE id=subject FOR KEY SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='retirement Auth parent unavailable'; END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended('signup:'||subject::text,0));
 END IF;
 -- Strongest account mode from entry: receipt FK KEY SHARE cannot interleave
 -- before a later lock upgrade, and registration already takes signup first.
 SELECT * INTO player FROM app.account WHERE id=p_account FOR UPDATE;
 IF NOT FOUND THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 IF player.state='deleted' THEN
  retirement:=app.read_account_retirement_proof(player.id,player.retirement_operation_id);
  IF retirement.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canonical retirement proof unavailable'; END IF;
  RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',player.id,'state','retired','retirementOperationId',player.retirement_operation_id));
 END IF;
 IF player.state<>'deletion_pending' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 IF subject IS NULL OR player.auth_subject IS DISTINCT FROM subject THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='retirement subject changed; retry'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('auth_authority_v1:'||app.auth_uuid_hash(subject),0));
 -- A lifecycle flag alone is not the user's accepted session-bound intent.
 SELECT EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.account_id=p_account AND r.principal_key='human:'||p_account::text
  AND r.scope='account_delete' AND r.target_ref=p_account::text AND r.state='accepted'
  AND substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"') IS NOT NULL
  AND r.result_ref=jsonb_build_object('version',1,'statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"'))::text
  AND r.payload_hash=app.auth_json_hash(jsonb_build_object('statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"')))
  AND r.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account))) INTO requested;
 SELECT * INTO owner FROM app.asset_owner WHERE account_id=p_account FOR UPDATE;
 IF NOT FOUND OR owner.state<>'active' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement owner unavailable'; END IF;
 IF NOT requested OR app.account_retirement_blocked(player.id,owner.id) THEN
  RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',player.id,'state','waiting')); END IF;
 IF EXISTS(SELECT 1 FROM app.deck WHERE account_id=p_account) THEN
  free_back:=app.free_cosmetic(p_account,'back'); free_sleeve:=app.free_cosmetic(p_account,'sleeve'); free_dice:=app.free_cosmetic(p_account,'dice');
  IF free_back IS NULL OR free_sleeve IS NULL OR free_dice IS NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement free appearance unavailable'; END IF;
 END IF;
 PERFORM id FROM app.inventory_asset WHERE owner_id=owner.id ORDER BY id FOR NO KEY UPDATE;
 terms:=jsonb_build_object('version',1,'scope','account_retirement_v1','accountId',p_account);
 receipt:=app.begin_account_command(p_account,p_account,'account_retirement_v1',p_account::text,terms);
 IF receipt.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement receipt conflicts with lifecycle'; END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('retirement',p_account::text,'account_retirement',p_account,clock_timestamp(),'pending') RETURNING id INTO operation;
 FOR item IN SELECT id,owned_quantity FROM app.inventory_asset WHERE owner_id=owner.id AND owned_quantity>0 ORDER BY id LOOP
  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
  VALUES(operation,item.id,'debit',item.owned_quantity,'retirement',p_account::text);
 END LOOP;
 IF EXISTS(SELECT 1 FROM app.inventory_asset WHERE owner_id=owner.id AND owned_quantity<>0)
 OR EXISTS(SELECT 1 FROM app.asset_hold WHERE owner_id=owner.id AND state='held') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement assets remain'; END IF;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.account SET state='deleted',retirement_operation_id=operation,deletion_finalized_at=clock_timestamp() WHERE id=p_account;
 UPDATE app.asset_owner SET state='retired' WHERE id=owner.id;
 UPDATE app.deck SET name='Deleted deck',back_id=free_back,sleeve_id=free_sleeve,dice_id=free_dice,playmat_id=NULL WHERE account_id=p_account;
 DELETE FROM app.profile WHERE account_id=p_account;
 DELETE FROM app.notification WHERE recipient_id=p_account;
 DELETE FROM app.friendship WHERE p_account IN (account_low_id,account_high_id);
 UPDATE app.cosmetic_grant SET state='revoked' WHERE account_id=p_account AND state='active';
 UPDATE app.purchase SET checkout_url=NULL WHERE account_id=p_account AND checkout_url IS NOT NULL;
 auth_retirement:=app.record_registered_auth_retirement(subject,p_account,operation);
 UPDATE app.account SET auth_cleanup_subject=subject,auth_subject=NULL WHERE id=p_account;
 UPDATE app.command_receipt SET operation_id=operation,result_ref=jsonb_build_object('version',1,'policy','account_minimization_v1',
  'retirementOperationId',operation,'authRetirementId',auth_retirement)::text WHERE id=receipt.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_account));
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',p_account,'state','retired','retirementOperationId',operation));
END $$;

REVOKE ALL ON FUNCTION app.read_account_retirement_proof(uuid,uuid),app.account_retirement_blocked(uuid,uuid),app.retire_account(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

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
 SELECT * INTO cleanup FROM app.command_receipt WHERE principal_key=retirement.principal_key AND scope='auth_cleanup_v1' AND request_ref='primary';
 IF cleanup.id IS NULL OR cleanup.state<>'accepted' OR cleanup.target_ref IS DISTINCT FROM p_account::text THEN
  RETURN jsonb_build_object('status','success','data',result); END IF;
 PERFORM app.check_auth_receipt_shape(cleanup);
 RETURN jsonb_build_object('status','success','data',result||jsonb_build_object('phase','deleted','authCleanupReceiptId',cleanup.id));
END $$;

COMMIT;
