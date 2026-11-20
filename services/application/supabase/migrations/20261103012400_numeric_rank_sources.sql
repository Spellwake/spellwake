BEGIN;
ALTER TABLE app.rank ADD COLUMN rating numeric(78,0) NOT NULL DEFAULT 1000 CHECK(rating>=0 AND rating::text ~ '^(0|[1-9][0-9]{0,77})$');
ALTER TABLE app.season ADD COLUMN ranking_policy jsonb NOT NULL DEFAULT '{"version":"elo-v1","initial":"1000","k":"32","divisor":"400","resetNumerator":"1","resetDenominator":"2","tiers":[{"tier":"bronze","minimum":"0","reward":"0"},{"tier":"silver","minimum":"1100","reward":"250"},{"tier":"gold","minimum":"1300","reward":"500"},{"tier":"platinum","minimum":"1500","reward":"750"},{"tier":"diamond","minimum":"1700","reward":"1000"},{"tier":"mythic","minimum":"1900","reward":"1500"}]}'::jsonb;
ALTER TABLE app.season ADD COLUMN result_cutoff_manifest jsonb;
ALTER TABLE app.season ADD COLUMN ranks_finalized_at timestamptz CHECK(ranks_finalized_at IS NULL OR isfinite(ranks_finalized_at));
ALTER TABLE app.competitive_result ADD COLUMN season_id uuid REFERENCES app.season(id) ON DELETE RESTRICT;
ALTER TABLE app.competitive_result ADD COLUMN rating_before numeric(78,0);
ALTER TABLE app.competitive_result ADD COLUMN rating_delta numeric(78,0);
ALTER TABLE app.competitive_result ADD COLUMN rating_after numeric(78,0);
ALTER TABLE app.competitive_result ADD COLUMN rating_evidence jsonb;
ALTER TABLE app.competitive_result ADD CONSTRAINT competitive_rating_tuple CHECK(
 (season_id IS NULL AND rating_before IS NULL AND rating_delta IS NULL AND rating_after IS NULL AND rating_evidence IS NULL)
 OR (season_id IS NOT NULL AND rating_before IS NOT NULL AND rating_before>=0 AND rating_before::text ~ '^(0|[1-9][0-9]{0,77})$'
 AND rating_delta IS NOT NULL AND rating_delta::text ~ '^-?(0|[1-9][0-9]{0,77})$'
 AND rating_after IS NOT NULL AND rating_after>=0 AND rating_after=greatest(0,rating_before+rating_delta)
 AND rating_evidence IS NOT NULL AND jsonb_typeof(rating_evidence)='object'));

CREATE FUNCTION app.validate_rank_policy(p_policy jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE item jsonb; last_min numeric:=-1; index_value integer:=0; names text[]:=ARRAY['bronze','silver','gold','platinum','diamond','mythic'];
BEGIN
 IF jsonb_typeof(p_policy) IS DISTINCT FROM 'object' OR p_policy->>'version' IS DISTINCT FROM 'elo-v1'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_policy) k WHERE k NOT IN ('version','initial','k','divisor','resetNumerator','resetDenominator','tiers'))
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['initial','k','divisor','resetNumerator','resetDenominator']) k WHERE jsonb_typeof(p_policy->k) IS DISTINCT FROM 'string' OR p_policy->>k !~ '^(0|[1-9][0-9]{0,8})$')
 OR jsonb_typeof(p_policy->'tiers') IS DISTINCT FROM 'array' THEN RETURN false; END IF;
 IF (p_policy->>'divisor')::numeric=0 OR (p_policy->>'resetDenominator')::numeric=0 OR (p_policy->>'k')::numeric=0 OR jsonb_array_length(p_policy->'tiers')<>6 THEN RETURN false; END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(p_policy->'tiers') LOOP
 index_value:=index_value+1;
 IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR item->>'tier' IS DISTINCT FROM names[index_value]
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('tier','minimum','reward'))
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['minimum','reward']) k WHERE jsonb_typeof(item->k) IS DISTINCT FROM 'string' OR item->>k !~ '^(0|[1-9][0-9]{0,8})$') THEN RETURN false; END IF;
 IF (item->>'minimum')::numeric<=last_min OR (index_value=1 AND (item->>'minimum')::numeric<>0) THEN RETURN false; END IF;
 last_min:=(item->>'minimum')::numeric;
 END LOOP;
 RETURN true;
END $$;
ALTER TABLE app.season ADD CONSTRAINT valid_ranking_policy CHECK(app.validate_rank_policy(ranking_policy));
CREATE FUNCTION app.rank_tier(p_rating numeric,p_policy jsonb) RETURNS text
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT item->>'tier' FROM jsonb_array_elements(p_policy->'tiers') item WHERE (item->>'minimum')::numeric<=p_rating ORDER BY (item->>'minimum')::numeric DESC LIMIT 1
$$;
CREATE FUNCTION app.guard_rank_source_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_TABLE_NAME='season' THEN
 IF (NEW.id,NEW.year,NEW.quarter,NEW.start_at,NEW.end_at) IS DISTINCT FROM (OLD.id,OLD.year,OLD.quarter,OLD.start_at,OLD.end_at) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='season calendar identity is immutable'; END IF;
 IF NEW.ranking_policy IS DISTINCT FROM OLD.ranking_policy THEN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='rank policy changes require read committed retry'; END IF;
 IF EXISTS(SELECT 1 FROM app.rank WHERE season_id=OLD.id) OR EXISTS(SELECT 1 FROM app.match_result WHERE ranking_format_id IS NOT NULL AND committed_at>=OLD.start_at AND committed_at<OLD.end_at) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='used seasonal rank policy is immutable'; END IF;
 END IF;
 IF OLD.ranks_finalized_at IS NOT NULL AND (NEW.result_cutoff_manifest,NEW.result_cutoff_ref,NEW.ranks_finalized_at,NEW.ranking_policy) IS DISTINCT FROM (OLD.result_cutoff_manifest,OLD.result_cutoff_ref,OLD.ranks_finalized_at,OLD.ranking_policy) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='finalized rank cutoff is immutable'; END IF;
 ELSE
 IF TG_OP='DELETE' OR OLD.finalized OR (NEW.id,NEW.account_id,NEW.format_id,NEW.season_id) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.format_id,OLD.season_id) OR NEW.revision<=OLD.revision THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='rank identity or finalized history is immutable'; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER seasonal_rank_policy BEFORE UPDATE ON app.season FOR EACH ROW EXECUTE FUNCTION app.guard_rank_source_history();
CREATE TRIGGER rank_source_history BEFORE UPDATE OR DELETE ON app.rank FOR EACH ROW EXECUTE FUNCTION app.guard_rank_source_history();
REVOKE ALL ON FUNCTION app.validate_rank_policy(jsonb),app.rank_tier(numeric,jsonb),app.guard_rank_source_history()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
