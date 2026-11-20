BEGIN;

-- A reserved game gives its known human chooser two minutes. AI choices still
-- use their controller, and an unresolved legacy draw still needs agreement.
ALTER TABLE app.native_opening_selection
 ADD COLUMN choice_expires_at timestamptz,
 ADD COLUMN choice_paused_at timestamptz,
 ADD COLUMN choice_timed_out_at timestamptz,
 ADD CHECK (choice_paused_at IS NULL OR (choice_expires_at IS NOT NULL AND first_id IS NULL)),
 ADD CHECK (choice_timed_out_at IS NULL OR (
   choice_expires_at IS NOT NULL AND choice_timed_out_at>=choice_expires_at
   AND first_id IS NOT NULL AND first_id=chooser_id
 ));

-- Both the scheduler and the signed choice use this writer under match then
-- game locks. A request arriving after expiry cannot replace the default.
CREATE OR REPLACE FUNCTION app.ensure_native_opening_selection(p_game uuid) RETURNS app.native_opening_selection
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; choice app.native_opening_selection; seats uuid[]; rolls smallint[]; chooser uuid; prior app.game_result; contenders uuid[]; round_rolls smallint[]; rounds jsonb:='[]'; observed_at timestamptz;
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.game g0 ON g0.match_id=m0.id WHERE g0.id=p_game FOR NO KEY UPDATE OF m0;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF g.id IS NULL OR g.state<>'reserved' OR m.state<>'ready' OR m.availability<>'available'
  OR m.reserved_game_id IS DISTINCT FROM g.id THEN RETURN NULL; END IF;
 SELECT * INTO choice FROM app.native_opening_selection WHERE game_id=p_game;
 IF FOUND THEN
  IF choice.first_id IS NULL AND choice.choice_paused_at IS NULL THEN
   observed_at:=clock_timestamp();
   IF choice.choice_expires_at<=observed_at THEN
    UPDATE app.native_opening_selection
     SET first_id=chooser_id,choice_timed_out_at=observed_at
     WHERE game_id=p_game RETURNING * INTO STRICT choice;
   ELSIF choice.choice_expires_at IS NULL AND EXISTS (
    SELECT 1 FROM app.match_seat WHERE id=choice.chooser_id AND controller_kind='human'
   ) THEN
    UPDATE app.native_opening_selection SET choice_expires_at=observed_at+interval '120 seconds'
     WHERE game_id=p_game RETURNING * INTO STRICT choice;
   END IF;
  END IF;
  RETURN choice;
 END IF;
 SELECT array_agg(id ORDER BY seat_index) INTO seats FROM app.match_seat WHERE match_id=m.id;
 IF cardinality(seats) NOT BETWEEN 2 AND 6 OR seats IS NULL
    OR (cardinality(seats)>2 AND m.structure<>'bo1') THEN RETURN NULL; END IF;
 IF g.game_index=0 THEN
  contenders:=seats;
  LOOP
   SELECT array_agg(app.native_opening_d20() ORDER BY idx) INTO round_rolls
    FROM unnest(contenders) WITH ORDINALITY AS seat(id,idx);
   rounds:=rounds || jsonb_build_array(to_jsonb(round_rolls));
   IF rolls IS NULL OR cardinality(seats)=2 THEN rolls:=round_rolls; END IF;
   SELECT array_agg(id ORDER BY idx) INTO contenders
    FROM unnest(contenders,round_rolls) WITH ORDINALITY AS result(id,face,idx)
    WHERE face=(SELECT max(value) FROM unnest(round_rolls) value);
   EXIT WHEN cardinality(contenders)=1;
  END LOOP;
  chooser:=contenders[1];
 ELSE
  SELECT r.* INTO prior FROM app.game prev JOIN app.game_result r ON r.game_id=prev.id
   WHERE prev.match_id=m.id AND prev.game_index=g.game_index-1;
  IF prior.outcome='win' AND cardinality(prior.winner_seat_ids)=1 THEN
   SELECT s INTO chooser FROM unnest(seats) s WHERE s<>ALL(prior.winner_seat_ids);
  ELSIF prior.outcome='draw' THEN
   SELECT chooser_id INTO chooser FROM app.native_opening_selection WHERE game_id=prior.game_id;
   IF chooser IS NULL THEN
    SELECT report.chooser INTO chooser FROM app.native_legacy_chooser_report report WHERE report.game_id=prior.game_id
    AND report.reporter=ANY(seats) AND report.chooser=ANY(seats) GROUP BY report.chooser HAVING count(*)=2;
   END IF;
  END IF;
  IF chooser IS NULL OR NOT chooser=ANY(seats) THEN RETURN NULL; END IF;
 END IF;
 INSERT INTO app.native_opening_selection(game_id,chooser_id,rolls,roll_rounds,choice_expires_at)
  SELECT p_game,chooser,rolls,rounds,
    CASE WHEN controller_kind='human' THEN clock_timestamp()+interval '120 seconds' END
  FROM app.match_seat WHERE id=chooser RETURNING * INTO STRICT choice;
 RETURN choice;
END $$;

-- Stop the pregame clock during an accepted operational interruption. This
-- trigger runs while the caller holds the match lock.
CREATE FUNCTION app.track_reserved_opening_availability() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE observed_at timestamptz;
BEGIN
 IF OLD.availability='available' THEN
  SELECT (normalized_source->>'incidentAt')::timestamptz INTO STRICT observed_at
   FROM app.command_receipt WHERE id=NEW.incident_receipt_id;
  UPDATE app.native_opening_selection SET choice_paused_at=observed_at
   WHERE game_id=NEW.reserved_game_id AND first_id IS NULL AND choice_expires_at IS NOT NULL
   AND choice_paused_at IS NULL;
 ELSIF NEW.availability='available' THEN
  SELECT (normalized_source->>'resumedAt')::timestamptz INTO STRICT observed_at
   FROM app.command_receipt WHERE id=NEW.restoration_receipt_id;
  UPDATE app.native_opening_selection
   SET choice_expires_at=observed_at+greatest(interval '0',choice_expires_at-choice_paused_at),
       choice_paused_at=NULL
   WHERE game_id=NEW.reserved_game_id AND choice_paused_at IS NOT NULL;
 END IF;
 RETURN NULL;
END $$;

CREATE TRIGGER track_reserved_opening_availability
AFTER UPDATE OF availability ON app.match
FOR EACH ROW WHEN (
 NEW.state='ready' AND NEW.reserved_game_id IS NOT NULL AND OLD.availability IS DISTINCT FROM NEW.availability
) EXECUTE FUNCTION app.track_reserved_opening_availability();

-- Initialize the choice without a participant poll and finalize expired
-- choices before the worker wake predicate checks for an accepted first seat.
CREATE FUNCTION app.dispatch_native_opening_choices(p_limit integer) RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE game_id uuid; choice app.native_opening_selection; processed integer:=0;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid opening choice dispatch limit';
 END IF;
 FOR game_id IN
  SELECT g.id FROM app.match m
  JOIN app.game g ON g.id=m.reserved_game_id AND g.match_id=m.id
  LEFT JOIN app.native_opening_selection s ON s.game_id=g.id
  LEFT JOIN app.match_seat chooser ON chooser.id=s.chooser_id
  LEFT JOIN app.game previous ON previous.match_id=m.id AND previous.game_index=g.game_index-1
  LEFT JOIN app.game_result prior ON prior.game_id=previous.id
  WHERE m.state='ready' AND m.availability='available' AND g.state='reserved'
   AND EXISTS (
    SELECT 1 FROM app.durable_handoff h WHERE h.game_id=g.id AND h.match_id=m.id
     AND h.state='pending' AND h.destination='game_host' AND h.kind IN ('ready_match','reserved_game')
   )
   AND (
    (s.game_id IS NULL AND (g.game_index=0 OR prior.outcome='win' OR (
     prior.outcome='draw' AND (
      EXISTS (SELECT 1 FROM app.native_opening_selection previous_choice WHERE previous_choice.game_id=previous.id)
      OR EXISTS (
       SELECT 1 FROM app.native_legacy_chooser_report report
        JOIN app.match_seat reporter ON reporter.id=report.reporter AND reporter.match_id=m.id
        JOIN app.match_seat reported ON reported.id=report.chooser AND reported.match_id=m.id
        WHERE report.game_id=previous.id GROUP BY report.chooser HAVING count(*)=2
      )
     )
    ))) OR (
     s.first_id IS NULL AND s.choice_paused_at IS NULL
     AND (s.choice_expires_at<=statement_timestamp() OR (s.choice_expires_at IS NULL AND chooser.controller_kind='human'))
    )
   )
  ORDER BY s.choice_expires_at NULLS LAST,m.started_at,m.id
  LIMIT p_limit FOR NO KEY UPDATE OF m SKIP LOCKED
 LOOP
  choice:=app.ensure_native_opening_selection(game_id);
  IF choice.game_id IS NOT NULL THEN processed:=processed+1; END IF;
 END LOOP;
 RETURN processed;
END $$;

REVOKE ALL ON FUNCTION app.track_reserved_opening_availability(),app.dispatch_native_opening_choices(integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
