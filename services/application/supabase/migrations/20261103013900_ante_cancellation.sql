BEGIN;
CREATE FUNCTION app.cancel_ante(p_commitment uuid,p_seat uuid,p_expected numeric,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; receipt app.command_receipt; actors uuid[]; operation uuid; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante cancellation requires read committed retry'; END IF;
 IF p_commitment IS NULL OR p_seat IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid ante cancellation source'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('commitment',p_commitment,'seat',p_seat,'basis',p_expected::text));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref) VALUES('ante-seat:'||p_seat::text,'ante_cancel_v1',p_commitment::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ante-seat:'||p_seat::text AND scope='ante_cancel_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_commitment::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante cancellation terms changed'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante cancellation pending'; END IF;RETURN receipt.response_payload;
 END IF;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO actors FROM(SELECT account_id id FROM app.match_seat WHERE match_id=c.match_id AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=c.match_id AND sponsor_id IS NOT NULL) participants;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.match WHERE id=c.match_id FOR NO KEY UPDATE;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment FOR NO KEY UPDATE;
 IF c.id IS NULL OR c.terms IS NULL OR c.state<>'offered' OR c.terms_revision<>p_expected OR NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=p_seat AND match_id=c.match_id) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','ante cannot be canceled at this basis')); END IF;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,created_at,state) VALUES('ante_cancel',c.id::text,'ante_cancel_v1',clock_timestamp(),'pending') RETURNING id INTO operation;
 UPDATE app.asset_hold SET state='released' WHERE id IN(SELECT hold_id FROM app.ante_stake WHERE commitment_id=c.id) AND state='held';
 UPDATE app.ai_loss_exposure SET state='released',loss_coins=0 WHERE commitment_id=c.id AND state='reserved';
 UPDATE app.ante_commitment SET state='canceled',settlement_operation_id=operation WHERE id=c.id;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'state','canceled'));
END $$;
REVOKE ALL ON FUNCTION app.cancel_ante(uuid,uuid,numeric,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
