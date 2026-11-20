BEGIN;
CREATE FUNCTION app.replace_supply_ballot_command(p_subject uuid,p_session uuid,p_request uuid,p_season uuid,p_sets uuid[]) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_season IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_season) OR p_sets IS NULL OR array_ndims(p_sets)>1 OR cardinality(p_sets)>10 OR array_position(p_sets,NULL) IS NOT NULL OR '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_sets) OR cardinality(p_sets)<>(SELECT count(DISTINCT id) FROM unnest(p_sets) id) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.replace_supply_ballot(player.id,p_season,p_sets,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_supply_ballot_command(p_subject uuid,p_session uuid,p_request uuid,p_season uuid,p_sets uuid[]) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_season IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_season) OR p_sets IS NULL OR array_ndims(p_sets)>1 OR cardinality(p_sets)>10 OR array_position(p_sets,NULL) IS NOT NULL OR '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_sets) OR cardinality(p_sets)<>(SELECT count(DISTINCT id) FROM unnest(p_sets) id) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='supply_ballot_v1' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_season::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('season',p_season,'sets',p_sets))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'supply_ballot_v1',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
-- Frozen candidate pages and the caller's ballot. Live tallies and private
-- population membership, hashes, random draws and source proofs stay closed.
CREATE FUNCTION app.read_supply_ballot(p_season uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;season_row app.season;opening app.command_receipt;final app.command_receipt;items jsonb;cursor_id uuid;approved uuid[];cutoff timestamptz;open_now boolean;qualified boolean;active boolean;publication jsonb;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_season IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_season,p_after) OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT * INTO season_row FROM app.season WHERE id=p_season;
 SELECT * INTO opening FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_open_v1' AND state='accepted';
 IF season_row.id IS NULL OR opening.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('ballot','unavailable'));END IF;
 SELECT * INTO final FROM app.command_receipt WHERE principal_key='supply-season:'||p_season::text AND scope='supply_ballot_result_v1' AND state='accepted';
 cutoff:=season_row.start_at-interval '168 hours';open_now:=statement_timestamp()<cutoff;
 SELECT state='active' INTO active FROM app.account WHERE id=actor;
 qualified:=EXISTS(SELECT 1 FROM app.game_participation WHERE account_id=actor AND real_game AND own_turns>=3 AND completed_at<=statement_timestamp());
 SELECT coalesce(array_agg(set_id ORDER BY set_id),'{}'::uuid[]) INTO approved FROM app.set_ballot WHERE account_id=actor AND season_id=p_season;
 IF final.id IS NOT NULL THEN
  publication:=jsonb_build_object('population',(SELECT jsonb_agg(jsonb_build_object('month',value->'month','accounts',value->'accounts') ORDER BY ordinal) FROM jsonb_array_elements(final.normalized_source->'population') WITH ORDINALITY x(value,ordinal)),'populationSum',final.normalized_source->'populationSum','coefficient',final.normalized_source->'coefficient','quantity',final.normalized_source->'quantity');
 END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT s.id,jsonb_build_object('setId',s.id,'name',s.name,'approved',s.id=ANY(approved),'finalApprovals',CASE WHEN final.id IS NOT NULL THEN(SELECT t->'approvals' FROM jsonb_array_elements(final.normalized_source->'tallies') t WHERE t->>'setId'=s.id::text) END,'winner',CASE WHEN final.id IS NOT NULL THEN EXISTS(SELECT 1 FROM jsonb_array_elements(final.normalized_source->'winners') w WHERE w->>'setId'=s.id::text) END,'dropQuantity',CASE WHEN final.id IS NOT NULL THEN(SELECT quantity::text FROM app.supply_drop WHERE season_id=p_season AND set_id=s.id) END) body
 FROM jsonb_array_elements(opening.normalized_source->'candidates') candidate JOIN app.card_set s ON s.id=(candidate->>'setId')::uuid WHERE p_after IS NULL OR s.id>p_after ORDER BY s.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'setId')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'seasonId',p_season,'cutoff',cutoff,'openedAt',opening.normalized_source->'openedAt','isOpen',open_now,'canVote',open_now AND active AND qualified,'qualifies',qualified,'approvedSetIds',approved,'publication',publication,'items',items,'nextCursor',cursor_id));
END $$;
REVOKE ALL ON FUNCTION app.replace_supply_ballot_command(uuid,uuid,uuid,uuid,uuid[]),app.read_supply_ballot_command(uuid,uuid,uuid,uuid,uuid[]),app.read_supply_ballot(uuid,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.replace_supply_ballot_command(uuid,uuid,uuid,uuid,uuid[]),app.read_supply_ballot_command(uuid,uuid,uuid,uuid,uuid[]) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_supply_ballot(uuid,uuid,integer) TO authenticated;
COMMIT;
