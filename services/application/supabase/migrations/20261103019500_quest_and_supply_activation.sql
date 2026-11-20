BEGIN;
CREATE OR REPLACE FUNCTION app.generate_daily_quest(p_actor uuid,p_definition uuid,p_day text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; daily app.daily_quest_state; assignment_id uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'quest_generate',p_actor::text,
  jsonb_build_object('definition',p_definition,'day',p_day));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF app.valid_utc_day(p_day) IS DISTINCT FROM true OR p_day<>to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('day','not current UTC day')); END IF;
 SELECT * INTO daily FROM app.daily_quest_state WHERE account_id=p_actor AND utc_day=p_day FOR NO KEY UPDATE;
 IF daily.generated_assignment_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',daily.generated_assignment_id)); END IF;
 PERFORM id FROM app.quest_definition_revision WHERE id=p_definition AND fact_kind IS NOT NULL AND fact_schema_revision IS NOT NULL AND objective_qualification_ref IS NOT NULL;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('definition','unavailable')); END IF;
 IF (SELECT count(*) FROM app.quest_assignment WHERE account_id=p_actor AND completed_at IS NULL AND replaced_by_id IS NULL)>=3 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('bank','full')); END IF;
 INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress)
 VALUES(p_actor,p_definition,p_day,'daily:'||p_day,0) RETURNING id INTO assignment_id;
 INSERT INTO app.daily_quest_state(account_id,utc_day,generated_assignment_id,rerolls_used,revision)
 VALUES(p_actor,p_day,assignment_id,0,0)
 ON CONFLICT(account_id,utc_day) DO UPDATE SET generated_assignment_id=EXCLUDED.generated_assignment_id,revision=app.daily_quest_state.revision+1;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',assignment_id));
END $$;
CREATE OR REPLACE FUNCTION app.reroll_daily_quest(p_actor uuid,p_assignment uuid,p_definition uuid,p_day text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; daily app.daily_quest_state; previous app.quest_assignment; replacement uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'quest_reroll',p_assignment::text,
  jsonb_build_object('definition',p_definition,'day',p_day));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF app.valid_utc_day(p_day) IS DISTINCT FROM true OR p_day<>to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('day','not current UTC day')); END IF;
 SELECT * INTO previous FROM app.quest_assignment WHERE id=p_assignment AND account_id=p_actor FOR NO KEY UPDATE;
 IF previous.id IS NULL OR previous.completed_at IS NOT NULL OR previous.replaced_by_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assignment','unavailable')); END IF;
 IF p_definition IS NOT DISTINCT FROM previous.definition_id OR NOT EXISTS(SELECT 1 FROM app.quest_definition_revision WHERE id=p_definition AND fact_kind IS NOT NULL AND fact_schema_revision IS NOT NULL AND objective_qualification_ref IS NOT NULL) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('definition','different available definition required')); END IF;
 SELECT * INTO daily FROM app.daily_quest_state WHERE account_id=p_actor AND utc_day=p_day FOR NO KEY UPDATE;
 IF daily.rerolls_used=1 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('reroll','already used')); END IF;
 INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress)
 VALUES(p_actor,p_definition,p_day,'reroll:'||p_day||':'||receipt.id::text,0) RETURNING id INTO replacement;
 UPDATE app.quest_assignment SET replaced_by_id=replacement WHERE id=p_assignment;
 INSERT INTO app.daily_quest_state(account_id,utc_day,rerolls_used,revision) VALUES(p_actor,p_day,1,0)
 ON CONFLICT(account_id,utc_day) DO UPDATE SET rerolls_used=1,revision=app.daily_quest_state.revision+1;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',replacement,'replacedId',p_assignment));
END $$;
CREATE OR REPLACE FUNCTION app.finalize_quarterly_supply(p_season uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE season_row app.season; opening app.command_receipt; receipt app.command_receipt; cutoff timestamptz; month_start timestamp;
 month_index integer; count_value numeric; total_population numeric:=0; members_hash text; population jsonb:='[]'; tallies jsonb; winners jsonb;
 quantity numeric; source jsonb; allocation_ref text; candidate jsonb; selected jsonb; product jsonb; drop_id uuid; share numeric;
 set_index bigint:=0; total_weight numeric; assigned numeric; leftovers numeric; product_quantity numeric; product_index bigint; windows jsonb:='[]'; window_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quarterly supply requires read committed retry'; END IF;
 IF p_season IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='supply season required'; END IF;
 -- Host acceptance takes the shared fence before assigning effective source
 -- completion times and holds it through commit. Direct participation is used
 -- here, so delayed monthly consumers cannot omit a qualifying account.
 PERFORM pg_advisory_xact_lock(hashtextextended('supply-population-cutoff-v1',0));
 PERFORM app.lock_valuation_catalogue(true);
 SELECT * INTO season_row FROM app.season WHERE id=p_season FOR NO KEY UPDATE;
 IF season_row.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='supply season missing'; END IF;
 cutoff:=season_row.start_at-interval '168 hours';
 IF clock_timestamp()<season_row.start_at THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='supply quarter has not started';END IF;
 SELECT * INTO opening FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_open_v1' AND state='accepted';
 IF opening.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified frozen ballot required'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('supply-season:'||p_season::text,'supply_ballot_result_v1',p_season::text,'initial',app.auth_json_hash(jsonb_build_object('season',p_season,'opening',opening.id)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_result_v1' AND request_ref='initial' FOR UPDATE;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quarterly supply pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 month_start:=date_trunc('month',cutoff AT TIME ZONE 'UTC');
 FOR month_index IN REVERSE 3..1 LOOP
 SELECT count(*),encode(sha256(convert_to(coalesce(string_agg(account_id::text,',' ORDER BY account_id),''),'UTF8')),'hex') INTO count_value,members_hash FROM (
  SELECT DISTINCT account_id FROM app.game_participation WHERE real_game AND own_turns>=3 AND completed_at>=(month_start-make_interval(months=>month_index)) AT TIME ZONE 'UTC' AND completed_at<(month_start-make_interval(months=>month_index-1)) AT TIME ZONE 'UTC') accounts;
 total_population:=total_population+count_value;
 population:=population||jsonb_build_array(jsonb_build_object('month',to_char(month_start-make_interval(months=>month_index),'YYYY-MM'),'accounts',count_value::text,'accountSetHash','sha256:'||members_hash));
 END LOOP;
 quantity:=div((opening.normalized_source->>'supplyCoefficient')::numeric*total_population,3);
 tallies:=app.supply_ballot_tallies(p_season);
 SELECT jsonb_agg(value ORDER BY ordinal) INTO winners FROM jsonb_array_elements(tallies) WITH ORDINALITY rows(value,ordinal) WHERE ordinal<=5;
 IF jsonb_array_length(winners)<>5 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='five eligible winners required'; END IF;
 source:=jsonb_build_object('version','quarterly-supply-v1','seasonId',p_season,'openingReceiptId',opening.id,'openingHash',opening.payload_hash,'cutoff',to_char(cutoff AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'population',population,'populationSum',total_population::text,'coefficient',opening.normalized_source->>'supplyCoefficient','quantity',quantity::text,'tallies',tallies,'winners',winners);
 allocation_ref:='sha256:'||app.auth_json_hash(source);
 FOR selected IN SELECT value FROM jsonb_array_elements(winners) ORDER BY value->>'setId' LOOP
 share:=div(quantity,5)+CASE WHEN set_index<mod(quantity,5) THEN 1 ELSE 0 END;set_index:=set_index+1;
 INSERT INTO app.supply_drop(season_id,set_id,population_snapshot_ref,quantity,policy_ref)
 VALUES(p_season,(selected->>'setId')::uuid,allocation_ref,share,'quarterly-real-game-v1') RETURNING id INTO drop_id;
 SELECT value INTO STRICT candidate FROM jsonb_array_elements(opening.normalized_source->'candidates') WHERE value->>'setId'=selected->>'setId';
 SELECT sum((p->>'weight')::numeric) INTO total_weight FROM jsonb_array_elements(candidate->'products') p;
 SELECT sum(div(share*(p->>'weight')::numeric,total_weight)) INTO assigned FROM jsonb_array_elements(candidate->'products') p;
 leftovers:=share-assigned;product_index:=0;
 FOR product IN SELECT p FROM jsonb_array_elements(candidate->'products') p JOIN app.booster_recipe r ON r.id=(p->>'recipeId')::uuid ORDER BY r.product_id LOOP
 product_quantity:=div(share*(product->>'weight')::numeric,total_weight)+CASE WHEN product_index<leftovers THEN 1 ELSE 0 END;product_index:=product_index+1;
 IF product_quantity>0 THEN
 window_id:=app.publish_supply_window((product->>'recipeId')::uuid,product_quantity,(product->>'normalQuantity')::numeric,(product->>'modifierN')::numeric,(product->>'modifierD')::numeric,product->>'modifierRef','quarterly-v1:'||receipt.id::text||':'||(product->>'recipeId'),'quarterly',drop_id,allocation_ref,NULL);
 windows:=windows||jsonb_build_array(jsonb_build_object('recipeId',product->>'recipeId','windowId',window_id,'quantity',product_quantity::text));
 END IF;
 END LOOP;
 END LOOP;
 UPDATE app.command_receipt SET normalized_source=source WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_season,'allocationRef',allocation_ref,'quantity',quantity::text,'windows',windows::text));
END $$;
COMMIT;
