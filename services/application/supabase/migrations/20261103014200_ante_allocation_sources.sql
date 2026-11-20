BEGIN;
CREATE FUNCTION app.build_ante_allocation(p_commitment uuid,p_result uuid,p_seed text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; r app.match_result; stake app.ante_stake; line record; share jsonb; split jsonb; balances jsonb:='[]'; allocations jsonb:='[]';
 winners uuid[]; winner uuid; unit_value jsonb; coin uuid; returned boolean; human boolean;
 pending jsonb:='[]'; pending_item jsonb; chosen_line jsonb; chosen_index integer; index_value integer; comparison numeric;
BEGIN
 IF p_seed IS NULL OR p_seed !~ '^[0-9a-f]{64}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='recorded ante draw seed required'; END IF;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;SELECT * INTO r FROM app.match_result WHERE id=p_result;
 IF c.id IS NULL OR r.id IS NULL OR c.match_id<>r.match_id OR r.winner_seat_ids IS NULL OR c.terms IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante allocation requires its normalized terminal match result'; END IF;
 PERFORM app.validate_normalized_winners(c.match_id,r.outcome,r.winner_seat_ids);
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO winners FROM app.match_seat WHERE id=ANY(r.winner_seat_ids);
 FOREACH winner IN ARRAY winners LOOP balances:=balances||jsonb_build_array(jsonb_build_object('numerator','0','denominator','1')); END LOOP;
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';
 FOR stake IN SELECT * FROM app.ante_stake WHERE commitment_id=c.id ORDER BY match_seat_id LOOP
 returned:=r.outcome<>'win' OR stake.match_seat_id=ANY(winners);
 IF stake.funding_kind='ai_loss_allowance' THEN
 IF returned THEN allocations:=allocations||jsonb_build_array(jsonb_build_object('sourceStakeId',stake.id,'sourceAssetId',coin,'recipientSeatId',stake.match_seat_id,'quantity',stake.estimated_value_coins::text,'classification','ai_unissued'));
 ELSE pending:=pending||jsonb_build_array(jsonb_build_object('stakeId',stake.id,'assetId',coin,'quantity',stake.estimated_value_coins::text,'funding',stake.funding_kind,'numerator','1','denominator','1')); END IF;
 ELSE
 FOR line IN SELECT i.asset_definition_id asset,l.quantity FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id WHERE l.hold_id=stake.hold_id ORDER BY i.asset_definition_id LOOP
 IF returned THEN allocations:=allocations||jsonb_build_array(jsonb_build_object('sourceStakeId',stake.id,'sourceAssetId',line.asset,'recipientSeatId',stake.match_seat_id,'quantity',line.quantity::text,'classification','return'));
 ELSE
 SELECT value->'unitAssessment'->'value' INTO STRICT unit_value FROM jsonb_array_elements(stake.bundle_assessment->'lines') WHERE value->>'assetId'=line.asset::text;
 pending:=pending||jsonb_build_array(jsonb_build_object('stakeId',stake.id,'assetId',line.asset,'quantity',line.quantity::text,'funding',stake.funding_kind,'numerator',unit_value->>'numerator','denominator',unit_value->>'denominator'));
 END IF;
 END LOOP;
 END IF;
 END LOOP;
 -- At most six100-line stakes: exact cross-product selection orders the
 -- indivisible high-value units before coins, independent of UUID order.
 -- No floating price ordering or per-coin loop is involved.
 WHILE jsonb_array_length(pending)>0 LOOP
 chosen_line:=NULL;chosen_index:=NULL;index_value:=0;
 FOR pending_item IN SELECT * FROM jsonb_array_elements(pending) LOOP
 IF chosen_line IS NULL THEN chosen_line:=pending_item;chosen_index:=index_value;
 ELSE
 comparison:=(pending_item->>'numerator')::numeric*(chosen_line->>'denominator')::numeric-(chosen_line->>'numerator')::numeric*(pending_item->>'denominator')::numeric;
 IF comparison>0 OR (comparison=0 AND (pending_item->>'assetId',pending_item->>'stakeId')<(chosen_line->>'assetId',chosen_line->>'stakeId')) THEN chosen_line:=pending_item;chosen_index:=index_value; END IF;
 END IF;index_value:=index_value+1;
 END LOOP;
 pending:=pending-chosen_index;
 split:=app.split_ante_asset(winners,balances,(chosen_line->>'quantity')::numeric,(chosen_line->>'numerator')::numeric,(chosen_line->>'denominator')::numeric,p_seed,(chosen_line->>'stakeId')::uuid,(chosen_line->>'assetId')::uuid);balances:=split->'balances';
 FOR share IN SELECT * FROM jsonb_array_elements(split->'allocations') LOOP
 SELECT controller_kind='human' INTO STRICT human FROM app.match_seat WHERE id=(share->>'recipientSeatId')::uuid;
 allocations:=allocations||jsonb_build_array(jsonb_build_object('sourceStakeId',chosen_line->>'stakeId','sourceAssetId',chosen_line->>'assetId','recipientSeatId',share->>'recipientSeatId','quantity',share->>'quantity','classification',CASE WHEN chosen_line->>'funding'='ai_loss_allowance' THEN CASE WHEN human THEN 'ai_issue' ELSE 'ai_unissued' END ELSE CASE WHEN human THEN 'transfer' ELSE 'retire' END END));
 END LOOP;
 END LOOP;
 RETURN jsonb_build_object('version','ante-settlement-v1','matchResultId',r.id,'termsHash',c.terms_hash,'kind',CASE WHEN r.outcome='win' THEN 'settled' ELSE 'refunded' END,'seed',p_seed,'allocations',allocations);
END $$;
REVOKE ALL ON FUNCTION app.build_ante_allocation(uuid,uuid,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
