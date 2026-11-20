BEGIN;
CREATE FUNCTION app.settle_ante(p_commitment uuid,p_result uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; r app.match_result; receipt app.command_receipt; actors uuid[]; assets uuid[];
 seed text; allocation jsonb; item jsonb; source_stake app.ante_stake; recipient app.match_seat; exposure app.ai_loss_exposure;
 operation uuid; owner uuid; source_owner uuid; source_inventory uuid; target_inventory uuid; asset uuid; quantity numeric; loss numeric; classification text; source_ref text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante settlement requires read committed retry'; END IF;
 IF p_commitment IS NULL OR p_result IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='ante settlement source required'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('ante-settlement:'||p_commitment::text,'ante_settlement_v1',p_result::text,p_commitment::text,app.auth_json_hash(jsonb_build_object('commitment',p_commitment,'result',p_result)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ante-settlement:'||p_commitment::text AND scope='ante_settlement_v1' AND request_ref=p_commitment::text FOR UPDATE;
 IF receipt.target_ref<>p_result::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante settlement source was retargeted'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante settlement pending'; END IF;RETURN receipt.response_payload;
 END IF;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO actors FROM(SELECT account_id id FROM app.match_seat WHERE match_id=c.match_id AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=c.match_id AND sponsor_id IS NOT NULL) participants;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.match WHERE id=c.match_id FOR NO KEY UPDATE;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment FOR NO KEY UPDATE;
 SELECT * INTO r FROM app.match_result WHERE id=p_result FOR SHARE;
 IF c.id IS NULL OR c.state<>'committed' OR c.terms IS NULL OR r.id IS NULL OR r.match_id<>c.match_id OR r.winner_seat_ids IS NULL
 OR EXISTS(SELECT 1 FROM unnest(actors) a WHERE NOT EXISTS(SELECT 1 FROM app.account WHERE id=a AND state IN ('active','deletion_pending'))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante settlement requires committed funding and its terminal match source'; END IF;
 PERFORM app.check_ante_source(c.id);
 seed:=encode(sha256(convert_to(gen_random_uuid()::text||':'||gen_random_uuid()::text,'UTF8')),'hex');allocation:=app.build_ante_allocation(c.id,r.id,seed);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,created_at,state)
 VALUES(CASE WHEN r.outcome='win' THEN 'ante_settlement' ELSE 'ante_refund' END,c.id::text,'ante_settlement_v1',clock_timestamp(),'pending') RETURNING id INTO operation;
 FOR item IN SELECT value FROM jsonb_array_elements(allocation->'allocations') WHERE value->>'classification' IN ('transfer','ai_issue') ORDER BY value->>'sourceAssetId',value->>'recipientSeatId' LOOP
 SELECT * INTO STRICT recipient FROM app.match_seat WHERE id=(item->>'recipientSeatId')::uuid;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=recipient.account_id AND state='active';PERFORM app.ensure_inventory(owner,(item->>'sourceAssetId')::uuid);
 END LOOP;
 SELECT array_agg(DISTINCT (value->>'sourceAssetId')::uuid) INTO assets FROM jsonb_array_elements(allocation->'allocations');
 PERFORM i.id FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id=ANY(actors) AND i.asset_definition_id=ANY(assets) ORDER BY i.id FOR NO KEY UPDATE OF i;
 UPDATE app.asset_hold h SET state=CASE WHEN r.outcome<>'win' OR s.match_seat_id=ANY(r.winner_seat_ids) THEN 'released' ELSE 'consumed' END FROM app.ante_stake s WHERE s.commitment_id=c.id AND h.id=s.hold_id AND h.state='held';
 FOR item IN SELECT * FROM jsonb_array_elements(allocation->'allocations') LOOP
 classification:=item->>'classification';IF classification IN ('return','ai_unissued') THEN CONTINUE; END IF;
 asset:=(item->>'sourceAssetId')::uuid;quantity:=(item->>'quantity')::numeric;source_ref:='ante-allocation:'||app.auth_json_hash(item);
 SELECT * INTO STRICT source_stake FROM app.ante_stake WHERE id=(item->>'sourceStakeId')::uuid;
 IF classification IN ('transfer','retire') THEN
 SELECT owner_id INTO STRICT source_owner FROM app.asset_hold WHERE id=source_stake.hold_id;
 SELECT id INTO STRICT source_inventory FROM app.inventory_asset WHERE owner_id=source_owner AND asset_definition_id=asset;
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES(operation,source_inventory,'debit',quantity,CASE WHEN classification='retire' THEN 'ante_retire' ELSE 'ante_transfer' END,source_ref);
 END IF;
 IF classification IN ('transfer','ai_issue') THEN
 SELECT * INTO STRICT recipient FROM app.match_seat WHERE id=(item->>'recipientSeatId')::uuid;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=recipient.account_id AND state='active';target_inventory:=app.ensure_inventory(owner,asset);
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES(operation,target_inventory,'credit',quantity,CASE WHEN classification='ai_issue' THEN 'ai_ante_loss' ELSE 'ante_transfer' END,source_ref);
 END IF;
 END LOOP;
 FOR exposure IN SELECT * FROM app.ai_loss_exposure WHERE commitment_id=c.id ORDER BY bucket_id,id FOR NO KEY UPDATE LOOP
 SELECT coalesce(sum(s.estimated_value_coins),0) INTO loss FROM app.ante_stake s WHERE s.ai_exposure_id=exposure.id AND r.outcome='win' AND NOT(s.match_seat_id=ANY(r.winner_seat_ids));
 UPDATE app.ai_loss_exposure SET state=CASE WHEN loss>0 THEN 'settled' ELSE 'released' END,loss_coins=loss WHERE id=exposure.id;
 UPDATE app.ai_loss_bucket SET lost_coins=lost_coins+loss,revision=revision+1 WHERE id=exposure.bucket_id;
 END LOOP;
 UPDATE app.ante_commitment SET state=allocation->>'kind',settlement_result_id=r.id,settlement=allocation,allocation_evidence_ref='sha256:'||app.auth_json_hash(allocation),settlement_operation_id=operation WHERE id=c.id;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'state',allocation->>'kind','operationId',operation,'allocationRef','sha256:'||app.auth_json_hash(allocation)));
END $$;
REVOKE ALL ON FUNCTION app.settle_ante(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
