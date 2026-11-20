BEGIN;

-- SQL CHECK permits unknown: require the conditional values explicitly, not just comparisons.
ALTER TABLE app.price_observation ADD CONSTRAINT valid_price_is_present
 CHECK (state<>'valid' OR price_usd_decimal IS NOT NULL);
ALTER TABLE app.match_result ADD CONSTRAINT ranking_sequence_group
 CHECK ((ranking_format_id IS NULL) = (rank_sequence IS NULL));

CREATE FUNCTION app.guard_accepted_deck_snapshot() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF OLD.deck_accepted_at IS NOT NULL AND
  (NEW.source_revision_id,NEW.deck_snapshot_ref,NEW.deck_accepted_at)
   IS DISTINCT FROM (OLD.source_revision_id,OLD.deck_snapshot_ref,OLD.deck_accepted_at) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted deck input is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER accepted_deck_snapshot BEFORE UPDATE ON app.match_seat
 FOR EACH ROW EXECUTE FUNCTION app.guard_accepted_deck_snapshot();
CREATE TRIGGER game_context_immutable BEFORE UPDATE OR DELETE ON app.game_context
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER match_result_immutable BEFORE UPDATE OR DELETE ON app.match_result
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER price_observation_immutable BEFORE UPDATE OR DELETE ON app.price_observation
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

REVOKE EXECUTE ON FUNCTION app.guard_accepted_deck_snapshot() FROM PUBLIC,anon,authenticated;

COMMIT;
