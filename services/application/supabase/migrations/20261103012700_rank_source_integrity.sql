BEGIN;
ALTER TABLE app.season ADD CONSTRAINT rank_cutoff_tuple CHECK((ranks_finalized_at IS NULL)=(result_cutoff_manifest IS NULL));
CREATE FUNCTION app.check_numeric_rank_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE s app.season; source app.match_result; expected_win boolean;
BEGIN
 IF TG_TABLE_NAME='competitive_result' AND NEW.season_id IS NULL THEN RETURN NULL; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='numeric rank source requires read committed retry'; END IF;
 SELECT * INTO s FROM app.season WHERE id=NEW.season_id FOR SHARE;
 IF s.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='rank source season missing'; END IF;
 IF TG_TABLE_NAME='rank' THEN
 IF NEW.tier IS DISTINCT FROM app.rank_tier(NEW.rating,s.ranking_policy) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank tier does not match the pinned numeric policy'; END IF;
 IF TG_OP='INSERT' AND s.ranks_finalized_at IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='finalized season cannot acquire a new rank'; END IF;
 ELSE
 SELECT * INTO STRICT source FROM app.match_result WHERE id=NEW.match_result_id FOR SHARE;
 SELECT source.outcome='win' AND EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=source.match_id AND seat.account_id=NEW.account_id AND seat.id=ANY(source.winner_seat_ids)) INTO expected_win;
 IF source.winner_seat_ids IS NULL OR NEW.completed_at<s.start_at OR NEW.completed_at>=s.end_at OR NEW.win IS DISTINCT FROM expected_win
 OR NEW.policy_ref<>'elo-v1' OR NEW.rating_evidence->>'version' IS DISTINCT FROM 'elo-v1'
 OR NEW.rating_evidence->'policy' IS DISTINCT FROM s.ranking_policy OR NEW.rating_evidence->>'sourceResultId' IS DISTINCT FROM source.id::text THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='numeric credit requires its pinned season, winner and rating evidence'; END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER numeric_rank_policy AFTER INSERT OR UPDATE ON app.rank DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_numeric_rank_source();
CREATE CONSTRAINT TRIGGER numeric_credit_source AFTER INSERT ON app.competitive_result DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_numeric_rank_source();

CREATE FUNCTION app.check_rank_finalization_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
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
 OR NOT EXISTS(SELECT 1 FROM app.ledger_operation o WHERE o.source_scope='rank_season_v1' AND o.source_ref=row_rank.id::text AND o.actor_account_id=row_rank.account_id AND o.state='committed') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='season rank requires finalized participation and once-only reward disposition'; END IF;
 SELECT (item->>'reward')::numeric INTO STRICT expected_reward FROM jsonb_array_elements(s.ranking_policy->'tiers') item WHERE item->>'tier'=row_rank.tier;
 SELECT coalesce(sum(p.quantity),0) INTO credits FROM app.ledger_operation o JOIN app.ledger_posting p ON p.operation_id=o.id JOIN app.inventory_asset inventory ON inventory.id=p.inventory_asset_id JOIN app.asset_owner owner ON owner.id=inventory.owner_id JOIN app.asset_definition asset ON asset.id=inventory.asset_definition_id
 WHERE o.source_scope='rank_season_v1' AND o.source_ref=row_rank.id::text AND p.direction='credit' AND owner.account_id=row_rank.account_id AND asset.currency_key='coin';
 IF credits<>expected_reward THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='final seasonal coin reward differs from the pinned tier'; END IF;
 END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER seasonal_rank_finalization AFTER INSERT OR UPDATE ON app.season DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_rank_finalization_source();
REVOKE ALL ON FUNCTION app.check_numeric_rank_source(),app.check_rank_finalization_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
