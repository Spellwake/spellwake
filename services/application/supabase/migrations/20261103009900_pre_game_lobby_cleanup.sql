BEGIN;

-- Cleanup is private database work, reached only after canonical deletion intent is proven.
CREATE FUNCTION app.cleanup_pre_game_lobbies(p_account uuid,p_limit integer DEFAULT 100) RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; receipt app.command_receipt; player app.account; cleaned integer:=0; role_name text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby cleanup requires read committed retry'; END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby cleanup limit'; END IF;
 SELECT * INTO player FROM app.account WHERE id=p_account FOR UPDATE;
 IF player.id IS NULL OR player.state<>'deletion_pending' OR NOT EXISTS(
  SELECT 1 FROM app.command_receipt r WHERE r.account_id=p_account AND r.principal_key='human:'||p_account::text
   AND r.scope='account_delete' AND r.target_ref=p_account::text AND r.state='accepted'
   AND substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"') IS NOT NULL
   AND r.result_ref=jsonb_build_object('version',1,'statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"'))::text
   AND r.payload_hash=app.auth_json_hash(jsonb_build_object('statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"')))
   AND r.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account))
 ) THEN RETURN 0; END IF;
 FOR lobby IN SELECT r.* FROM app.room r WHERE r.state IN ('lobby','countdown','closed')
  AND (r.host_id=p_account OR EXISTS(SELECT 1 FROM app.room_slot s WHERE s.room_id=r.id AND s.account_id=p_account AND s.controller_kind='human'))
  ORDER BY r.id FOR UPDATE OF r
 LOOP
  -- Recheck references after the strongest room lock. Concurrent FK insertions serialize here.
  IF EXISTS(SELECT 1 FROM app.match WHERE room_id=lobby.id)
   OR EXISTS(SELECT 1 FROM app.invitation WHERE room_id=lobby.id)
   OR EXISTS(SELECT 1 FROM app.room_slot s JOIN app.ai_runner_binding b ON b.room_slot_id=s.id WHERE s.room_id=lobby.id)
   OR EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=lobby.id AND (controller_kind<>'human' OR sponsor_id IS NOT NULL))
   OR NOT EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.account_id=lobby.host_id
    AND r.principal_key='human:'||lobby.host_id::text AND r.scope='room_create' AND r.target_ref=lobby.host_id::text AND r.state='accepted'
    AND r.payload_hash=app.auth_json_hash(jsonb_build_object('format',lobby.format_id,'profile',lobby.profile_id,'mode',lobby.mode,'competitive',lobby.competitive,'visibility',lobby.visibility))
    AND r.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',lobby.id))) THEN
   CONTINUE;
  END IF;
  role_name:=CASE WHEN lobby.host_id=p_account THEN 'host' ELSE 'guest' END;
  receipt:=app.begin_account_command(p_account,lobby.id,'lobby_retirement_v1',lobby.id::text,
   jsonb_build_object('version',1,'accountId',p_account,'roomId',lobby.id,'role',role_name));
  IF receipt.state<>'pending' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='lobby cleanup receipt contradicts retained participation'; END IF;
  IF role_name='host' THEN
   DELETE FROM app.room_slot WHERE room_id=lobby.id;
   DELETE FROM app.room WHERE id=lobby.id;
  ELSE
   UPDATE app.room_slot SET account_id=NULL,sponsor_id=NULL,deck_revision_id=NULL,ready=false
    WHERE room_id=lobby.id AND account_id=p_account AND controller_kind='human';
   UPDATE app.room SET state=CASE WHEN state='closed' THEN 'closed' ELSE 'lobby' END,countdown_at=NULL,revision=revision+1 WHERE id=lobby.id;
  END IF;
  PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',lobby.id));
  cleaned:=cleaned+1;
  IF cleaned>=p_limit THEN EXIT; END IF;
 END LOOP;
 RETURN cleaned;
END $$;
REVOKE ALL ON FUNCTION app.cleanup_pre_game_lobbies(uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.retire_account(p_account uuid) RETURNS jsonb
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

COMMIT;
