BEGIN;
CREATE OR REPLACE FUNCTION app.native_opening_player_choice(p_subject uuid,p_session uuid,p_game uuid,p_play_first boolean,p_prior_chooser uuid)
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
 IF choice.game_id IS NULL THEN
  -- An accepted selection remains observable on a retry after the game opens.
  SELECT * INTO choice FROM app.native_opening_selection WHERE game_id=p_game AND first_id IS NOT NULL;
 END IF;
 IF choice.game_id IS NULL THEN RETURN failed; END IF;
 IF p_prior_chooser IS NOT NULL AND choice.chooser_id IS NOT NULL AND choice.chooser_id<>p_prior_chooser THEN
  RETURN '{"status":"fail","data":{"choice":"already selected"}}';
 END IF;
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

CREATE TABLE app.native_entropy_seed (
 game_id uuid PRIMARY KEY REFERENCES app.game(id), seed_a uuid NOT NULL DEFAULT gen_random_uuid(),
 seed_b uuid NOT NULL DEFAULT gen_random_uuid()
);
ALTER TABLE app.native_entropy_seed ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.native_entropy_seed FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE FUNCTION app.reserve_native_entropy_seed(p_game uuid,p_checkpoint uuid,p_owner text,p_fence numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE seed app.native_entropy_seed; m uuid;
BEGIN
 SELECT match_id INTO m FROM app.game WHERE id=p_game;
 PERFORM id FROM app.match WHERE id=m AND current_game_id=p_game AND state='active' AND availability='available' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN '{"ready":false}'; END IF;
 PERFORM g.id FROM app.game g JOIN app.game_checkpoint cp ON cp.game_id=g.id AND cp.accepted_revision=g.accepted_revision
  WHERE g.id=p_game AND g.state='active' AND cp.id=p_checkpoint FOR NO KEY UPDATE OF g;
 IF NOT FOUND THEN RETURN '{"ready":false}'; END IF;
 PERFORM id FROM app.operational_state WHERE game_id=p_game AND owner_instance_ref=p_owner AND fence_epoch=p_fence
  AND availability='available' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN '{"ready":false}'; END IF;
 INSERT INTO app.native_entropy_seed(game_id) VALUES(p_game) ON CONFLICT DO NOTHING;
 SELECT * INTO STRICT seed FROM app.native_entropy_seed WHERE game_id=p_game;
 RETURN jsonb_build_object('ready',true,'seedA',seed.seed_a,'seedB',seed.seed_b);
END $$;
REVOKE ALL ON FUNCTION app.reserve_native_entropy_seed(uuid,uuid,text,numeric) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.reserve_native_entropy_seed(uuid,uuid,text,numeric) TO service_role;
CREATE OR REPLACE FUNCTION app.deliver_native_between_decision(p_decision uuid,p_basis numeric,p_deadline numeric,p_request uuid,
 p_seconds integer,p_selected uuid,p_hash text,p_reply text,p_qualification text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match; observed jsonb; response jsonb; due timestamptz; expected_deck uuid; format_key text;
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.between_game_decision d0 ON d0.match_id=m0.id WHERE d0.id=p_decision FOR NO KEY UPDATE OF m0;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision FOR NO KEY UPDATE;
 IF d.id IS NULL OR d.state<>'pending' OR m.state<>'between_games' OR m.availability<>'available' OR m.revision<>p_basis
  OR d.deadline_revision<>p_deadline OR p_request IS DISTINCT FROM coalesce(d.deadline_source_receipt_id,d.id)
  THEN RETURN '{"status":"waiting"}'; END IF;
 IF d.deadline_policy_ref IS NULL AND (p_seconds IS NULL OR p_seconds NOT BETWEEN 1 AND 86400) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid pinned sideboard duration'; END IF;
 SELECT coalesce(previous.selected_deck_revision_id,seat.source_revision_id),f.key INTO expected_deck,format_key
 FROM app.game g JOIN app.match_seat seat ON seat.match_id=g.match_id AND seat.id=d.seat_id
 JOIN app.format f ON f.id=m.format_id LEFT JOIN app.game prev ON prev.match_id=g.match_id AND prev.game_index=g.game_index-1
 LEFT JOIN app.between_game_decision previous ON previous.after_game_id=prev.id AND previous.seat_id=seat.id AND previous.state='answered'
 WHERE g.id=d.after_game_id;
 IF p_selected IS DISTINCT FROM expected_deck OR p_reply IS DISTINCT FROM 'native-between-reply-v1/'||p_hash
  OR p_qualification IS DISTINCT FROM 'native-constructed-between-v1/'||format_key||'/'||p_hash THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained-deck deadline selection'; END IF;
 observed:=app.read_native_between_source(p_decision,p_basis,p_selected);
 IF observed->>'status' IS DISTINCT FROM 'success' OR p_hash IS NULL OR observed->'data'->>'sourceHash' IS DISTINCT FROM p_hash
 THEN RETURN '{"status":"waiting"}'; END IF;
 IF d.deadline_policy_ref IS NULL THEN
  -- Start prospectively: an old unarmed decision does not inherit prior downtime.
  due:=clock_timestamp()+make_interval(secs=>p_seconds);
  response:=app.schedule_between_game_deadline(d.id,m.revision,d.deadline_revision,d.clock_revision,m.availability_revision,m.profile_id,
   'native-between-keep-deck-v1',due,p_qualification,p_request);
 ELSIF d.deadline_policy_ref='native-between-keep-deck-v1' AND d.deadline_at<=statement_timestamp() THEN
  response:=app.expire_between_game_deadline(d.id,d.deadline_revision,p_selected,p_reply,p_qualification,p_request);
 ELSE RETURN '{"status":"waiting"}'; END IF;
 IF response->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='decision delivery requires fresh source'; END IF;
 RETURN '{"status":"accepted"}';
END $$;
COMMIT;
