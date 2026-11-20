BEGIN;
CREATE FUNCTION app.rank_expected_score(p_own numeric,p_other numeric,p_divisor numeric) RETURNS numeric
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT CASE WHEN (p_other-p_own)/p_divisor>=25 THEN 0::numeric WHEN (p_other-p_own)/p_divisor<=-25 THEN 1::numeric
 ELSE 1::numeric/(1+power(10::numeric,(p_other-p_own)/p_divisor)) END
$$;
CREATE FUNCTION app.consume_ranked_match_result(p_result uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.match_result; m app.match; source_season app.season; prior_season app.season; receipt app.command_receipt;
 actors uuid[]; actor uuid; policy jsonb; rank_row app.rank; old_rank app.rank; next_sequence numeric; changes jsonb;
 item jsonb; before_value numeric; delta numeric; after_value numeric; credit_id uuid; reward jsonb; initial_value numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ranked result consumption requires read committed retry'; END IF;
 SELECT * INTO source FROM app.match_result WHERE id=p_result;
 SELECT * INTO m FROM app.match WHERE id=source.match_id;
 IF source.id IS NULL OR NOT m.competitive OR m.mode<>'collection' OR source.winner_seat_ids IS NULL OR source.ranking_format_id IS DISTINCT FROM m.format_id THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='normalized competitive match outcome required'; END IF;
 SELECT * INTO source_season FROM app.season WHERE source.committed_at>=start_at AND source.committed_at<end_at;
 IF source_season.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank source season missing'; END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('rank-season:'||source_season.id::text,0));
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=m.id AND controller_kind='human';
 IF actors IS NULL OR cardinality(actors)<2 OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=m.id AND controller_kind<>'human') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ranked match requires human opponents'; END IF;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('ranked-format:'||m.format_id::text,'ranked_match_v1',source.id::text,source.id::text,app.auth_json_hash(jsonb_build_object('resultId',source.id)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ranked-format:'||m.format_id::text AND scope='ranked_match_v1' AND request_ref=source.id::text FOR UPDATE;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='rank consumption pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT last_ranked_sequence+1 INTO STRICT next_sequence FROM app.format WHERE id=m.format_id FOR NO KEY UPDATE;
 IF source.rank_sequence<>next_sequence THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ranked result must wait for the preceding format sequence'; END IF;
 SELECT * INTO source_season FROM app.season WHERE source.committed_at>=start_at AND source.committed_at<end_at FOR SHARE;
 IF source_season.id IS NULL OR source_season.ranks_finalized_at IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank source season unavailable or already finalized'; END IF;
 policy:=source_season.ranking_policy;
 SELECT * INTO prior_season FROM app.season WHERE end_at=source_season.start_at;
 FOREACH actor IN ARRAY actors LOOP
 SELECT * INTO rank_row FROM app.rank WHERE account_id=actor AND format_id=m.format_id AND season_id=source_season.id FOR NO KEY UPDATE;
 IF rank_row.id IS NULL THEN
 initial_value:=(policy->>'initial')::numeric;
 IF prior_season.id IS NOT NULL THEN
 SELECT * INTO old_rank FROM app.rank WHERE account_id=actor AND format_id=m.format_id AND season_id=prior_season.id FOR SHARE;
 IF old_rank.id IS NOT NULL THEN
 IF NOT old_rank.finalized OR prior_season.ranks_finalized_at IS NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='previous rank season must finalize before reset'; END IF;
 initial_value:=greatest(0,initial_value+floor((old_rank.rating-initial_value)*(policy->>'resetNumerator')::numeric/(policy->>'resetDenominator')::numeric));
 END IF;
 END IF;
 INSERT INTO app.rank(account_id,format_id,season_id,tier,rating_ref,revision,finalized,rating)
 VALUES(actor,m.format_id,source_season.id,app.rank_tier(initial_value,policy),'initial:'||source_season.id::text,0,false,initial_value);
 END IF;
 END LOOP;
 -- Side means and every delta use the same locked pre-update values. Numeric
 -- PostgreSQL power/division and round define v1; score tails beyond25 divisor
 -- units saturate, far below the half-point final integer rounding threshold.
 WITH players AS (
 SELECT s.id seat_id,s.account_id,r.id rank_id,r.rating,
 jsonb_build_array(s.team_ref,CASE WHEN s.team_ref IS NULL THEN s.id END)::text side_key,
 s.id=ANY(source.winner_seat_ids) won FROM app.match_seat s JOIN app.rank r ON r.account_id=s.account_id AND r.format_id=m.format_id AND r.season_id=source_season.id WHERE s.match_id=m.id),
 sides AS(SELECT side_key,avg(rating) rating,bool_or(won) won FROM players GROUP BY side_key),
 deltas AS(SELECT a.side_key,CASE WHEN source.outcome='no_contest' THEN 0 ELSE round((policy->>'k')::numeric*avg(
 CASE WHEN source.outcome='draw' OR a.won=b.won THEN 0.5::numeric WHEN a.won THEN 1::numeric ELSE 0::numeric END-app.rank_expected_score(a.rating,b.rating,(policy->>'divisor')::numeric))) END delta FROM sides a JOIN sides b ON b.side_key<>a.side_key GROUP BY a.side_key)
 SELECT jsonb_agg(jsonb_build_object('accountId',p.account_id,'rankId',p.rank_id,'before',p.rating::text,'delta',d.delta::text,'side',p.side_key) ORDER BY p.account_id) INTO changes FROM players p JOIN deltas d ON d.side_key=p.side_key;
 IF changes IS NULL OR jsonb_array_length(changes)<>cardinality(actors) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ranked match requires distinct opposing sides'; END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(changes) LOOP
 actor:=(item->>'accountId')::uuid;before_value:=(item->>'before')::numeric;delta:=(item->>'delta')::numeric;after_value:=greatest(0,before_value+delta);
 UPDATE app.rank SET rating=after_value,tier=app.rank_tier(after_value,policy),rating_ref='ranked-result:'||source.id::text,revision=revision+1 WHERE id=(item->>'rankId')::uuid AND rating=before_value AND NOT finalized;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='locked rank source changed'; END IF;
 INSERT INTO app.competitive_result(account_id,match_result_id,source_ref,format_id,win,completed_at,policy_ref,season_id,rating_before,rating_delta,rating_after,rating_evidence)
 VALUES(actor,source.id,'match-result:'||source.id::text,m.format_id,source.outcome='win' AND EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.account_id=actor AND s.id=ANY(source.winner_seat_ids)),source.committed_at,'elo-v1',source_season.id,before_value,delta,after_value,jsonb_build_object('version','elo-v1','policy',policy,'lockedPlayers',changes,'sourceResultId',source.id)) RETURNING id INTO credit_id;
 reward:=app.consume_competitive_win_v1(credit_id);
 IF reward->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive credit reward failed'; END IF;
 END LOOP;
 UPDATE app.format SET last_ranked_sequence=source.rank_sequence WHERE id=m.format_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',source.id,'seasonId',source_season.id,'sequence',source.rank_sequence::text,'players',cardinality(actors)::text));
END $$;
REVOKE ALL ON FUNCTION app.rank_expected_score(numeric,numeric,numeric),app.consume_ranked_match_result(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
