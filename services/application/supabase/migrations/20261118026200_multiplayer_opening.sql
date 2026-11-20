BEGIN;
ALTER TABLE app.native_opening_selection DROP CONSTRAINT native_opening_selection_rolls_check,
 ADD CHECK (rolls IS NULL OR cardinality(rolls) BETWEEN 2 AND 6),
 ADD COLUMN roll_rounds jsonb NOT NULL DEFAULT '[]'::jsonb;
CREATE OR REPLACE FUNCTION app.ensure_native_opening_selection(p_game uuid) RETURNS app.native_opening_selection
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; choice app.native_opening_selection; seats uuid[]; rolls smallint[]; chooser uuid; prior app.game_result; contenders uuid[]; round_rolls smallint[]; rounds jsonb:='[]';
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.game g0 ON g0.match_id=m0.id WHERE g0.id=p_game FOR NO KEY UPDATE OF m0;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF g.id IS NULL OR g.state<>'reserved' OR m.state<>'ready' OR m.availability<>'available'
  OR m.reserved_game_id IS DISTINCT FROM g.id THEN RETURN NULL; END IF;
 SELECT * INTO choice FROM app.native_opening_selection WHERE game_id=p_game;
 IF FOUND THEN RETURN choice; END IF;
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
 INSERT INTO app.native_opening_selection(game_id,chooser_id,rolls,roll_rounds) VALUES(p_game,chooser,rolls,rounds) RETURNING * INTO choice;
 RETURN choice;
END $$;

CREATE FUNCTION app.native_opening_seat_choice(own_seat uuid,p_game uuid,p_play_first boolean,p_prior_chooser uuid,p_first_seat uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE choice app.native_opening_selection; selected uuid; prior_game uuid; seats uuid[];
 failed constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}';
BEGIN
 SELECT array_agg(s.id ORDER BY s.seat_index) INTO seats FROM app.match_seat s JOIN app.game g ON g.match_id=s.match_id WHERE g.id=p_game;
 IF own_seat IS NULL OR seats IS NULL OR NOT own_seat=ANY(seats) THEN RETURN failed; END IF;
 IF num_nonnulls(p_play_first,p_prior_chooser,p_first_seat)>1 OR
    (p_first_seat IS NOT NULL AND NOT p_first_seat=ANY(seats)) OR
    (p_play_first IS FALSE AND cardinality(seats)<>2) THEN
  RETURN '{"status":"fail","data":{"choice":"invalid selection"}}';
 END IF;
 choice:=app.ensure_native_opening_selection(p_game);
 IF choice.game_id IS NULL THEN
  SELECT previous.id INTO prior_game FROM app.game current_game
   JOIN app.match m ON m.id=current_game.match_id AND m.reserved_game_id=current_game.id
   JOIN app.game previous ON previous.match_id=m.id AND previous.game_index=current_game.game_index-1
   JOIN app.game_result r ON r.game_id=previous.id AND r.outcome='draw'
   WHERE current_game.id=p_game AND current_game.state='reserved' AND m.state='ready' AND m.availability='available'
   AND (SELECT count(*) FROM app.match_seat s WHERE s.match_id=m.id)=2
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
 IF p_play_first IS NOT NULL OR p_first_seat IS NOT NULL THEN
  IF own_seat<>choice.chooser_id THEN RETURN failed; END IF;
  selected:=coalesce(p_first_seat,CASE WHEN p_play_first THEN own_seat WHEN seats[1]=own_seat THEN seats[2] ELSE seats[1] END);
  IF choice.first_id IS NOT NULL AND choice.first_id<>selected THEN
   RETURN '{"status":"fail","data":{"choice":"already selected"}}';
  END IF;
  IF choice.first_id IS NULL THEN
   UPDATE app.native_opening_selection SET first_id=selected WHERE game_id=p_game AND first_id IS NULL RETURNING * INTO STRICT choice;
  END IF;
 END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('gameId',p_game,'chooserSeatId',choice.chooser_id,
  'firstSeatId',choice.first_id,'yourChoice',choice.chooser_id=own_seat,'rolls',choice.rolls) ||
  CASE WHEN cardinality(seats)>2 THEN jsonb_build_object('seatIds',seats,'rollRounds',choice.roll_rounds) ELSE '{}'::jsonb END);
END $$;
CREATE FUNCTION app.native_opening_human_choice(p_subject uuid,p_session uuid,p_game uuid,p_play_first boolean,p_prior_chooser uuid,p_first_seat uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; own_seat uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 SELECT s.id INTO own_seat FROM app.match_seat s JOIN app.game g ON g.match_id=s.match_id
  WHERE g.id=p_game AND s.account_id=player.id AND s.controller_kind='human' AND app.native_account_may_continue(player,s.match_id);
 RETURN app.native_opening_seat_choice(own_seat,p_game,p_play_first,p_prior_chooser,p_first_seat);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN '{"status":"fail","data":{"session":"unauthorized"}}';
END $$;

CREATE OR REPLACE FUNCTION app.native_opening_player_choice(p_subject uuid,p_session uuid,p_game uuid,p_play_first boolean,p_prior_chooser uuid)
RETURNS jsonb LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$
 SELECT app.native_opening_human_choice(p_subject,p_session,p_game,p_play_first,p_prior_chooser,NULL)
$$;

CREATE FUNCTION app.native_opening_controller_choice(p_grant uuid,p_epoch numeric,p_secret_ref text,p_game uuid,p_play_first boolean,p_prior_chooser uuid,p_first_seat uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actor jsonb; match_key uuid;
BEGIN
 SELECT match_id INTO match_key FROM app.game WHERE id=p_game;
 actor:=app.lock_native_controller_actor(p_grant,p_epoch,p_secret_ref,match_key);
 RETURN app.native_opening_seat_choice((actor->>'seatId')::uuid,p_game,p_play_first,p_prior_chooser,p_first_seat);
END $$;
CREATE OR REPLACE FUNCTION app.read_native_match_delivery_page(p_owner text,p_lane text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_owner IS NULL OR btrim(p_owner)='' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery page'; END IF;
 IF p_lane='handoff' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT h.id,jsonb_build_object('id',h.id,'kind',h.kind,'gameId',h.game_id) body FROM app.durable_handoff h
   WHERE h.state='pending' AND h.kind IN ('ready_match','reserved_game','game_outcome','match_outcome','between_deadline')
    AND (p_after IS NULL OR h.id>p_after) ORDER BY h.id LIMIT p_limit+1) page;
 ELSIF p_lane='between' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT d.id,jsonb_build_object('id',d.id,'matchRevision',m.revision::text,'profileId',m.profile_id,'releaseId',m.accepted_release_id,
    'releaseProof',r.evidence_ref,'selectedRevisionId',coalesce(previous.selected_deck_revision_id,seat.source_revision_id),
    'deadlineRevision',d.deadline_revision::text,'requestId',coalesce(d.deadline_source_receipt_id,d.id)) body
   FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat seat ON seat.id=d.seat_id
    JOIN app.qualified_release r ON r.id=m.accepted_release_id JOIN app.game g ON g.id=d.after_game_id
    LEFT JOIN app.game prev ON prev.match_id=m.id AND prev.game_index=g.game_index-1
    LEFT JOIN app.between_game_decision previous ON previous.after_game_id=prev.id AND previous.seat_id=d.seat_id AND previous.state='answered'
   WHERE (p_after IS NULL OR d.id>p_after) AND d.state='pending' AND m.state='between_games' AND m.availability='available'
    AND (d.deadline_policy_ref IS NULL OR (d.deadline_policy_ref='native-between-keep-deck-v1' AND d.deadline_at<=statement_timestamp()))
   ORDER BY d.id LIMIT p_limit+1) page;
 ELSIF p_lane='clock' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT g.id,jsonb_build_object('id',g.id,'requestId',c.id,'basis',jsonb_build_object('gameId',g.id,'contextId',c.context_id,
    'epoch',o.normalized_source->>'nativeEpoch','revision',g.accepted_revision::text)) body
   FROM app.game g JOIN app.match m ON m.id=g.match_id AND m.current_game_id=g.id JOIN app.operational_state op ON op.game_id=g.id
    JOIN app.game_checkpoint c ON c.game_id=g.id AND c.accepted_revision=g.accepted_revision
    JOIN app.command_receipt o ON o.principal_key='native-opening:'||g.id::text AND o.scope='native_opening_v1' AND o.state='accepted'
   WHERE op.owner_instance_ref=p_owner AND op.availability='available' AND g.state='active' AND m.state='active' AND m.availability='available'
    AND (p_after IS NULL OR g.id>p_after) ORDER BY g.id LIMIT p_limit+1) page;
 ELSE RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery lane'; END IF;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;
REVOKE ALL ON FUNCTION app.native_opening_seat_choice(uuid,uuid,boolean,uuid,uuid),
 app.native_opening_human_choice(uuid,uuid,uuid,boolean,uuid,uuid),
 app.native_opening_controller_choice(uuid,numeric,text,uuid,boolean,uuid,uuid) FROM PUBLIC,anon,authenticated,supabase_auth_admin,service_role;
GRANT EXECUTE ON FUNCTION app.native_opening_human_choice(uuid,uuid,uuid,boolean,uuid,uuid),
 app.native_opening_controller_choice(uuid,numeric,text,uuid,boolean,uuid,uuid) TO service_role;
COMMIT;
