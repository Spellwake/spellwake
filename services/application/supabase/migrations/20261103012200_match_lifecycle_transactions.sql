BEGIN;
CREATE FUNCTION app.accept_reserved_game(p_match uuid,p_expected numeric,p_game uuid,p_owner text,p_opening jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; receipt app.command_receipt; fingerprint text; context_id uuid; at_time timestamptz; next_clock numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(p_opening->k) IS DISTINCT FROM 'string' OR length(btrim(p_opening->>k)) NOT BETWEEN 1 AND 2048)
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening) k WHERE k NOT IN ('snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized opening artifacts'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',p_match,'game',p_game,'owner',p_owner,'opening',p_opening));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-opening:'||p_match::text,'game_opening_v1',p_game::text,p_game::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text AND scope='game_opening_v1' AND request_ref=p_game::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening source was reused with different artifacts'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF m.id IS NULL OR g.id IS NULL OR m.revision<>p_expected OR m.reserved_game_id IS DISTINCT FROM p_game OR m.current_game_id IS NOT NULL OR g.match_id<>m.id OR g.state<>'reserved' OR m.state<>'ready' OR m.accepted_release_id IS NULL THEN
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unavailable game reservation; retry from current basis'; END IF;
 -- Optional ante must have its accepted commitment before an opening exists.
 IF EXISTS(SELECT 1 FROM app.ante_commitment WHERE match_id=m.id AND state NOT IN ('committed','canceled')) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante source is not committed; opening remains reserved'; END IF;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id;
 INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision)
 VALUES(g.id,m.accepted_release_id,m.profile_id,p_opening->>'snapshotRef',p_opening->>'contextRevision') RETURNING id INTO context_id;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision)
 VALUES(g.id,context_id,0,p_opening->>'stateRef',p_opening->>'factsRef',next_clock);
 INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES(g.id,context_id,0,0,p_opening->>'eventRef');
 UPDATE app.game SET state='active' WHERE id=g.id;
 UPDATE app.match SET state='active',reserved_game_id=NULL,current_game_id=g.id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES(g.id,p_owner,1,'available',0);
 at_time:=clock_timestamp();
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;

CREATE FUNCTION app.consume_game_outcome(p_result uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE r app.game_result; g app.game; m app.match; receipt app.command_receipt; terminal boolean; winners uuid[];
 final_games uuid[]; result_id uuid; sequence_value numeric; accepted_at timestamptz; source_season app.season; pending_ids uuid[];
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='outcome consumption requires read committed retry'; END IF;
 IF p_result IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid outcome source basis'; END IF;
 SELECT * INTO r FROM app.game_result WHERE id=p_result;
 SELECT * INTO g FROM app.game WHERE id=r.game_id;
 IF r.id IS NULL OR g.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='game outcome unavailable'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-lifecycle:'||g.match_id::text,'game_outcome_v1',r.id::text,r.id::text,app.auth_json_hash(jsonb_build_object('resultId',r.id)),'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-lifecycle:'||g.match_id::text AND scope='game_outcome_v1' AND request_ref=r.id::text FOR UPDATE;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='outcome consumption pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=g.match_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT g FROM app.game WHERE id=g.id FOR SHARE;
 SELECT * INTO STRICT r FROM app.game_result WHERE id=r.id FOR SHARE;
 IF m.revision<>p_expected OR m.state<>'active' OR m.current_game_id IS DISTINCT FROM g.id OR g.state NOT IN ('completed','no_contest') OR r.winner_seat_ids IS NULL THEN
 -- A stale delivery has no permanent consumption disposition; the same immutable
 -- source can be retried after obtaining the actual lifecycle basis.
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unqualified game outcome; retry from current basis'; END IF;
 PERFORM app.validate_normalized_winners(m.id,r.outcome,r.winner_seat_ids);
 IF m.structure NOT IN ('bo1','bo3') OR (m.structure='bo3' AND (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)<>2) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported match scoring structure'; END IF;
 terminal:=m.structure='bo1' OR r.outcome='no_contest';winners:=r.winner_seat_ids;
 IF NOT terminal AND r.outcome='win' THEN
  SELECT coalesce(array_agg(s.id ORDER BY s.id),'{}'::uuid[]) INTO winners FROM app.match_seat s WHERE s.match_id=m.id AND
   (SELECT count(*) FROM app.game_result past JOIN app.game pg ON pg.id=past.game_id WHERE pg.match_id=m.id AND past.outcome='win' AND s.id=ANY(past.winner_seat_ids)
    AND (past.id=r.id OR EXISTS(SELECT 1 FROM app.command_receipt used WHERE used.principal_key='match-lifecycle:'||m.id::text AND used.scope='game_outcome_v1' AND used.target_ref=past.id::text AND used.state='accepted')))>=2;
  terminal:=cardinality(winners)>0;
 END IF;
 IF terminal THEN
  SELECT array_agg(pg.id ORDER BY pg.game_index) INTO final_games FROM app.game pg JOIN app.game_result past ON past.game_id=pg.id WHERE pg.match_id=m.id AND
   (past.id=r.id OR EXISTS(SELECT 1 FROM app.command_receipt used WHERE used.principal_key='match-lifecycle:'||m.id::text AND used.scope='game_outcome_v1' AND used.target_ref=past.id::text AND used.state='accepted'));
  IF m.competitive THEN
   SELECT next_competitive_sequence INTO sequence_value FROM app.format WHERE id=m.format_id FOR NO KEY UPDATE;
   SELECT * INTO source_season FROM app.season WHERE start_at<=clock_timestamp() AND end_at>clock_timestamp() FOR SHARE;
   IF source_season.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive completion requires the configured season'; END IF;
   accepted_at:=clock_timestamp();
   IF accepted_at>=source_season.end_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season boundary crossed during source fence; retry'; END IF;
   UPDATE app.format SET next_competitive_sequence=next_competitive_sequence+1 WHERE id=m.format_id;
  ELSE accepted_at:=clock_timestamp(); END IF;
  INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id)
  VALUES(m.id,'game-result:'||r.id::text,r.outcome,'normalized:'||r.id::text,accepted_at,CASE WHEN m.competitive THEN m.format_id END,sequence_value,winners,final_games,r.id) RETURNING id INTO result_id;
  UPDATE app.match SET state=CASE WHEN r.outcome='no_contest' THEN 'no_contest' ELSE 'completed' END,current_game_id=NULL,revision=revision+1 WHERE id=m.id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(m.id,g.id,result_id::text,'match_settlement','match_outcome','pending');
 ELSE
  UPDATE app.match SET state='between_games',current_game_id=NULL,revision=revision+1 WHERE id=m.id;
  INSERT INTO app.between_game_decision(match_id,seat_id,decision_kind,basis_revision,clock_revision,state,after_game_id)
   SELECT m.id,s.id,'sideboard',m.revision+1,c.clock_revision,'pending',g.id FROM app.match_seat s JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=m.id;
  SELECT array_agg(id ORDER BY id) INTO pending_ids FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=g.id;
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-outcome-consumption-v1','gameResultId',r.id,'gameId',g.id,'matchId',m.id,'winnerSeatIds',r.winner_seat_ids) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',m.id,'revision',(m.revision+1)::text,'phase',CASE WHEN terminal THEN 'terminal' ELSE 'between_games' END,'terminalResultId',coalesce(result_id::text,''),'pendingDecisionIds',coalesce(to_jsonb(pending_ids),'[]'::jsonb)::text));
END $$;
REVOKE ALL ON FUNCTION app.accept_reserved_game(uuid,numeric,uuid,text,jsonb),app.consume_game_outcome(uuid,numeric)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
