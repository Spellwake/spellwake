BEGIN;
-- One whole-match shape check for the actual accepted room-start path. Draft
-- admissions and explicitly unqualified retained fixtures remain distinguishable.
CREATE FUNCTION app.check_between_game_answer(p_decision uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; receipt app.command_receipt; terms jsonb; revision text; qualification text;
BEGIN
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 IF d.id IS NULL OR d.state<>'answered' THEN RETURN;END IF;
 FOR receipt IN SELECT * FROM app.command_receipt WHERE scope='between_game_reply_v1' AND principal_key='match-seat:'||d.seat_id::text AND target_ref=d.id::text AND state='accepted' LOOP
 revision:=receipt.response_payload->'data'->>'revision';qualification:=receipt.response_payload->'data'->>'qualificationRef';
 IF revision IS NULL OR revision !~ '^[1-9][0-9]{0,77}$' OR qualification IS NULL OR length(btrim(qualification)) NOT BETWEEN 1 AND 2048 OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM d.id::text THEN CONTINUE;END IF;
 terms:=jsonb_build_object('decision',d.id,'basis',((revision::numeric)-1)::text,'revision',d.selected_deck_revision_id,'reply',d.reply_ref,'qualification',qualification);
 IF receipt.payload_hash=app.auth_json_hash(terms) THEN RETURN;END IF;
 END LOOP;
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='answered selection requires its own exact accepted reply source';
END $$;

CREATE FUNCTION app.check_started_match_frontier(p_match uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; item app.game; ctx app.game_context; native app.game_result; final app.match_result; opening app.command_receipt; previous app.game; round_result app.game_result; terminal app.command_receipt; count_games numeric; maximum numeric; seats bigint; pending bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match frontier changes require read committed retry';END IF;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF m.id IS NULL OR m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL THEN RETURN;END IF;
 IF NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='room_start_v1' AND source.principal_key='room-start:'||m.room_id::text AND source.target_ref=m.room_id::text AND source.state='accepted' AND source.response_payload->'data'->>'resourceId'=m.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='started room match requires its accepted start source';END IF;
 IF NOT EXISTS(SELECT 1 FROM app.game game0 JOIN app.command_receipt start ON start.scope='room_start_v1' AND start.principal_key='room-start:'||m.room_id::text AND start.target_ref=m.room_id::text AND start.state='accepted' AND start.response_payload->'data'->>'resourceId'=m.id::text AND start.response_payload->'data'->>'gameId'=game0.id::text WHERE game0.match_id=m.id AND game0.game_index=0) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game zero must retain the exact accepted room-start reservation';END IF;
 SELECT * INTO final FROM app.match_result WHERE match_id=m.id;
 SELECT count(*),max(game_index) INTO count_games,maximum FROM app.game WHERE match_id=m.id;
 SELECT count(*) INTO seats FROM app.match_seat WHERE match_id=m.id;
 IF count_games=0 OR count_games<>maximum+1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='started match game history must be contiguous from game zero';END IF;
 IF m.state='ready' THEN
 IF m.current_game_id IS NOT NULL OR m.reserved_game_id IS NULL OR final.id IS NOT NULL OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=m.reserved_game_id AND match_id=m.id AND state='reserved' AND game_index=maximum) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ready match requires only its actual unopened frontier';END IF;
 ELSIF m.state='active' THEN
 IF m.current_game_id IS NULL OR m.reserved_game_id IS NOT NULL OR final.id IS NOT NULL OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=m.current_game_id AND match_id=m.id AND state IN('active','completed','no_contest') AND game_index=maximum) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='active match requires its actual current game frontier';END IF;
 ELSIF m.state='between_games' THEN
 IF m.current_game_id IS NOT NULL OR m.reserved_game_id IS NOT NULL OR final.id IS NOT NULL OR m.structure<>'bo3' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game match cannot retain a live or terminal frontier';END IF;
 SELECT * INTO previous FROM app.game WHERE match_id=m.id AND game_index=maximum;
 SELECT * INTO round_result FROM app.game_result WHERE game_id=previous.id;
 IF previous.state<>'completed' OR round_result.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.principal_key='match-lifecycle:'||m.id::text AND source.scope='game_outcome_v1' AND source.target_ref=round_result.id::text AND source.request_ref=round_result.id::text AND source.state='accepted' AND source.response_payload->'data'->>'phase'='between_games') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game phase requires its latest consumed nonterminal game';END IF;
 SELECT count(*) FILTER(WHERE state='pending') INTO pending FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=previous.id;
 IF pending=0 OR (SELECT count(*) FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=previous.id)<>seats OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=m.id AND NOT EXISTS(SELECT 1 FROM app.between_game_decision d WHERE d.match_id=m.id AND d.after_game_id=previous.id AND d.seat_id=seat.id AND d.state IN('pending','answered'))) OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending' AND after_game_id IS DISTINCT FROM previous.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game phase requires the exact current per-seat reply set';END IF;
 ELSIF m.state IN('completed','no_contest') THEN
 IF m.current_game_id IS NOT NULL OR m.reserved_game_id IS NOT NULL OR final.id IS NULL OR (final.outcome='no_contest') IS DISTINCT FROM (m.state='no_contest') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal match phase must retain its actual result and no live frontier';END IF;
 IF final.terminal_kind='unrecoverable_incident' THEN
 PERFORM app.check_operational_match_terminal(m.id);SELECT * INTO terminal FROM app.command_receipt WHERE id=final.terminal_receipt_id;
 ELSE
 IF final.final_game_ids IS NULL OR final.final_game_ids IS DISTINCT FROM (SELECT array_agg(id ORDER BY game_index) FROM app.game WHERE match_id=m.id) OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='game_outcome_v1' AND source.principal_key='match-lifecycle:'||m.id::text AND source.target_ref=final.source_game_result_id::text AND source.request_ref=final.source_game_result_id::text AND source.state='accepted' AND source.payload_hash=app.auth_json_hash(jsonb_build_object('resultId',final.source_game_result_id)) AND source.normalized_source->>'gameResultId'=final.source_game_result_id::text AND source.normalized_source->>'matchId'=m.id::text AND source.response_payload->'data'->>'resourceId'=m.id::text AND source.response_payload->'data'->>'terminalResultId'=final.id::text AND source.response_payload->'data'->>'phase'='terminal' AND source.response_payload->'data'->>'revision'=m.revision::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ordinary terminal result requires its exact accepted lifecycle source and history';END IF;
 END IF;
 ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported started match phase';END IF;
 IF m.state<>'between_games' AND EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending selections require the actual between-game phase';END IF;
 PERFORM app.check_between_game_answer(id) FROM app.between_game_decision WHERE match_id=m.id AND state='answered';
 FOR item IN SELECT * FROM app.game WHERE match_id=m.id ORDER BY game_index LOOP
 SELECT * INTO native FROM app.game_result WHERE game_id=item.id;
 IF item.state='reserved' THEN
 IF item.accepted_revision<>0 OR item.id IS DISTINCT FROM m.reserved_game_id OR EXISTS(SELECT 1 FROM app.game_context WHERE game_id=item.id) OR EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=item.id) OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=item.id) OR EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=item.id) OR native.id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved frontier must remain genuinely unopened';END IF;
 ELSIF item.state='canceled' THEN
 IF terminal.id IS NULL OR terminal.normalized_source->>'neverOpenedGameId' IS DISTINCT FROM item.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canceled started game requires its exact operational cancellation';END IF;
 ELSE
 IF item.state='active' AND item.id IS DISTINCT FROM m.current_game_id OR item.state NOT IN('active','completed','no_contest') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unpointed live game is not a match frontier';END IF;
 SELECT * INTO ctx FROM app.game_context WHERE game_id=item.id;
 SELECT * INTO opening FROM app.command_receipt WHERE scope='game_opening_v1' AND principal_key='match-opening:'||m.id::text AND target_ref=item.id::text AND request_ref=item.id::text;
 IF ctx.id IS NULL OR ctx.profile_id IS DISTINCT FROM m.profile_id OR ctx.release_id IS DISTINCT FROM m.accepted_release_id OR opening.state IS DISTINCT FROM 'accepted' OR opening.response_payload->'data'->>'resourceId' IS DISTINCT FROM item.id::text OR opening.response_payload->'data'->>'contextId' IS DISTINCT FROM ctx.id::text OR opening.normalized_source->>'version' IS DISTINCT FROM 'game-opening-v1'
 OR NOT EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=item.id AND context_id=ctx.id AND accepted_revision=0) OR NOT EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=item.id AND context_id=ctx.id AND accepted_revision=item.accepted_revision) OR NOT EXISTS(SELECT 1 FROM app.game_event WHERE game_id=item.id AND context_id=ctx.id AND sequence=0 AND accepted_revision=0) OR NOT EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=item.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opened game requires its complete accepted opening and current native frontier';END IF;
 IF item.accepted_revision>0 AND NOT EXISTS(SELECT 1 FROM app.game_checkpoint checkpoint JOIN app.command_receipt source ON source.scope='host_candidate_v1' AND source.principal_key='host:'||item.id::text AND source.target_ref=item.id::text AND source.state='accepted' AND source.normalized_source->>'version'='host-clock-transition-v1' AND source.normalized_source->>'acceptedRevision'=item.accepted_revision::text AND source.normalized_source->>'gameId'=item.id::text AND source.response_payload->'data'->>'resourceId'=checkpoint.id::text WHERE checkpoint.game_id=item.id AND checkpoint.context_id=ctx.id AND checkpoint.accepted_revision=item.accepted_revision) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='current native checkpoint requires its exact accepted host source';END IF;
 IF ctx.snapshot_ref IS DISTINCT FROM opening.normalized_source->>'snapshotRef' OR ctx.context_revision IS DISTINCT FROM opening.normalized_source->>'contextRevision' OR NOT EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=item.id AND accepted_revision=0 AND private_state_ref=opening.normalized_source->>'stateRef' AND facts_ref=opening.normalized_source->>'factsRef') OR NOT EXISTS(SELECT 1 FROM app.game_event WHERE game_id=item.id AND sequence=0 AND private_payload_ref=opening.normalized_source->>'eventRef') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening artifacts must retain their exact accepted source';END IF;
 IF item.state='active' THEN
 IF native.id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native terminal result cannot remain an active game';END IF;
 ELSIF terminal.id IS NULL OR terminal.normalized_source->>'interruptedGameId' IS DISTINCT FROM item.id::text THEN
 IF native.id IS NULL OR native.winner_seat_ids IS NULL OR (native.outcome='no_contest') IS DISTINCT FROM (item.state='no_contest') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='completed game requires its matching actual native result';END IF;
 END IF;
 END IF;
 IF item.game_index>0 THEN
 SELECT * INTO previous FROM app.game WHERE match_id=m.id AND game_index=item.game_index-1;
 SELECT * INTO round_result FROM app.game_result WHERE game_id=previous.id;
 IF round_result.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.scope='game_outcome_v1' AND source.principal_key='match-lifecycle:'||m.id::text AND source.target_ref=round_result.id::text AND source.state='accepted' AND source.response_payload->'data'->>'phase'='between_games')
 OR (SELECT count(*) FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=previous.id AND state='answered')<>seats OR NOT EXISTS(SELECT 1 FROM app.between_game_decision d JOIN app.command_receipt reply ON reply.scope='between_game_reply_v1' AND reply.principal_key='match-seat:'||d.seat_id::text AND reply.target_ref=d.id::text AND reply.state='accepted' AND reply.response_payload->'data'->>'reservedGameId'=item.id::text WHERE d.match_id=m.id AND d.after_game_id=previous.id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='successor game requires its exact completed selection round';END IF;
 END IF;
 END LOOP;
END $$;
CREATE FUNCTION app.guard_started_match_frontier() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parents uuid[]; old_parent uuid; new_parent uuid; parent uuid;
BEGIN
 -- Check isolation before source discovery, including snapshots predating start.
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match frontier mutations require read committed retry';END IF;
 IF TG_TABLE_NAME='match' THEN
 IF TG_OP<>'INSERT' THEN old_parent:=OLD.id;END IF;IF TG_OP<>'DELETE' THEN new_parent:=NEW.id;END IF;
 ELSIF TG_TABLE_NAME IN('game','between_game_decision','match_result') THEN
 IF TG_OP<>'INSERT' THEN old_parent:=OLD.match_id;END IF;IF TG_OP<>'DELETE' THEN new_parent:=NEW.match_id;END IF;
 ELSE
 IF TG_OP<>'INSERT' THEN SELECT match_id INTO old_parent FROM app.game WHERE id=OLD.game_id;END IF;
 IF TG_OP<>'DELETE' THEN SELECT match_id INTO new_parent FROM app.game WHERE id=NEW.game_id;END IF;
 END IF;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO parents FROM unnest(ARRAY[old_parent,new_parent]) id WHERE id IS NOT NULL;
 PERFORM id FROM app.match WHERE id=ANY(parents) ORDER BY id FOR NO KEY UPDATE;
 FOREACH parent IN ARRAY coalesce(parents,'{}'::uuid[]) LOOP PERFORM app.check_started_match_frontier(parent);END LOOP;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER started_match_frontier AFTER INSERT OR UPDATE OR DELETE ON app.match DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
CREATE CONSTRAINT TRIGGER started_game_frontier AFTER INSERT OR UPDATE OR DELETE ON app.game DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
CREATE CONSTRAINT TRIGGER started_decision_frontier AFTER INSERT OR UPDATE OR DELETE ON app.between_game_decision DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
CREATE CONSTRAINT TRIGGER started_match_result_frontier AFTER INSERT OR UPDATE OR DELETE ON app.match_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
CREATE CONSTRAINT TRIGGER started_game_result_frontier AFTER INSERT OR UPDATE OR DELETE ON app.game_result DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
CREATE CONSTRAINT TRIGGER started_context_frontier AFTER INSERT OR UPDATE OR DELETE ON app.game_context DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_started_match_frontier();
REVOKE ALL ON FUNCTION app.check_started_match_frontier(uuid),app.guard_started_match_frontier(),app.check_between_game_answer(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
