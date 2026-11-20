BEGIN;
-- A pinned profile supplies the duration. The default answer keeps the last played deck.
CREATE FUNCTION app.deliver_native_between_decision(p_decision uuid,p_basis numeric,p_deadline numeric,p_request uuid,
 p_seconds integer,p_selected uuid,p_hash text,p_reply text,p_qualification text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match; observed jsonb; response jsonb; due timestamptz; expected_deck uuid; format_key text;
BEGIN
 SELECT m0.* INTO m FROM app.match m0 JOIN app.between_game_decision d0 ON d0.match_id=m0.id WHERE d0.id=p_decision FOR NO KEY UPDATE OF m0;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision FOR NO KEY UPDATE;
 IF d.id IS NULL OR d.state<>'pending' OR m.state<>'between_games' OR m.availability<>'available' OR m.revision<>p_basis
  OR d.deadline_revision<>p_deadline OR p_request IS DISTINCT FROM coalesce(d.deadline_source_receipt_id,d.id)
  THEN RETURN '{"status":"waiting"}'; END IF;
 IF p_seconds IS NULL OR p_seconds NOT BETWEEN 1 AND 86400 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid pinned sideboard duration'; END IF;
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
REVOKE ALL ON FUNCTION app.deliver_native_between_decision(uuid,numeric,numeric,uuid,integer,uuid,text,text,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.deliver_native_between_decision(uuid,numeric,numeric,uuid,integer,uuid,text,text,text) TO service_role;
COMMIT;
