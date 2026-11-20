BEGIN;
CREATE FUNCTION app.cancel_ante_for_terminal(p_commitment uuid,p_terminal uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; m app.match; source app.command_receipt; receipt app.command_receipt; terms jsonb; operation uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal ante cancellation requires read committed retry';END IF;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;
 SELECT * INTO m FROM app.match WHERE id=c.match_id FOR NO KEY UPDATE;
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment FOR NO KEY UPDATE;
 SELECT * INTO source FROM app.command_receipt WHERE id=p_terminal;
 IF c.id IS NULL OR c.terms IS NULL OR m.terminal_receipt_id IS DISTINCT FROM p_terminal OR m.state<>'no_contest' OR source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'match_unrecoverable_v1' OR source.target_ref IS DISTINCT FROM m.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='offered ante cancellation requires accepted operational terminal cause';END IF;
 terms:=jsonb_build_object('version','ante-terminal-cancel-v1','commitmentId',p_commitment,'terminalReceiptId',p_terminal,'basis',c.terms_revision::text);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('ante-terminal:'||c.id::text,'ante_terminal_cancel_v1',c.id::text,p_terminal::text,app.auth_json_hash(terms),'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ante-terminal:'||c.id::text AND scope='ante_terminal_cancel_v1' AND request_ref=p_terminal::text FOR UPDATE;
 IF receipt.payload_hash<>app.auth_json_hash(terms) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal ante cancellation terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal ante cancellation pending';END IF;RETURN receipt.response_payload;END IF;
 operation:=app.release_offered_ante(c.id,receipt.id);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'state','canceled'));
END $$;
CREATE FUNCTION app.finalize_unrecoverable_match(p_match uuid,p_expected numeric,p_availability numeric,p_incident uuid,p_failure_ref text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; receipt app.command_receipt; terms jsonb; fingerprint text; original_room uuid; actors uuid[]; current_actors uuid[]; binding_id uuid; binding_ids uuid[]; current_binding_ids uuid[];
 result_id uuid; sequence_value numeric; source_season app.season; accepted_at timestamptz; final_games uuid[]; canceled_ids uuid[]; next_clock numeric; commitment app.ante_commitment; response jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='operational terminalization requires read committed retry';END IF;
 IF p_match IS NULL OR p_incident IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_availability IS NULL OR p_availability::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_failure_ref IS NULL OR length(btrim(p_failure_ref)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid failed-restoration source';END IF;
 terms:=jsonb_build_object('version','match-unrecoverable-v1','matchId',p_match,'basis',p_expected::text,'availabilityBasis',p_availability::text,'incidentId',p_incident,'failureRef',p_failure_ref);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('match-recovery:'||p_match::text,'match_unrecoverable_v1',p_match::text,p_incident::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-recovery:'||p_match::text AND scope='match_unrecoverable_v1' AND request_ref=p_incident::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='failed-restoration source terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='failed-restoration source pending';END IF;RETURN receipt.response_payload;END IF;
 SELECT room_id INTO original_room FROM app.match WHERE id=p_match;
 SELECT array_agg(b.id ORDER BY b.id) INTO binding_ids FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match);
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO actors FROM(SELECT account_id actor FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 PERFORM id FROM app.ai_runner_binding WHERE id=ANY(binding_ids) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT array_agg(b.id ORDER BY b.id) INTO current_binding_ids FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match);
 IF current_binding_ids IS DISTINCT FROM binding_ids THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller set changed before terminal parent fence';END IF;
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO current_actors FROM(SELECT account_id actor FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) roster;
 IF m.room_id IS DISTINCT FROM original_room OR current_actors IS DISTINCT FROM actors THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='terminal source parent or actor discovery changed';END IF;
 IF m.id IS NULL OR m.accepted_release_id IS NULL OR m.revision<>p_expected OR m.availability_revision<>p_availability OR m.availability<>'recovering' OR m.incident_receipt_id IS DISTINCT FROM p_incident OR m.state NOT IN ('ready','active','between_games') OR EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state NOT IN ('active','deletion_pending')) OR EXISTS(SELECT 1 FROM app.match_result WHERE match_id=p_match) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or contradictory unrecoverable match source';END IF;
 PERFORM id FROM app.game WHERE match_id=p_match ORDER BY game_index FOR NO KEY UPDATE;
 PERFORM op.id FROM app.operational_state op JOIN app.game g ON g.id=op.game_id WHERE g.match_id=p_match ORDER BY op.game_id FOR NO KEY UPDATE OF op;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 PERFORM app.check_match_availability(p_match);
 IF m.current_game_id IS NOT NULL THEN PERFORM app.check_game_recovery_source(m.current_game_id);END IF;
 IF EXISTS(SELECT 1 FROM app.game g WHERE g.match_id=p_match AND ((g.state='active' AND g.id IS DISTINCT FROM m.current_game_id) OR (g.state='reserved' AND g.id IS DISTINCT FROM m.reserved_game_id)))
 OR EXISTS(SELECT 1 FROM app.game_result WHERE game_id IN (m.current_game_id,m.reserved_game_id))
 OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=m.current_game_id)
 OR EXISTS(SELECT 1 FROM app.game g WHERE g.id=m.reserved_game_id AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id))
 OR EXISTS(SELECT 1 FROM app.game g WHERE g.match_id=p_match AND g.id IS DISTINCT FROM m.current_game_id AND g.id IS DISTINCT FROM m.reserved_game_id AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id) AND NOT EXISTS(SELECT 1 FROM app.game_result r JOIN app.command_receipt used ON used.principal_key='match-lifecycle:'||p_match::text AND used.scope='game_outcome_v1' AND used.target_ref=r.id::text AND used.state='accepted' WHERE r.game_id=g.id AND r.winner_seat_ids IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native outcome/frontier must be reconciled instead of overwritten';END IF;
 IF EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match AND NOT c.paused) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal incident bank is not paused';END IF;
 SELECT coalesce(array_agg(id ORDER BY game_index),'{}'::uuid[]) INTO final_games FROM app.game g WHERE match_id=p_match AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id);
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO canceled_ids FROM app.between_game_decision WHERE match_id=p_match AND state='pending';
 IF m.competitive THEN
 SELECT next_competitive_sequence INTO sequence_value FROM app.format WHERE id=m.format_id FOR NO KEY UPDATE;
 SELECT * INTO source_season FROM app.season WHERE start_at<=clock_timestamp() AND end_at>clock_timestamp() FOR SHARE;
 IF source_season.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive terminalization requires configured season';END IF;
 accepted_at:=clock_timestamp();IF accepted_at>=source_season.end_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='season boundary crossed during source fence';END IF;
 UPDATE app.format SET next_competitive_sequence=next_competitive_sequence+1 WHERE id=m.format_id;
 ELSE accepted_at:=clock_timestamp();END IF;
 INSERT INTO app.match_result(match_id,source_ref,outcome,winners_ref,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,source_game_result_id,terminal_kind,terminal_receipt_id)
 VALUES(p_match,'incident:'||p_incident::text,'no_contest','operational:none',accepted_at,CASE WHEN m.competitive THEN m.format_id END,sequence_value,'{}'::uuid[],final_games,NULL,'unrecoverable_incident',receipt.id) RETURNING id INTO result_id;
 UPDATE app.game SET state='no_contest' WHERE id=m.current_game_id;
 UPDATE app.game SET state='canceled' WHERE id=m.reserved_game_id;
 UPDATE app.between_game_decision SET state='canceled',terminal_receipt_id=receipt.id WHERE id=ANY(canceled_ids);
 SELECT max(c.clock_revision)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=p_match;
 UPDATE app.match_clock c SET paused=true,clock_revision=next_clock FROM app.match_seat seat WHERE seat.id=c.match_seat_id AND seat.match_id=p_match;
 UPDATE app.match SET state='no_contest',availability='unrecoverable',availability_revision=availability_revision+1,terminal_receipt_id=receipt.id,current_game_id=NULL,reserved_game_id=NULL,restoration_receipt_id=NULL,revision=revision+1 WHERE id=p_match;
 UPDATE app.operational_state SET availability='unrecoverable',fence_epoch=fence_epoch+1,revision=revision+1,terminal_receipt_id=receipt.id WHERE game_id=m.current_game_id;
 FOR binding_id IN SELECT b.id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE (slot.room_id=original_room OR EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.id=b.match_seat_id AND seat.match_id=p_match)) AND b.desired_state<>'stopped' ORDER BY b.id LOOP PERFORM app.request_ai_stop(binding_id,receipt.id);END LOOP;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('interruptedGameId',m.current_game_id,'neverOpenedGameId',m.reserved_game_id,'finalGameIds',to_jsonb(final_games),'canceledDecisionIds',to_jsonb(canceled_ids),'frontiers',app.match_retained_frontiers(p_match),'clocks',app.match_clock_snapshot(p_match),'operational',app.match_operational_snapshot(p_match),'terminalResultId',result_id,'matchAvailabilityRevision',(p_availability+1)::text) WHERE id=receipt.id;
 response:=app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'terminalResultId',result_id,'revision',(p_expected+1)::text,'availabilityRevision',(p_availability+1)::text,'phase','terminal'));
 FOR commitment IN SELECT * FROM app.ante_commitment WHERE match_id=p_match AND state='offered' ORDER BY id LOOP PERFORM app.cancel_ante_for_terminal(commitment.id,receipt.id);END LOOP;
 INSERT INTO app.durable_handoff(match_id,source_ref,destination,kind,state) VALUES(p_match,result_id::text,'match_settlement','match_outcome','pending');
 RETURN response;
END $$;
REVOKE ALL ON FUNCTION app.cancel_ante_for_terminal(uuid,uuid),app.finalize_unrecoverable_match(uuid,numeric,numeric,uuid,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
