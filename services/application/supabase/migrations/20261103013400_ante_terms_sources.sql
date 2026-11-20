BEGIN;
ALTER TABLE app.ai_loss_exposure ADD COLUMN loss_coins numeric(78,0) CHECK(loss_coins IS NULL OR (loss_coins>=0 AND loss_coins<=reserved_coins AND loss_coins::text ~ '^(0|[1-9][0-9]{0,77})$'));
ALTER TABLE app.ante_commitment ADD CONSTRAINT ante_lifecycle_state CHECK(state IN ('offered','committed','settled','refunded','canceled'));
ALTER TABLE app.ante_commitment ADD COLUMN terms jsonb;
ALTER TABLE app.ante_commitment ADD COLUMN terms_hash text CHECK(terms_hash IS NULL OR terms_hash ~ '^[0-9a-f]{64}$');
ALTER TABLE app.ante_commitment ADD COLUMN accepted_seat_ids uuid[] NOT NULL DEFAULT '{}';
ALTER TABLE app.ante_commitment ADD COLUMN accepted_at timestamptz CHECK(accepted_at IS NULL OR isfinite(accepted_at));
ALTER TABLE app.ante_commitment ADD COLUMN start_day_utc text CHECK(start_day_utc IS NULL OR app.valid_utc_day(start_day_utc));
ALTER TABLE app.ante_commitment ADD COLUMN valuation_snapshot jsonb;
ALTER TABLE app.ante_commitment ADD COLUMN settlement_result_id uuid REFERENCES app.match_result(id) ON DELETE RESTRICT;
ALTER TABLE app.ante_commitment ADD COLUMN settlement jsonb;
ALTER TABLE app.ante_stake ADD COLUMN value_numerator numeric;
ALTER TABLE app.ante_stake ADD COLUMN value_denominator numeric;
ALTER TABLE app.ante_stake ADD COLUMN bundle_assessment jsonb;
ALTER TABLE app.ante_stake ADD CONSTRAINT ante_exact_value_tuple CHECK(
 (value_numerator IS NULL AND value_denominator IS NULL AND bundle_assessment IS NULL)
 OR (value_numerator IS NOT NULL AND value_numerator>0 AND value_numerator<'Infinity'::numeric AND scale(value_numerator)=0 AND length(value_numerator::text)<=8192
 AND value_denominator IS NOT NULL AND value_denominator>0 AND value_denominator<'Infinity'::numeric AND scale(value_denominator)=0 AND length(value_denominator::text)<=8192
 AND bundle_assessment IS NOT NULL AND jsonb_typeof(bundle_assessment)='object'));
ALTER TABLE app.ante_commitment ADD CONSTRAINT ante_terms_tuple CHECK((terms IS NULL AND terms_hash IS NULL) OR (terms IS NOT NULL AND jsonb_typeof(terms)='object' AND terms_hash IS NOT NULL));

CREATE FUNCTION app.ante_terms_manifest(p_commitment uuid) RETURNS jsonb
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('version','ante-terms-v1','matchId',c.match_id,'termsRevision',c.terms_revision::text,'stakes',coalesce((SELECT jsonb_agg(jsonb_build_object('seatId',stake.match_seat_id,'fundingKind',stake.funding_kind,'holdId',stake.hold_id,'exposureId',stake.ai_exposure_id,'aiCoins',CASE WHEN stake.funding_kind='ai_loss_allowance' THEN stake.estimated_value_coins::text END,'entries',coalesce((SELECT jsonb_agg(jsonb_build_object('assetId',inventory.asset_definition_id,'quantity',line.quantity::text) ORDER BY inventory.asset_definition_id) FROM app.hold_line line JOIN app.inventory_asset inventory ON inventory.id=line.inventory_asset_id WHERE line.hold_id=stake.hold_id),'[]'::jsonb)) ORDER BY seat.seat_index) FROM app.ante_stake stake JOIN app.match_seat seat ON seat.id=stake.match_seat_id WHERE stake.commitment_id=c.id),'[]'::jsonb)) FROM app.ante_commitment c WHERE c.id=p_commitment
$$;
CREATE FUNCTION app.guard_ante_source_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.ante_commitment;
BEGIN
 IF TG_TABLE_NAME='ante_commitment' THEN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante source history cannot be deleted'; END IF;
 IF OLD.terms IS NULL THEN RETURN NEW; END IF;
 IF (NEW.id,NEW.match_id) IS DISTINCT FROM (OLD.id,OLD.match_id) OR NEW.terms_revision<OLD.terms_revision
 OR (OLD.state<>'offered' AND (NEW.terms,NEW.terms_hash,NEW.terms_revision,NEW.accepted_seat_ids,NEW.accepted_at,NEW.start_day_utc,NEW.valuation_snapshot,NEW.reservation_operation_id) IS DISTINCT FROM (OLD.terms,OLD.terms_hash,OLD.terms_revision,OLD.accepted_seat_ids,OLD.accepted_at,OLD.start_day_utc,OLD.valuation_snapshot,OLD.reservation_operation_id))
 OR (OLD.state IN ('settled','refunded','canceled') AND NEW IS DISTINCT FROM OLD)
 OR (OLD.state='committed' AND NEW.state NOT IN ('committed','settled','refunded')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted ante terms and terminal sources are immutable'; END IF;
 ELSE
 IF TG_OP='UPDATE' AND (NEW.id,NEW.commitment_id,NEW.match_seat_id,NEW.funding_kind) IS DISTINCT FROM (OLD.id,OLD.commitment_id,OLD.match_seat_id,OLD.funding_kind) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante stake parent, seat and funding identity are immutable'; END IF;
 IF TG_OP='INSERT' THEN SELECT * INTO parent FROM app.ante_commitment WHERE id=NEW.commitment_id FOR SHARE;
 ELSE SELECT * INTO parent FROM app.ante_commitment WHERE id=OLD.commitment_id FOR SHARE; END IF;
 IF parent.terms IS NOT NULL AND parent.state<>'offered' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed ante stake cannot change'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER ante_source_history BEFORE UPDATE OR DELETE ON app.ante_commitment FOR EACH ROW EXECUTE FUNCTION app.guard_ante_source_history();
CREATE TRIGGER ante_stake_history BEFORE INSERT OR UPDATE OR DELETE ON app.ante_stake FOR EACH ROW EXECUTE FUNCTION app.guard_ante_source_history();
CREATE FUNCTION app.guard_ante_hold_lines() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM app.ante_stake stake JOIN app.ante_commitment c ON c.id=stake.commitment_id WHERE stake.hold_id=NEW.hold_id AND c.terms IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='replace an offered ante bundle rather than editing its held lines'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_line_ante_history BEFORE INSERT OR UPDATE ON app.hold_line FOR EACH ROW EXECUTE FUNCTION app.guard_ante_hold_lines();
REVOKE ALL ON FUNCTION app.ante_terms_manifest(uuid),app.guard_ante_source_history(),app.guard_ante_hold_lines() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
