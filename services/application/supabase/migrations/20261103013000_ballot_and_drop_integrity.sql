BEGIN;
ALTER TABLE app.supply_drop ADD CONSTRAINT supported_quarterly_source_policy CHECK(policy_ref='quarterly-real-game-v1');
CREATE FUNCTION app.guard_ballot_history() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid; s app.season; opening app.command_receipt;
BEGIN
 IF TG_OP='UPDATE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='replace the complete ballot rather than mutate approval identity'; END IF;
 IF TG_OP='DELETE' THEN target:=OLD.season_id; ELSE target:=NEW.season_id; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ballot mutation requires read committed retry'; END IF;
 PERFORM id FROM app.account WHERE id=CASE WHEN TG_OP='DELETE' THEN OLD.account_id ELSE NEW.account_id END FOR NO KEY UPDATE;
 SELECT * INTO STRICT s FROM app.season WHERE id=target FOR NO KEY UPDATE;
 SELECT * INTO opening FROM app.command_receipt WHERE principal_key='supply-season:'||target::text AND scope='supply_ballot_open_v1' AND state='accepted';
 IF opening.id IS NULL OR clock_timestamp()>=s.start_at-interval '168 hours' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='frozen or unavailable ballot'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM app.game_participation WHERE account_id=NEW.account_id AND real_game AND own_turns>=3 AND completed_at<=clock_timestamp())
 OR NOT EXISTS(SELECT 1 FROM jsonb_array_elements(opening.normalized_source->'candidates') candidate WHERE candidate->>'setId'=NEW.set_id::text)
 OR (SELECT count(*) FROM app.set_ballot WHERE season_id=target AND account_id=NEW.account_id)>=10 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ballot requires qualified voter and at most ten frozen candidates'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supply_ballot_history BEFORE INSERT OR UPDATE OR DELETE ON app.set_ballot FOR EACH ROW EXECUTE FUNCTION app.guard_ballot_history();

CREATE FUNCTION app.guard_drop_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted quarterly source allocation is immutable';
END $$;
CREATE TRIGGER quarterly_drop_history BEFORE UPDATE OR DELETE ON app.supply_drop FOR EACH ROW EXECUTE FUNCTION app.guard_drop_history();
CREATE FUNCTION app.check_quarterly_drop_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; expected_share numeric; set_order bigint; total_quantity numeric;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='supply-season:'||NEW.season_id::text AND scope='supply_ballot_result_v1' AND state='accepted';
 IF receipt.id IS NULL OR receipt.normalized_source->>'version' IS DISTINCT FROM 'quarterly-supply-v1'
 OR NEW.population_snapshot_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(receipt.normalized_source) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quarterly allocation requires its accepted normalized source'; END IF;
 total_quantity:=(receipt.normalized_source->>'quantity')::numeric;
 SELECT ordinal-1 INTO set_order FROM(SELECT value,row_number() OVER(ORDER BY value->>'setId') ordinal FROM jsonb_array_elements(receipt.normalized_source->'winners')) selected WHERE value->>'setId'=NEW.set_id::text;
 IF set_order IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='drop set is outside the five accepted winners'; END IF;
 expected_share:=div(total_quantity,5)+CASE WHEN set_order<mod(total_quantity,5) THEN 1 ELSE 0 END;
 IF NEW.quantity<>expected_share OR (SELECT coalesce(sum(starting_quantity),0) FROM app.supply_stock WHERE drop_id=NEW.id AND origin='quarterly' AND allocation_ref=NEW.population_snapshot_ref)<>expected_share THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quarterly set quantity and product stock must conserve the accepted allocation'; END IF;
 IF (SELECT count(*) FROM app.supply_drop WHERE season_id=NEW.season_id AND policy_ref='quarterly-real-game-v1')<>5 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quarterly result requires exactly five set drops'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER quarterly_drop_source AFTER INSERT OR UPDATE ON app.supply_drop DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_quarterly_drop_source();
REVOKE ALL ON FUNCTION app.guard_ballot_history(),app.guard_drop_history(),app.check_quarterly_drop_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
