BEGIN;

-- Scalar references establish existence; composite references also establish shared scope.
-- These targets remain nondeferrable, as required for PostgreSQL referenced unique keys.
ALTER TABLE app.game ADD CONSTRAINT game_match_scope UNIQUE (id,match_id);
ALTER TABLE app.match_seat ADD CONSTRAINT seat_match_scope UNIQUE (id,match_id);
ALTER TABLE app.game_context ADD CONSTRAINT context_game_scope UNIQUE (id,game_id);
ALTER TABLE app.game_event ADD CONSTRAINT event_game_scope UNIQUE (id,game_id);

ALTER TABLE app.between_game_decision ADD CONSTRAINT decision_seat_match_scope
 FOREIGN KEY (seat_id,match_id) REFERENCES app.match_seat(id,match_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.game_checkpoint ADD CONSTRAINT checkpoint_context_game_scope
 FOREIGN KEY (context_id,game_id) REFERENCES app.game_context(id,game_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.game_event ADD CONSTRAINT event_context_game_scope
 FOREIGN KEY (context_id,game_id) REFERENCES app.game_context(id,game_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.game_result ADD CONSTRAINT result_event_game_scope
 FOREIGN KEY (source_event_id,game_id) REFERENCES app.game_event(id,game_id)
 DEFERRABLE INITIALLY IMMEDIATE;
-- MATCH SIMPLE permits match-wide handoffs, whose game_id is intentionally null.
-- The original nonnullable match FK continues to require the match to exist.
ALTER TABLE app.durable_handoff ADD CONSTRAINT handoff_game_match_scope
 FOREIGN KEY (game_id,match_id) REFERENCES app.game(id,match_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.replay_manifest ADD CONSTRAINT replay_context_game_scope
 FOREIGN KEY (context_id,game_id) REFERENCES app.game_context(id,game_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.game_activity_fact ADD CONSTRAINT activity_event_game_scope
 FOREIGN KEY (source_event_id,game_id) REFERENCES app.game_event(id,game_id)
 DEFERRABLE INITIALLY IMMEDIATE;

COMMIT;
