BEGIN;
CREATE FUNCTION app.open_supply_ballot(p_season uuid,p_manifest jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE season_row app.season; receipt app.command_receipt; candidate jsonb; product jsonb; fingerprint text; previous uuid; candidate_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot opening requires read committed retry'; END IF;
 IF p_season IS NULL OR jsonb_typeof(p_manifest) IS DISTINCT FROM 'object' OR octet_length(p_manifest::text)>1048576 OR p_manifest->>'version' IS DISTINCT FROM 'supply-ballot-v1'
 OR jsonb_typeof(p_manifest->'supplyCoefficient') IS DISTINCT FROM 'string' OR p_manifest->>'supplyCoefficient' !~ '^[1-9][0-9]{0,8}$'
 OR jsonb_typeof(p_manifest->'sourceRef') IS DISTINCT FROM 'string' OR p_manifest->>'sourceRef' !~ '^sha256:[0-9a-f]{64}$'
 OR jsonb_typeof(p_manifest->'tieSeed') IS DISTINCT FROM 'string' OR p_manifest->>'tieSeed' !~ '^[0-9a-f]{64}$'
 OR jsonb_typeof(p_manifest->'candidates') IS DISTINCT FROM 'array'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_manifest) k WHERE k NOT IN ('version','supplyCoefficient','sourceRef','tieSeed','candidates')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized ballot configuration'; END IF;
 fingerprint:=app.auth_json_hash(p_manifest);
 PERFORM app.lock_valuation_catalogue(true);
 SELECT * INTO season_row FROM app.season WHERE id=p_season FOR NO KEY UPDATE;
 IF season_row.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='ballot season missing'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('supply-season:'||p_season::text,'supply_ballot_open_v1',p_season::text,'initial',fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_open_v1' AND request_ref='initial' FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ballot configuration is frozen'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot opening pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 IF clock_timestamp()<((season_row.start_at AT TIME ZONE 'UTC')-interval '3 months') AT TIME ZONE 'UTC' OR clock_timestamp()>=season_row.start_at-interval '168 hours' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='outside current-quarter ballot opening window'; END IF;
 IF jsonb_array_length(p_manifest->'candidates') NOT BETWEEN 5 AND 10000 OR (SELECT count(DISTINCT x->>'setId') FROM jsonb_array_elements(p_manifest->'candidates') x)<>jsonb_array_length(p_manifest->'candidates') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='at least five distinct eligible winners are required'; END IF;
 SELECT id INTO previous FROM app.season WHERE end_at=season_row.start_at;
 IF previous IS NOT NULL AND EXISTS(SELECT 1 FROM app.command_receipt WHERE principal_key='supply-season:'||previous::text AND scope='supply_ballot_open_v1' AND state='accepted') AND NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE principal_key='supply-season:'||previous::text AND scope='supply_ballot_result_v1' AND state='accepted') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='previous drop must finalize before its successor candidate list freezes'; END IF;
 FOR candidate IN SELECT * FROM jsonb_array_elements(p_manifest->'candidates') LOOP
 IF jsonb_typeof(candidate) IS DISTINCT FROM 'object' OR jsonb_typeof(candidate->'setId') IS DISTINCT FROM 'string' OR candidate->>'setId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(candidate->'products') IS DISTINCT FROM 'array' OR EXISTS(SELECT 1 FROM jsonb_object_keys(candidate) k WHERE k NOT IN ('setId','products')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid ballot candidate'; END IF;
 candidate_id:=(candidate->>'setId')::uuid;
 IF NOT EXISTS(SELECT 1 FROM app.card_set WHERE id=candidate_id) OR EXISTS(SELECT 1 FROM app.supply_drop WHERE season_id=previous AND set_id=candidate_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='candidate missing or under previous-drop cooldown'; END IF;
 IF jsonb_array_length(candidate->'products') NOT BETWEEN 1 AND 1000 OR (SELECT count(DISTINCT p->>'recipeId') FROM jsonb_array_elements(candidate->'products') p)<>jsonb_array_length(candidate->'products') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='candidate needs distinct qualified products'; END IF;
 FOR product IN SELECT * FROM jsonb_array_elements(candidate->'products') LOOP
 IF jsonb_typeof(product) IS DISTINCT FROM 'object'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['recipeId','weight','normalQuantity','modifierN','modifierD','modifierRef']) k WHERE jsonb_typeof(product->k) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(product) k WHERE k NOT IN ('recipeId','weight','normalQuantity','modifierN','modifierD','modifierRef'))
 OR product->>'recipeId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR product->>'weight' !~ '^[1-9][0-9]{0,8}$' OR product->>'normalQuantity' !~ '^[1-9][0-9]{0,29}$' OR product->>'modifierN' !~ '^[1-9][0-9]{0,29}$' OR product->>'modifierD' !~ '^[1-9][0-9]{0,29}$' OR product->>'modifierRef' !~ '^sha256:[0-9a-f]{64}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid weighted ballot product'; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe r JOIN app.booster_product p ON p.id=r.product_id WHERE r.id=(product->>'recipeId')::uuid AND p.set_id=candidate_id AND r.qualified_at IS NOT NULL AND r.opening_qualified_at IS NOT NULL FOR SHARE OF r,p) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='candidate product lacks qualified collation/opening'; END IF;
 END LOOP;
 END LOOP;
 UPDATE app.command_receipt SET normalized_source=p_manifest||jsonb_build_object('openedAt',to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_season,'candidates',jsonb_array_length(p_manifest->'candidates')::text,'cutoff',to_char((season_row.start_at-interval '168 hours') AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')));
END $$;

CREATE FUNCTION app.replace_supply_ballot(p_actor uuid,p_season uuid,p_sets uuid[],p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; season_row app.season; opening app.command_receipt; next_revision numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot replacement requires read committed retry'; END IF;
 IF p_actor IS NULL OR p_season IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_sets IS NULL OR array_ndims(p_sets)>1 OR cardinality(p_sets)>10 OR array_position(p_sets,NULL) IS NOT NULL OR cardinality(p_sets)<>(SELECT count(DISTINCT id) FROM unnest(p_sets) id) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='up to ten distinct candidate approvals required'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'supply_ballot_v1',p_season::text,jsonb_build_object('season',p_season,'sets',p_sets));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO season_row FROM app.season WHERE id=p_season FOR NO KEY UPDATE;
 SELECT * INTO opening FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_open_v1' AND state='accepted';
 IF season_row.id IS NULL OR opening.id IS NULL OR clock_timestamp()>=season_row.start_at-interval '168 hours' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('ballot','closed or unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.game_participation WHERE account_id=p_actor AND real_game AND own_turns>=3 AND completed_at<=clock_timestamp()) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','qualifying real game required')); END IF;
 IF EXISTS(SELECT 1 FROM unnest(p_sets) chosen WHERE NOT EXISTS(SELECT 1 FROM jsonb_array_elements(opening.normalized_source->'candidates') candidate WHERE candidate->>'setId'=chosen::text)) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sets','outside frozen candidates')); END IF;
 SELECT coalesce(max(revision),0)+1 INTO next_revision FROM app.set_ballot WHERE season_id=p_season AND account_id=p_actor;
 DELETE FROM app.set_ballot WHERE season_id=p_season AND account_id=p_actor;
 INSERT INTO app.set_ballot(season_id,account_id,set_id,revision) SELECT p_season,p_actor,id,next_revision FROM unnest(p_sets) id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_season,'approvals',cardinality(p_sets)::text));
END $$;
REVOKE ALL ON FUNCTION app.open_supply_ballot(uuid,jsonb),app.replace_supply_ballot(uuid,uuid,uuid[],uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
