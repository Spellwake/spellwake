BEGIN;
CREATE FUNCTION app.finalize_rank_season(p_season uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE season_row app.season; receipt app.command_receipt; manifest jsonb; format_row record; rank_row app.rank;
 reward numeric; owner uuid; coin uuid; inventory uuid; operation uuid; rank_count bigint:=0; manifest_hash text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='rank finalization requires read committed retry'; END IF;
 IF p_season IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='season source required'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('rank-season:'||p_season::text,0));
 SELECT * INTO season_row FROM app.season WHERE id=p_season;
 IF season_row.id IS NULL OR season_row.end_at>clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank season has not ended'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('rank-season:'||p_season::text,'rank_season_v1',p_season::text,p_season::text,app.auth_json_hash(jsonb_build_object('seasonId',p_season)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='rank-season:'||p_season::text AND scope='rank_season_v1' AND request_ref=p_season::text FOR UPDATE;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season finalization pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 -- Consumers take the same season advisory fence before their account locks.
 -- Account-first order also agrees with existing game rewards/retirement.
 PERFORM a.id FROM app.account a WHERE EXISTS(SELECT 1 FROM app.rank r WHERE r.account_id=a.id AND r.season_id=p_season) ORDER BY a.id FOR NO KEY UPDATE;
 -- Terminal writers own format then season rows through commit. Lock formats
 -- before the season row, so a pre-cutoff writer cannot be missed or deadlock
 -- behind a finalizer holding the season while waiting for its format.
 PERFORM id FROM app.format ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO STRICT season_row FROM app.season WHERE id=p_season FOR NO KEY UPDATE;
 IF season_row.ranks_finalized_at IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='finalized season is missing its source receipt'; END IF;
 IF EXISTS(SELECT 1 FROM app.match_result r JOIN app.format f ON f.id=r.ranking_format_id WHERE r.committed_at>=season_row.start_at AND r.committed_at<season_row.end_at AND r.rank_sequence>f.last_ranked_sequence) THEN
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season cutoff cannot finalize before every accepted format result drains'; END IF;
 SELECT jsonb_build_object('version','rank-cutoff-v1','seasonId',p_season,'formats',coalesce(jsonb_agg(jsonb_build_object('formatId',format_id,'lastSequence',sequence::text,'resultIds',ids,'count',count_value::text) ORDER BY format_id),'[]'::jsonb)) INTO manifest
 FROM(SELECT ranking_format_id format_id,max(rank_sequence) sequence,jsonb_agg(id ORDER BY rank_sequence) ids,count(*) count_value FROM app.match_result WHERE ranking_format_id IS NOT NULL AND committed_at>=season_row.start_at AND committed_at<season_row.end_at GROUP BY ranking_format_id) grouped;
 manifest_hash:=app.auth_json_hash(manifest);
 FOR rank_row IN SELECT * FROM app.rank WHERE season_id=p_season ORDER BY account_id,format_id FOR NO KEY UPDATE LOOP
 IF NOT EXISTS(SELECT 1 FROM app.competitive_result c WHERE c.account_id=rank_row.account_id AND c.format_id=rank_row.format_id AND c.season_id=p_season AND c.rating_before IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank reward requires actual consumed season participation'; END IF;
 SELECT (item->>'reward')::numeric INTO STRICT reward FROM jsonb_array_elements(season_row.ranking_policy->'tiers') item WHERE item->>'tier'=rank_row.tier;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=rank_row.account_id AND state='active';
 PERFORM app.lock_asset_account_for_write(owner);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('rank_season',rank_row.id::text,'rank_season_v1',rank_row.account_id,clock_timestamp(),'pending') RETURNING id INTO operation;
 IF reward>0 THEN
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';inventory:=app.ensure_inventory(owner,coin);
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES(operation,inventory,'credit',reward,'rank_season_v1',rank_row.id::text);
 END IF;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.rank SET finalized=true,revision=revision+1 WHERE id=rank_row.id;
 rank_count:=rank_count+1;
 END LOOP;
 UPDATE app.season SET result_cutoff_manifest=manifest,result_cutoff_ref='sha256:'||manifest_hash,ranks_finalized_at=clock_timestamp() WHERE id=p_season;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_season,'cutoffRef','sha256:'||manifest_hash,'ranks',rank_count::text));
END $$;
REVOKE ALL ON FUNCTION app.finalize_rank_season(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
