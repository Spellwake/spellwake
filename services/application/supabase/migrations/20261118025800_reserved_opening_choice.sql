BEGIN;

CREATE TABLE app.native_opening_selection (
 game_id uuid PRIMARY KEY REFERENCES app.game(id), context_id uuid NOT NULL UNIQUE DEFAULT gen_random_uuid(),
 seed_a uuid NOT NULL DEFAULT gen_random_uuid(), seed_b uuid NOT NULL DEFAULT gen_random_uuid(),
 chooser_id uuid NOT NULL REFERENCES app.match_seat(id), first_id uuid REFERENCES app.match_seat(id),
 rolls smallint[], CHECK(rolls IS NULL OR cardinality(rolls)=2)
);
ALTER TABLE app.native_opening_selection ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.native_opening_selection FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- An old opening may record the first player but omit who chose. Both original
-- human participants must agree on the missing chooser before a draw can carry it.
CREATE TABLE app.native_legacy_chooser_report (
 game_id uuid NOT NULL REFERENCES app.game(id), reporter uuid NOT NULL REFERENCES app.match_seat(id),
 chooser uuid NOT NULL REFERENCES app.match_seat(id), PRIMARY KEY(game_id,reporter)
);
ALTER TABLE app.native_legacy_chooser_report ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.native_legacy_chooser_report FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Rejection sampling from an unrestricted UUID octet gives each d20 face equal probability.
CREATE FUNCTION app.native_opening_d20() RETURNS smallint LANGUAGE plpgsql VOLATILE SET search_path='' AS $$
DECLARE b integer;
BEGIN LOOP
 b:=get_byte(uuid_send(gen_random_uuid()),0);
 IF b<240 THEN RETURN (1+b%20)::smallint; END IF;
END LOOP; END $$;

CREATE FUNCTION app.ensure_native_opening_selection(p_game uuid) RETURNS app.native_opening_selection
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; choice app.native_opening_selection; seats uuid[]; rolls smallint[]; chooser uuid; prior app.game_result;
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.game g0 ON g0.match_id=m0.id WHERE g0.id=p_game FOR NO KEY UPDATE OF m0;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF g.id IS NULL OR g.state<>'reserved' OR m.state<>'ready' OR m.availability<>'available'
  OR m.reserved_game_id IS DISTINCT FROM g.id THEN RETURN NULL; END IF;
 SELECT * INTO choice FROM app.native_opening_selection WHERE game_id=p_game;
 IF FOUND THEN RETURN choice; END IF;
 SELECT array_agg(id ORDER BY seat_index) INTO seats FROM app.match_seat WHERE match_id=m.id;
 IF cardinality(seats) IS DISTINCT FROM 2 OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=m.id AND controller_kind<>'human')
 THEN RETURN NULL; END IF;
 IF g.game_index=0 THEN
  LOOP rolls:=ARRAY[app.native_opening_d20(),app.native_opening_d20()]; EXIT WHEN rolls[1]<>rolls[2]; END LOOP;
  chooser:=CASE WHEN rolls[1]>rolls[2] THEN seats[1] ELSE seats[2] END;
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
 INSERT INTO app.native_opening_selection(game_id,chooser_id,rolls) VALUES(p_game,chooser,rolls) RETURNING * INTO choice;
 RETURN choice;
END $$;

CREATE FUNCTION app.native_opening_player_choice(p_subject uuid,p_session uuid,p_game uuid,p_play_first boolean,p_prior_chooser uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; own_seat uuid; other_seat uuid; choice app.native_opening_selection; selected uuid; prior_game uuid;
 failed constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}';
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 SELECT s.id INTO own_seat FROM app.match_seat s JOIN app.game g ON g.match_id=s.match_id
 WHERE g.id=p_game AND s.account_id=player.id AND s.controller_kind='human' AND player.state='active';
 IF own_seat IS NULL THEN RETURN failed; END IF;
 choice:=app.ensure_native_opening_selection(p_game);
 IF choice.game_id IS NULL THEN
  SELECT previous.id INTO prior_game FROM app.game current_game
   JOIN app.match m ON m.id=current_game.match_id AND m.reserved_game_id=current_game.id
   JOIN app.game previous ON previous.match_id=m.id AND previous.game_index=current_game.game_index-1
   JOIN app.game_result r ON r.game_id=previous.id AND r.outcome='draw'
   WHERE current_game.id=p_game AND current_game.state='reserved' AND m.state='ready' AND m.availability='available'
   AND (SELECT count(*) FROM app.match_seat s WHERE s.match_id=m.id)=2
   AND NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.controller_kind<>'human')
   AND NOT EXISTS(SELECT 1 FROM app.native_opening_selection old WHERE old.game_id=previous.id);
  IF prior_game IS NOT NULL THEN
   IF p_prior_chooser IS NOT NULL THEN
    IF NOT EXISTS(SELECT 1 FROM app.match_seat s JOIN app.game g ON g.match_id=s.match_id WHERE g.id=p_game AND s.id=p_prior_chooser)
    THEN RETURN failed; END IF;
    INSERT INTO app.native_legacy_chooser_report(game_id,reporter,chooser) VALUES(prior_game,own_seat,p_prior_chooser)
      ON CONFLICT(game_id,reporter) DO UPDATE SET chooser=excluded.chooser;
    choice:=app.ensure_native_opening_selection(p_game);
   END IF;
   IF choice.game_id IS NULL THEN RETURN '{"status":"fail","data":{"choice":"confirm previous chooser"}}'; END IF;
  END IF;
 END IF;
 IF p_prior_chooser IS NOT NULL AND choice.chooser_id IS NOT NULL AND choice.chooser_id<>p_prior_chooser THEN
  RETURN '{"status":"fail","data":{"choice":"already selected"}}';
 END IF;
 IF choice.game_id IS NULL THEN
  -- An accepted selection remains observable on a retry after the game opens.
  SELECT * INTO choice FROM app.native_opening_selection WHERE game_id=p_game AND first_id IS NOT NULL;
 END IF;
 IF choice.game_id IS NULL THEN RETURN failed; END IF;
 IF p_play_first IS NOT NULL THEN
  IF own_seat<>choice.chooser_id THEN RETURN failed; END IF;
  SELECT s.id INTO STRICT other_seat FROM app.match_seat s JOIN app.game g ON g.match_id=s.match_id WHERE g.id=p_game AND s.id<>own_seat;
  selected:=CASE WHEN p_play_first THEN own_seat ELSE other_seat END;
  IF choice.first_id IS NOT NULL AND choice.first_id<>selected THEN
   RETURN '{"status":"fail","data":{"choice":"already selected"}}';
  END IF;
  IF choice.first_id IS NULL THEN
   UPDATE app.native_opening_selection SET first_id=selected WHERE game_id=p_game AND first_id IS NULL RETURNING * INTO STRICT choice;
  END IF;
 END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('gameId',p_game,'chooserSeatId',choice.chooser_id,
  'firstSeatId',choice.first_id,'yourChoice',choice.chooser_id=own_seat,'rolls',choice.rolls));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN '{"status":"fail","data":{"session":"unauthorized"}}';
END $$;

CREATE FUNCTION app.read_native_opening_material(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE choice app.native_opening_selection;
BEGIN
 choice:=app.ensure_native_opening_selection(p_game);
 IF choice.first_id IS NULL THEN RETURN '{"ready":false}'; END IF;
 RETURN jsonb_build_object('ready',true,'contextId',choice.context_id,'firstSeatId',choice.first_id,'seedA',choice.seed_a,'seedB',choice.seed_b);
END $$;
REVOKE ALL ON FUNCTION app.native_opening_d20(),app.ensure_native_opening_selection(uuid),
 app.native_opening_player_choice(uuid,uuid,uuid,boolean,uuid),app.read_native_opening_material(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.native_opening_player_choice(uuid,uuid,uuid,boolean,uuid),app.read_native_opening_material(uuid) TO service_role;
COMMIT;
