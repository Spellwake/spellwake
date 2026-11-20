BEGIN;
-- Deleted players retain rank history, but later season awards never create
-- spendable retired inventory. Preserve the exact unissued amount and cutoff.
CREATE FUNCTION app.retired_rank_reward_source(p_rank uuid,p_cutoff text,p_season_receipt uuid) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('version','retired-rank-reward-v1','rankId',r.id,'accountId',r.account_id,'seasonId',r.season_id,'tier',r.tier,'quantity',policy_tier.value->>'reward','cutoffRef',p_cutoff,'seasonReceiptId',p_season_receipt,'retirementOperationId',a.retirement_operation_id)
 FROM app.rank r JOIN app.season s ON s.id=r.season_id JOIN app.account a ON a.id=r.account_id JOIN app.asset_owner o ON o.account_id=a.id CROSS JOIN LATERAL jsonb_array_elements(s.ranking_policy->'tiers') AS policy_tier(value)
 WHERE r.id=p_rank AND policy_tier.value->>'tier'=r.tier AND a.state='deleted' AND o.state='retired' AND CASE WHEN a.state='deleted' AND o.state='retired' THEN (app.read_account_retirement_proof(a.id,a.retirement_operation_id)).id IS NOT NULL ELSE false END
$$;
CREATE FUNCTION app.check_retired_rank_reward(p_rank uuid,p_cutoff text) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.rank rank JOIN app.command_receipt season ON season.principal_key='rank-season:'||rank.season_id::text AND season.scope='rank_season_v1' AND season.request_ref=rank.season_id::text JOIN app.command_receipt reward ON reward.principal_key=season.principal_key AND reward.scope='rank_season_retired_v1' AND reward.request_ref=rank.id::text
 WHERE rank.id=p_rank AND season.state='accepted' AND season.target_ref=rank.season_id::text AND season.response_payload->'data'->>'cutoffRef'=p_cutoff AND reward.state='accepted' AND reward.target_ref=rank.id::text AND reward.operation_id IS NULL
 AND reward.normalized_source IS NOT DISTINCT FROM app.retired_rank_reward_source(rank.id,p_cutoff,season.id) AND reward.normalized_source IS NOT NULL AND reward.payload_hash=app.auth_json_hash(reward.normalized_source)
 AND reward.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',rank.id,'disposition','retired without issuance')))
$$;
CREATE OR REPLACE FUNCTION app.finalize_rank_season(p_season uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE season_row app.season; receipt app.command_receipt; manifest jsonb; format_row record; rank_row app.rank;
 reward numeric; owner uuid; coin uuid; inventory uuid; operation uuid; rank_count bigint:=0; manifest_hash text; retired_source jsonb; retired_receipt uuid;
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
 retired_source:=app.retired_rank_reward_source(rank_row.id,'sha256:'||manifest_hash,receipt.id);
 IF retired_source IS NOT NULL THEN
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES(receipt.principal_key,'rank_season_retired_v1',rank_row.id::text,rank_row.id::text,app.auth_json_hash(retired_source),'pending','',retired_source) RETURNING id INTO retired_receipt;
 PERFORM app.finish_account_command(retired_receipt,true,jsonb_build_object('resourceId',rank_row.id,'disposition','retired without issuance'));
 ELSE
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=rank_row.account_id AND state='active';
 PERFORM app.lock_asset_account_for_write(owner);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('rank_season',rank_row.id::text,'rank_season_v1',rank_row.account_id,clock_timestamp(),'pending') RETURNING id INTO operation;
 IF reward>0 THEN
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';inventory:=app.ensure_inventory(owner,coin);
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES(operation,inventory,'credit',reward,'rank_season_v1',rank_row.id::text);
 END IF;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 END IF;
 UPDATE app.rank SET finalized=true,revision=revision+1 WHERE id=rank_row.id;
 rank_count:=rank_count+1;
 END LOOP;
 UPDATE app.season SET result_cutoff_manifest=manifest,result_cutoff_ref='sha256:'||manifest_hash,ranks_finalized_at=clock_timestamp() WHERE id=p_season;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_season,'cutoffRef','sha256:'||manifest_hash,'ranks',rank_count::text));
END $$;
CREATE OR REPLACE FUNCTION app.check_rank_finalization_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE s app.season; expected_manifest jsonb; row_rank app.rank; expected_reward numeric; credits numeric;
BEGIN
 SELECT * INTO STRICT s FROM app.season WHERE id=NEW.id;
 IF s.ranks_finalized_at IS NULL THEN RETURN NULL; END IF;
 IF s.end_at>s.ranks_finalized_at OR s.ranks_finalized_at>clock_timestamp() OR s.result_cutoff_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(s.result_cutoff_manifest)
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.principal_key='rank-season:'||s.id::text AND r.scope='rank_season_v1' AND r.target_ref=s.id::text AND r.state='accepted' AND r.response_payload->'data'->>'cutoffRef'=s.result_cutoff_ref) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='season finalization requires its accepted cutoff receipt'; END IF;
 SELECT jsonb_build_object('version','rank-cutoff-v1','seasonId',s.id,'formats',coalesce(jsonb_agg(jsonb_build_object('formatId',format_id,'lastSequence',sequence::text,'resultIds',ids,'count',count_value::text) ORDER BY format_id),'[]'::jsonb)) INTO expected_manifest
 FROM(SELECT ranking_format_id format_id,max(rank_sequence) sequence,jsonb_agg(id ORDER BY rank_sequence) ids,count(*) count_value FROM app.match_result WHERE ranking_format_id IS NOT NULL AND committed_at>=s.start_at AND committed_at<s.end_at GROUP BY ranking_format_id) grouped;
 IF s.result_cutoff_manifest IS DISTINCT FROM expected_manifest OR EXISTS(SELECT 1 FROM app.match_result r JOIN app.format f ON f.id=r.ranking_format_id WHERE r.committed_at>=s.start_at AND r.committed_at<s.end_at AND r.rank_sequence>f.last_ranked_sequence) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='final rank manifest must contain every drained accepted source'; END IF;
 FOR row_rank IN SELECT * FROM app.rank WHERE season_id=s.id LOOP
 IF NOT row_rank.finalized OR NOT EXISTS(SELECT 1 FROM app.competitive_result c WHERE c.account_id=row_rank.account_id AND c.format_id=row_rank.format_id AND c.season_id=s.id AND c.rating_before IS NOT NULL)
 OR (NOT EXISTS(SELECT 1 FROM app.ledger_operation o WHERE o.source_scope='rank_season_v1' AND o.source_ref=row_rank.id::text AND o.actor_account_id=row_rank.account_id AND o.state='committed') AND NOT app.check_retired_rank_reward(row_rank.id,s.result_cutoff_ref)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='season rank requires finalized participation and once-only reward disposition'; END IF;
 SELECT (item->>'reward')::numeric INTO STRICT expected_reward FROM jsonb_array_elements(s.ranking_policy->'tiers') item WHERE item->>'tier'=row_rank.tier;
 SELECT coalesce(sum(p.quantity),0) INTO credits FROM app.ledger_operation o JOIN app.ledger_posting p ON p.operation_id=o.id JOIN app.inventory_asset inventory ON inventory.id=p.inventory_asset_id JOIN app.asset_owner owner ON owner.id=inventory.owner_id JOIN app.asset_definition asset ON asset.id=inventory.asset_definition_id
 WHERE o.source_scope='rank_season_v1' AND o.source_ref=row_rank.id::text AND p.direction='credit' AND owner.account_id=row_rank.account_id AND asset.currency_key='coin';
 IF app.check_retired_rank_reward(row_rank.id,s.result_cutoff_ref) THEN
 IF credits<>0 OR EXISTS(SELECT 1 FROM app.ledger_operation WHERE source_scope='rank_season_v1' AND source_ref=row_rank.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retired season award cannot also issue coins';END IF;
 ELSIF credits<>expected_reward THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='final seasonal coin reward differs from the pinned tier'; END IF;
 END LOOP;
 RETURN NULL;
END $$;
CREATE FUNCTION app.guard_retired_rank_issuance() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE r app.rank; s app.season;
BEGIN
 IF NEW.source_scope<>'rank_season_v1' THEN RETURN NULL;END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season reward source requires read committed retry';END IF;
 SELECT * INTO r FROM app.rank WHERE id::text=NEW.source_ref;
 IF r.id IS NULL THEN RETURN NULL;END IF;
 SELECT * INTO s FROM app.season WHERE id=r.season_id FOR SHARE;
 IF s.ranks_finalized_at IS NOT NULL AND app.check_retired_rank_reward(r.id,s.result_cutoff_ref) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retired seasonal reward cannot acquire an issuance operation';END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER retired_rank_issuance AFTER INSERT OR UPDATE ON app.ledger_operation DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_retired_rank_issuance();

REVOKE ALL ON FUNCTION app.guard_retired_rank_issuance(),app.retired_rank_reward_source(uuid,text,uuid),app.check_retired_rank_reward(uuid,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
