BEGIN;
ALTER TABLE app.match DROP CONSTRAINT match_availability_check;
ALTER TABLE app.match ADD CONSTRAINT match_availability_check CHECK(availability IN ('available','recovering','unrecoverable'));
ALTER TABLE app.match ADD COLUMN terminal_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.match ADD CONSTRAINT match_operational_terminal_tuple CHECK((availability='unrecoverable')=(terminal_receipt_id IS NOT NULL));
ALTER TABLE app.operational_state ADD COLUMN terminal_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.operational_state ADD CONSTRAINT game_operational_terminal_tuple CHECK((availability='unrecoverable')=(terminal_receipt_id IS NOT NULL));
ALTER TABLE app.match_result ADD COLUMN terminal_kind text NOT NULL DEFAULT 'game_outcome' CHECK(terminal_kind IN ('game_outcome','unrecoverable_incident'));
ALTER TABLE app.match_result ADD COLUMN terminal_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.match_result ADD CONSTRAINT match_result_terminal_tuple CHECK((terminal_kind='unrecoverable_incident')=(terminal_receipt_id IS NOT NULL));
ALTER TABLE app.between_game_decision ADD COLUMN terminal_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.between_game_decision ADD CONSTRAINT decision_terminal_tuple CHECK((state='canceled')=(terminal_receipt_id IS NOT NULL));
-- Ordinary outcomes still require their actual normalized game result. The new
-- operational kind has a separately checked terminal receipt and real played history.
CREATE OR REPLACE FUNCTION app.check_normalized_outcome() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; source app.game_result; ids uuid[];
BEGIN
 IF TG_TABLE_NAME='game_result' THEN
 IF NEW.winner_seat_ids IS NULL THEN RETURN NULL;END IF;
 SELECT match_id INTO parent FROM app.game WHERE id=NEW.game_id FOR SHARE;
 ELSE
 parent:=NEW.match_id;
 IF NEW.terminal_kind='unrecoverable_incident' THEN
 IF NEW.outcome<>'no_contest' OR NEW.winner_seat_ids IS NULL OR cardinality(NEW.winner_seat_ids)<>0 OR NEW.source_game_result_id IS NOT NULL OR NEW.final_game_ids IS NULL OR array_ndims(NEW.final_game_ids)>1 OR array_position(NEW.final_game_ids,NULL) IS NOT NULL
 OR cardinality(NEW.final_game_ids)<>(SELECT count(DISTINCT value) FROM unnest(NEW.final_game_ids) value)
 OR EXISTS(SELECT 1 FROM unnest(NEW.final_game_ids) value WHERE NOT EXISTS(SELECT 1 FROM app.game g WHERE g.id=value AND g.match_id=parent AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id)))
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.id=NEW.terminal_receipt_id AND r.scope='match_unrecoverable_v1' AND r.target_ref=parent::text AND r.principal_key='match-recovery:'||parent::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational no-contest requires actual played history and terminal source';END IF;
 PERFORM app.validate_normalized_winners(parent,NEW.outcome,NEW.winner_seat_ids);RETURN NULL;
 END IF;
 IF NEW.winner_seat_ids IS NULL AND NEW.final_game_ids IS NULL AND NEW.source_game_result_id IS NULL THEN RETURN NULL;END IF;
 IF NEW.final_game_ids IS NULL OR cardinality(NEW.final_game_ids)=0 OR array_ndims(NEW.final_game_ids)>1 OR array_position(NEW.final_game_ids,NULL) IS NOT NULL OR cardinality(NEW.final_game_ids)<>(SELECT count(DISTINCT x) FROM unnest(NEW.final_game_ids) x)
 OR EXISTS(SELECT 1 FROM unnest(NEW.final_game_ids) sources(game_id) WHERE NOT EXISTS(SELECT 1 FROM app.game g JOIN app.game_result r ON r.game_id=g.id WHERE g.id=sources.game_id AND g.match_id=parent AND r.winner_seat_ids IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal match requires its distinct completed normalized source games';END IF;
 SELECT * INTO source FROM app.game_result WHERE id=NEW.source_game_result_id FOR SHARE;
 IF source.id IS NULL OR NOT(source.game_id=ANY(NEW.final_game_ids)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal match source must belong to its final game manifest';END IF;
 END IF;
 PERFORM app.validate_normalized_winners(parent,NEW.outcome,NEW.winner_seat_ids);RETURN NULL;
END $$;
CREATE OR REPLACE FUNCTION app.guard_between_game_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
 IF NEW.after_game_id IS NOT NULL AND (NEW.state<>'pending' OR NEW.reply_ref IS NOT NULL OR NEW.selected_deck_revision_id IS NOT NULL OR NEW.terminal_receipt_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new normalized decision must be unanswered';END IF;RETURN NEW;
 END IF;
 IF OLD.after_game_id IS NULL THEN IF TG_OP='DELETE' THEN RETURN OLD;END IF;RETURN NEW;END IF;
 IF TG_OP='DELETE' OR OLD.state<>'pending' OR (NEW.id,NEW.match_id,NEW.seat_id,NEW.decision_kind,NEW.basis_revision,NEW.after_game_id,NEW.clock_revision) IS DISTINCT FROM (OLD.id,OLD.match_id,OLD.seat_id,OLD.decision_kind,OLD.basis_revision,OLD.after_game_id,OLD.clock_revision)
 OR NEW.state NOT IN ('pending','answered','expired','canceled') OR (NEW.state='answered' AND (NEW.selected_deck_revision_id IS NULL OR NEW.reply_ref IS NULL))
 OR (NEW.state<>'answered' AND (NEW.selected_deck_revision_id IS NOT NULL OR NEW.reply_ref IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game source and completed decisions are immutable';END IF;RETURN NEW;
END $$;
COMMIT;
