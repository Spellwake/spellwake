BEGIN;
CREATE FUNCTION app.account_minimization_actions() RETURNS jsonb LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT '["profile","recipient_notifications","friendships","deck_labels_and_appearance","cosmetic_access","checkout_urls","patreon_entitlements","controller_authority","private_username_reservation","resource_cleanup_intents"]'::jsonb
$$;
CREATE FUNCTION app.valid_account_minimization(p_account uuid,p_retirement uuid,p_source jsonb) RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE a app.account; r app.command_receipt; expected jsonb; appearance jsonb; memberships uuid[]; jobs uuid[]; job app.command_receipt; grant_row app.controller_grant; terms jsonb;
BEGIN
 SELECT * INTO a FROM app.account WHERE id=p_account;
 SELECT * INTO r FROM app.command_receipt WHERE id=p_retirement;
 IF a.id IS NULL OR a.state<>'deleted' OR r.account_id IS DISTINCT FROM a.id OR r.scope IS DISTINCT FROM 'account_retirement_v1' OR r.operation_id IS DISTINCT FROM a.retirement_operation_id OR jsonb_typeof(p_source) IS DISTINCT FROM 'object' THEN RETURN false;END IF;
 appearance:=p_source->'freeAppearance';
 IF jsonb_typeof(appearance) IS DISTINCT FROM 'object' OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(appearance) key) IS DISTINCT FROM ARRAY['backId','diceId','sleeveId']::text[] OR EXISTS(SELECT 1 FROM jsonb_each(appearance) value WHERE jsonb_typeof(value.value) NOT IN('string','null') OR (jsonb_typeof(value.value)='string' AND value.value#>>'{}' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')) THEN RETURN false;END IF;
 SELECT coalesce(array_agg(source_event_id ORDER BY source_event_id),'{}'::uuid[]) INTO memberships FROM app.patreon_membership WHERE account_id=p_account;
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO jobs FROM app.command_receipt WHERE principal_key='account-resources:'||p_account::text AND scope='account_resource_cleanup_v1';
 expected:=jsonb_build_object('version','account-minimization-v2','accountId',a.id,'intentId',app.accepted_deletion_intent(a.id),'retirementOperationId',a.retirement_operation_id,'usernameHash',app.auth_json_hash(to_jsonb(a.username)),'actions',app.account_minimization_actions(),'freeAppearance',appearance,'patreonSourceIds',to_jsonb(memberships),'resourceWorkIds',to_jsonb(jobs));
 IF p_source IS DISTINCT FROM expected OR app.accepted_deletion_intent(a.id) IS NULL THEN RETURN false;END IF;
 IF NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE account_id=a.id AND state='retired') OR EXISTS(SELECT 1 FROM app.inventory_asset inventory JOIN app.asset_owner owner ON owner.id=inventory.owner_id WHERE owner.account_id=a.id AND inventory.owned_quantity<>0)
 OR EXISTS(SELECT 1 FROM app.asset_hold h JOIN app.asset_owner owner ON owner.id=h.owner_id WHERE owner.account_id=a.id AND h.state='held')
 OR EXISTS(SELECT 1 FROM app.profile WHERE account_id=a.id) OR EXISTS(SELECT 1 FROM app.notification WHERE recipient_id=a.id) OR EXISTS(SELECT 1 FROM app.friendship WHERE a.id IN(account_low_id,account_high_id))
 OR EXISTS(SELECT 1 FROM app.deck d WHERE d.account_id=a.id AND (d.name<>'Deleted deck' OR d.back_id::text IS DISTINCT FROM appearance->>'backId' OR d.sleeve_id::text IS DISTINCT FROM appearance->>'sleeveId' OR d.dice_id::text IS DISTINCT FROM appearance->>'diceId' OR d.playmat_id IS NOT NULL))
 OR EXISTS(SELECT 1 FROM app.cosmetic_grant WHERE account_id=a.id AND state='active') OR EXISTS(SELECT 1 FROM app.purchase WHERE account_id=a.id AND checkout_url IS NOT NULL)
 OR EXISTS(SELECT 1 FROM app.patreon_membership m WHERE m.account_id=a.id AND NOT app.valid_patreon_retirement_source(m)) THEN RETURN false;END IF;
 FOR grant_row IN SELECT g.* FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE a.id IN(slot.sponsor_id,room.host_id) LOOP
 terms:=app.account_resource_terms(a.id,grant_row.id,r.id);
 IF terms IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt work WHERE work.id=ANY(jobs) AND work.principal_key='account-resources:'||a.id::text AND work.scope='account_resource_cleanup_v1' AND work.request_ref=grant_row.id::text AND work.target_ref=grant_row.id::text AND work.normalized_source=terms AND work.payload_hash=app.auth_json_hash(terms) AND work.state='accepted' AND work.account_id IS NULL AND work.operation_id IS NULL AND work.controller_grant_id IS NULL AND work.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',grant_row.id,'kind','controller_secret','disposition','cleanup pending'))) THEN RETURN false;END IF;
 END LOOP;
 FOR job IN SELECT * FROM app.command_receipt WHERE id=ANY(jobs) LOOP
 IF job.state<>'accepted' OR NOT EXISTS(SELECT 1 FROM app.controller_grant g WHERE g.id::text=job.target_ref AND job.normalized_source=app.account_resource_terms(a.id,g.id,r.id)) THEN RETURN false;END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE a.id IN(slot.sponsor_id,room.host_id) AND (b.bootstrap_receipt_id IS NULL OR b.desired_state<>'stopped' OR NOT app.ai_stop_confirmed(b.id,b.stop_receipt_id) OR EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)))) THEN RETURN false;END IF;
 RETURN true;
END $$;
CREATE FUNCTION app.patreon_retirement_blocked(p_account uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.patreon_membership m JOIN app.provider_event source ON source.id=m.source_event_id WHERE m.account_id=p_account AND (m.patreon_user_ref IS NULL OR source.processing_state<>'processed' OR source.event_type NOT IN('oauth.association','api.membership_refresh') OR source.capture->>'userId' IS DISTINCT FROM m.patreon_user_ref OR source.capture->>'memberId' IS DISTINCT FROM m.patreon_member_ref OR source.capture->>'campaignId' IS DISTINCT FROM m.patreon_campaign_ref))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='patreon' AND e.processing_state IN('received','verified') AND (e.account_id=p_account OR EXISTS(SELECT 1 FROM app.patreon_membership m WHERE m.account_id=p_account AND (m.patreon_member_ref IN(e.capture->>'memberRef',e.capture->>'memberId',e.capture->>'targetId') OR m.patreon_user_ref=e.capture->>'userId'))))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='patreon' AND e.account_id=p_account AND e.processing_state='processed' AND (e.event_type NOT IN('oauth.association','api.membership_refresh','member.notification','financial.observation','application.retirement') OR e.capture IS NULL OR e.effect IS NULL))
 OR EXISTS(SELECT 1 FROM app.patreon_payment payment JOIN app.patreon_membership member ON member.id=payment.membership_id WHERE member.account_id=p_account AND NOT EXISTS(SELECT 1 FROM app.provider_event e WHERE e.id=payment.provider_event_id AND e.provider='patreon' AND e.account_id=p_account AND e.processing_state='processed' AND e.event_type='financial.observation' AND e.capture->>'family'='patreon-financial-v1' AND e.capture->>'paymentRef'=payment.payment_ref AND e.capture->>'state'=payment.state AND e.effect->>'kind'='patreon_financial_v1' AND e.effect->>'membershipId'=member.id::text AND e.effect->'result'->>'paymentId'=payment.id::text AND e.effect->'result'->>'state'=payment.state))
$$;
CREATE OR REPLACE FUNCTION app.account_retirement_blocked(p_account uuid,p_owner uuid) RETURNS boolean
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
 -- Preserve canonical private financial/association history; only unresolved work blocks.
 OR app.patreon_retirement_blocked(p_account)
 -- Game and accounting sources must drain before the one-way minimization commit.
 OR app.account_game_retirement_blocked(p_account)
$$;
CREATE OR REPLACE FUNCTION app.retire_account(p_account uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE subject uuid; player app.account; owner app.asset_owner; receipt app.command_receipt;
 retirement app.command_receipt; operation uuid; auth_retirement uuid; item record;
 free_back uuid; free_sleeve uuid; free_dice uuid; requested boolean; terms jsonb; minimization jsonb; resources uuid[]; patreon_sources uuid[];
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
 IF requested THEN PERFORM app.cleanup_pre_game_lobbies(player.id,100); END IF;
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
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 UPDATE app.account SET state='deleted',retirement_operation_id=operation,deletion_finalized_at=clock_timestamp() WHERE id=p_account;
 UPDATE app.asset_owner SET state='retired' WHERE id=owner.id;
 UPDATE app.deck SET name='Deleted deck',back_id=free_back,sleeve_id=free_sleeve,dice_id=free_dice,playmat_id=NULL WHERE account_id=p_account;
 DELETE FROM app.profile WHERE account_id=p_account;
 DELETE FROM app.notification WHERE recipient_id=p_account;
 DELETE FROM app.friendship WHERE p_account IN (account_low_id,account_high_id);
 UPDATE app.cosmetic_grant SET state='revoked' WHERE account_id=p_account AND state='active';
 UPDATE app.purchase SET checkout_url=NULL WHERE account_id=p_account AND checkout_url IS NOT NULL;
 patreon_sources:=app.retire_patreon_memberships(p_account,receipt.id);
 resources:=app.record_account_resource_work(p_account,receipt.id);
 minimization:=jsonb_build_object('version','account-minimization-v2','accountId',p_account,'intentId',app.accepted_deletion_intent(p_account),'retirementOperationId',operation,'usernameHash',app.auth_json_hash(to_jsonb(player.username)),'actions',app.account_minimization_actions(),'freeAppearance',jsonb_build_object('backId',free_back,'sleeveId',free_sleeve,'diceId',free_dice),'patreonSourceIds',to_jsonb(patreon_sources),'resourceWorkIds',to_jsonb(resources));
 UPDATE app.command_receipt SET normalized_source=minimization WHERE id=receipt.id;
 auth_retirement:=app.record_registered_auth_retirement(subject,p_account,operation);
 UPDATE app.account SET auth_cleanup_subject=subject,auth_subject=NULL WHERE id=p_account;
 UPDATE app.command_receipt SET operation_id=operation,result_ref=jsonb_build_object('version',2,'policy','account_minimization_v2','minimizationHash',app.auth_json_hash(minimization),
  'retirementOperationId',operation,'authRetirementId',auth_retirement)::text WHERE id=receipt.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_account));
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',p_account,'state','retired','retirementOperationId',operation));
END $$;
CREATE OR REPLACE FUNCTION app.read_account_retirement_proof(p_account uuid,p_operation uuid) RETURNS app.command_receipt
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
 IF finalization.result_ref::jsonb IS DISTINCT FROM jsonb_build_object('version',2,'policy','account_minimization_v2','minimizationHash',app.auth_json_hash(finalization.normalized_source),
  'retirementOperationId',p_operation,'authRetirementId',retirement.id) THEN RETURN NULL; END IF;
 IF NOT app.valid_account_minimization(p_account,finalization.id,finalization.normalized_source) THEN RETURN NULL;END IF;
 RETURN retirement;
END $$;
REVOKE ALL ON FUNCTION app.account_minimization_actions(),app.valid_account_minimization(uuid,uuid,jsonb),app.patreon_retirement_blocked(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
