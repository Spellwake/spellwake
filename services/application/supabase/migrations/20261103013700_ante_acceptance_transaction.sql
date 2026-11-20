BEGIN;
CREATE FUNCTION app.accept_ante_terms(p_commitment uuid,p_seat uuid,p_expected numeric,p_hash text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; m app.match; seat app.match_seat; receipt app.command_receipt; actors uuid[]; accepted_ids uuid[];
 fingerprint text; stakes_count bigint; stake app.ante_stake; other app.ante_stake; own_value app.exact_value; largest app.exact_value; assessment jsonb; bundle jsonb; all_assets uuid[]; coin uuid; policy uuid;
 values_source jsonb; operation uuid; failure jsonb; rejected boolean:=false; day_key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante acceptance requires read committed retry'; END IF;
 IF p_commitment IS NULL OR p_seat IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_hash IS NULL OR p_hash !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid ante acceptance source'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('commitment',p_commitment,'seat',p_seat,'basis',p_expected::text,'termsHash',p_hash));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref) VALUES('ante-seat:'||p_seat::text,'ante_accept_v1',p_commitment::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ante-seat:'||p_seat::text AND scope='ante_accept_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_commitment::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante acceptance source terms changed'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante acceptance pending'; END IF;RETURN receipt.response_payload;
 END IF;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO actors FROM(SELECT account_id id FROM app.match_seat WHERE match_id=c.match_id AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=c.match_id AND sponsor_id IS NOT NULL) participants;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.match WHERE id=c.match_id FOR NO KEY UPDATE;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment FOR NO KEY UPDATE;
 SELECT * INTO seat FROM app.match_seat WHERE id=p_seat AND match_id=m.id;
 IF c.id IS NULL OR seat.id IS NULL OR c.state<>'offered' OR c.terms IS NULL OR c.terms_revision<>p_expected OR c.terms_hash<>p_hash OR m.state<>'ready'
 OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=m.reserved_game_id AND game_index=0 AND state='reserved')
 OR EXISTS(SELECT 1 FROM unnest(actors) a WHERE NOT EXISTS(SELECT 1 FROM app.account WHERE id=a AND state='active')) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','unavailable or stale ante agreement')); END IF;
 SELECT array_agg(id ORDER BY id) INTO accepted_ids FROM(SELECT DISTINCT unnest(c.accepted_seat_ids||ARRAY[p_seat]) id) accepted;
 IF cardinality(accepted_ids)<>(SELECT count(*) FROM app.match_seat WHERE match_id=m.id) THEN
 UPDATE app.ante_commitment SET accepted_seat_ids=accepted_ids WHERE id=c.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'state','offered','termsRevision',c.terms_revision::text)); END IF;
 BEGIN
 SELECT count(*) INTO stakes_count FROM app.ante_stake WHERE commitment_id=c.id;
 IF stakes_count=1 THEN failure:=jsonb_build_object('stakes','a positive stake cannot stand alone');RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='ante agreement rejected'; END IF;
 policy:=app.lock_valuation_catalogue(false);
 SELECT array_agg(DISTINCT i.asset_definition_id ORDER BY i.asset_definition_id) INTO all_assets FROM app.ante_stake s JOIN app.hold_line l ON l.hold_id=s.hold_id JOIN app.inventory_asset i ON i.id=l.inventory_asset_id WHERE s.commitment_id=c.id;
 PERFORM app.lock_assessment_products(coalesce(all_assets,'{}'::uuid[]));
 FOR stake IN SELECT * FROM app.ante_stake WHERE commitment_id=c.id ORDER BY match_seat_id FOR NO KEY UPDATE LOOP
 IF stake.funding_kind='owned_assets' THEN
 IF NOT EXISTS(SELECT 1 FROM app.asset_hold WHERE id=stake.hold_id AND state='held' FOR SHARE) THEN failure:=jsonb_build_object('assets','ante reservation unavailable');RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='ante agreement rejected'; END IF;
 SELECT jsonb_agg(jsonb_build_object('assetId',i.asset_definition_id,'quantity',l.quantity::text) ORDER BY i.asset_definition_id) INTO bundle FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id WHERE l.hold_id=stake.hold_id;
 ELSE SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';bundle:=jsonb_build_array(jsonb_build_object('assetId',coin,'quantity',stake.estimated_value_coins::text)); END IF;
 assessment:=app.assess_bundle(bundle);own_value:=app.value_make((assessment->'total'->>'numerator')::numeric,(assessment->'total'->>'denominator')::numeric);
 UPDATE app.ante_stake SET value_numerator=own_value.numerator,value_denominator=own_value.denominator,estimated_value_coins=div(own_value.numerator+own_value.denominator-1,own_value.denominator),valuation_policy_id=policy,bundle_assessment=assessment WHERE id=stake.id;
 END LOOP;
 FOR stake IN SELECT s.* FROM app.ante_stake s JOIN app.match_seat ms ON ms.id=s.match_seat_id WHERE s.commitment_id=c.id AND s.funding_kind='owned_assets' AND app.account_trade_restricted(ms.account_id) LOOP
 largest:=app.value_make(0,1);
 FOR other IN SELECT * FROM app.ante_stake WHERE commitment_id=c.id AND id<>stake.id LOOP
 own_value:=app.value_make(other.value_numerator,other.value_denominator);IF app.value_less(largest,own_value) THEN largest:=own_value; END IF;
 END LOOP;
 IF 10*stake.value_numerator*largest.denominator>13*largest.numerator*stake.value_denominator THEN failure:=jsonb_build_object('stakes','restricted outgoing stake exceeds130percent of the largest other stake');RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='ante agreement rejected'; END IF;
 END LOOP;
 day_key:=to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD');
 IF EXISTS(SELECT 1 FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=c.id AND (x.state<>'reserved' OR b.utc_day<>day_key OR x.reserved_coins<>(SELECT coalesce(sum(s.estimated_value_coins),0) FROM app.ante_stake s WHERE s.ai_exposure_id=x.id))) THEN failure:=jsonb_build_object('allowance','AI exposure differs from accepted start-day stakes');RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='ante agreement rejected'; END IF;
 IF app.ante_terms_manifest(c.id) IS DISTINCT FROM c.terms THEN failure:=jsonb_build_object('terms','ante asset sources changed');RAISE EXCEPTION USING ERRCODE='SW002',MESSAGE='ante agreement rejected'; END IF;
 SELECT jsonb_build_object('version','ante-valuations-v1','stakes',coalesce(jsonb_agg(jsonb_build_object('seatId',s.match_seat_id,'assessment',s.bundle_assessment) ORDER BY s.match_seat_id),'[]'::jsonb)) INTO values_source FROM app.ante_stake s WHERE commitment_id=c.id;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,created_at,state) VALUES('ante_commit',c.id::text,'ante_commit_v1',clock_timestamp(),'pending') RETURNING id INTO operation;
 UPDATE app.ante_commitment SET state=CASE WHEN stakes_count=0 THEN 'canceled' ELSE 'committed' END,accepted_seat_ids=accepted_ids,accepted_at=clock_timestamp(),start_day_utc=day_key,valuation_snapshot=values_source,valuation_evidence_ref='sha256:'||app.auth_json_hash(values_source),reservation_operation_id=operation WHERE id=c.id;
 IF stakes_count=0 THEN UPDATE app.ai_loss_exposure SET state='released',loss_coins=0 WHERE commitment_id=c.id AND state='reserved'; END IF;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 EXCEPTION WHEN SQLSTATE 'SW002' THEN rejected:=true;
 END;
 IF rejected THEN RETURN app.finish_account_command(receipt.id,false,failure); END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'state',CASE WHEN stakes_count=0 THEN 'canceled' ELSE 'committed' END,'termsRevision',c.terms_revision::text));
END $$;
REVOKE ALL ON FUNCTION app.accept_ante_terms(uuid,uuid,numeric,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
