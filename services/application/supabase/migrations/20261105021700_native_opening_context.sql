BEGIN;
-- Trusted preparation must know its exact context UUID before retaining scoped
-- artifacts. Only the existing atomic opening transaction establishes authority.
-- Existing function signatures/ACLs stay unchanged; no raw runtime grant expands.
CREATE OR REPLACE FUNCTION app.accept_reserved_game(p_match uuid,p_expected numeric,p_game uuid,p_owner text,p_opening jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; receipt app.command_receipt; fingerprint text; context_id uuid; at_time timestamptz; next_clock numeric; ante_source app.ante_commitment; original_room uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(p_opening->k) IS DISTINCT FROM 'string' OR length(btrim(p_opening->>k)) NOT BETWEEN 1 AND 2048)
 OR jsonb_typeof(p_opening->'contextId') IS DISTINCT FROM 'string' OR (p_opening->>'contextId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR (p_opening->>'contextId')='00000000-0000-0000-0000-000000000000'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening) k WHERE k NOT IN ('contextId','snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized opening artifacts'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',p_match,'game',p_game,'owner',p_owner,'opening',p_opening));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-opening:'||p_match::text,'game_opening_v1',p_game::text,p_game::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text AND scope='game_opening_v1' AND request_ref=p_game::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening source was reused with different artifacts'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT room_id INTO original_room FROM app.match WHERE id=p_match;
 PERFORM id FROM app.room WHERE id=original_room FOR NO KEY UPDATE;
 PERFORM b.id FROM app.ai_runner_binding b JOIN app.match_seat s ON s.id=b.match_seat_id WHERE s.match_id=p_match ORDER BY b.id FOR NO KEY UPDATE OF b;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF m.room_id IS DISTINCT FROM original_room THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening room source changed before fence';END IF;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF m.id IS NULL OR g.id IS NULL OR m.availability<>'available' OR m.revision<>p_expected OR m.reserved_game_id IS DISTINCT FROM p_game OR m.current_game_id IS NOT NULL OR g.match_id<>m.id OR g.state<>'reserved' OR m.state<>'ready' OR m.accepted_release_id IS NULL THEN
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unavailable game reservation; retry from current basis'; END IF;
 IF EXISTS(SELECT 1 FROM app.match_seat seat WHERE seat.match_id=m.id AND seat.controller_kind='ai' AND NOT EXISTS(
 SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.controller_grant grant_row ON grant_row.binding_id=b.id AND grant_row.binding_epoch=b.binding_epoch AND grant_row.match_seat_id=seat.id
 WHERE b.match_seat_id=seat.id AND b.bootstrap_receipt_id IS NOT NULL AND b.desired_state='running' AND app.ai_slot_ready(s.id) AND grant_row.state='active' AND grant_row.delivery_state='delivered' AND b.ready_deck_revision_id=seat.source_revision_id)) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI opening requires delivered adopted grant and fresh current-epoch readiness';END IF;
 -- Optional ante must have its accepted commitment before an opening exists.
 IF EXISTS(SELECT 1 FROM app.ante_commitment WHERE match_id=m.id AND state NOT IN ('committed','canceled')) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante source is not committed; opening remains reserved'; END IF;
 FOR ante_source IN SELECT * FROM app.ante_commitment WHERE match_id=m.id LOOP
 IF ante_source.terms IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game opening requires source-qualified ante terms'; END IF;
 PERFORM app.check_ante_source(ante_source.id);
 END LOOP;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id;
 INSERT INTO app.game_context(id,game_id,release_id,profile_id,snapshot_ref,context_revision)
 VALUES((p_opening->>'contextId')::uuid,g.id,m.accepted_release_id,m.profile_id,p_opening->>'snapshotRef',p_opening->>'contextRevision') RETURNING id INTO context_id;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id)
 VALUES(g.id,context_id,0,p_opening->>'stateRef',p_opening->>'factsRef',next_clock,receipt.id);
 INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id) VALUES(g.id,context_id,0,0,p_opening->>'eventRef',receipt.id);
 UPDATE app.game SET state='active' WHERE id=g.id;
 UPDATE app.match SET state='active',reserved_game_id=NULL,current_game_id=g.id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES(g.id,p_owner,1,'available',0);
 at_time:=clock_timestamp();
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 UPDATE app.command_receipt SET normalized_source=coalesce(normalized_source,'{}'::jsonb)||jsonb_build_object('matchId',m.id,'gameId',p_game,'owner',p_owner,'clockValues',app.match_clock_snapshot(m.id)) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;

-- Preserve the complete current190 host/elimination proof, extending only its
-- opening branch with the exact original proposed context identity.
CREATE OR REPLACE FUNCTION app.check_native_creator_frame(p_source uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source app.command_receipt; g app.game; m app.match; cp app.game_checkpoint; ctx app.game_context; event app.game_event; outcome app.game_result; body jsonb; expected jsonb; original jsonb; item jsonb; terminal jsonb; fact app.game_activity_fact; participant app.game_participation; seat app.match_seat; ordinal bigint; first_sequence numeric; revision numeric; is_opening boolean; clock_values jsonb; requested_timeout jsonb; projected_timeout jsonb;
BEGIN
 SELECT * INTO source FROM app.command_receipt WHERE id=p_source;
 IF source.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native creator is unavailable';END IF;
 is_opening:=source.scope='game_opening_v1';
 IF NOT ((is_opening AND source.principal_key LIKE 'match-opening:%') OR (source.scope='host_candidate_v1' AND source.principal_key LIKE 'host:%')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native creator family is unavailable';END IF;
 IF source.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native frame creator must be accepted';END IF;
 SELECT * INTO g FROM app.game WHERE id::text=source.target_ref;SELECT * INTO m FROM app.match WHERE id=g.match_id;
 SELECT * INTO cp FROM app.game_checkpoint WHERE source_receipt_id=source.id;
 SELECT * INTO ctx FROM app.game_context WHERE id=cp.context_id;
 IF g.id IS NULL OR m.id IS NULL OR (SELECT count(*) FROM app.game_checkpoint WHERE source_receipt_id=source.id)<>1 OR cp.game_id IS DISTINCT FROM g.id OR ctx.game_id IS DISTINCT FROM g.id OR source.normalized_source->>'gameId' IS DISTINCT FROM g.id::text OR source.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR cp.accepted_revision>g.accepted_revision THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native frame requires one actual same-game checkpoint';END IF;
 clock_values:=CASE WHEN is_opening THEN source.normalized_source->'clockValues' ELSE source.normalized_source->'after' END;
 IF jsonb_typeof(clock_values) IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native checkpoint requires its creator clock snapshot';END IF;
 IF jsonb_array_length(clock_values)=0 OR jsonb_array_length(clock_values)<>(SELECT count(*) FROM app.match_seat WHERE match_id=m.id) OR (SELECT count(DISTINCT value->>'seatId') FROM jsonb_array_elements(clock_values))<>jsonb_array_length(clock_values) OR EXISTS(SELECT 1 FROM jsonb_array_elements(clock_values) value WHERE value->>'clockRevision' IS DISTINCT FROM cp.clock_revision::text OR NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id::text=value->>'seatId' AND match_id=m.id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='checkpoint clock revision must match its exact complete creator snapshot';END IF;
 IF is_opening THEN
 original:=jsonb_build_object('contextId',source.normalized_source->'contextId','snapshotRef',source.normalized_source->'snapshotRef','contextRevision',source.normalized_source->'contextRevision','stateRef',source.normalized_source->'stateRef','factsRef',source.normalized_source->'factsRef','eventRef',source.normalized_source->'eventRef','qualificationRef',source.normalized_source->'qualificationRef');
 expected:=jsonb_build_object('match',m.id,'game',g.id,'owner',source.normalized_source->>'owner','opening',original);
 SELECT * INTO event FROM app.game_event WHERE source_receipt_id=source.id;
 IF source.principal_key IS DISTINCT FROM 'match-opening:'||m.id::text OR source.request_ref IS DISTINCT FROM g.id::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(expected) OR source.normalized_source->>'version' IS DISTINCT FROM 'game-opening-v1' OR source.normalized_source->>'owner' IS NULL OR ctx.id::text IS DISTINCT FROM original->>'contextId' OR cp.accepted_revision<>0 OR cp.private_state_ref IS DISTINCT FROM original->>'stateRef' OR cp.facts_ref IS DISTINCT FROM original->>'factsRef' OR ctx.snapshot_ref IS DISTINCT FROM original->>'snapshotRef' OR ctx.context_revision IS DISTINCT FROM original->>'contextRevision' OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM g.id::text OR source.response_payload->'data'->>'contextId' IS DISTINCT FROM ctx.id::text
 OR (SELECT count(*) FROM app.game_event WHERE source_receipt_id=source.id)<>1 OR event.game_id IS DISTINCT FROM g.id OR event.context_id IS DISTINCT FROM ctx.id OR event.sequence<>0 OR event.accepted_revision<>0 OR event.private_payload_ref IS DISTINCT FROM original->>'eventRef' OR EXISTS(SELECT 1 FROM app.game_activity_fact WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_result WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=g.id AND accepted_revision=0) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening frame differs from its exact accepted source';END IF;
 RETURN;
 END IF;
 body:=source.normalized_source->'candidate';revision:=cp.accepted_revision;
 IF jsonb_typeof(body) IS DISTINCT FROM 'object' OR jsonb_typeof(body->'events') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'facts') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'participation') IS DISTINCT FROM 'array' OR jsonb_typeof(body->'clocks') IS DISTINCT FROM 'array' OR jsonb_typeof(source.normalized_source->'eventIds') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame requires its exact original normalized input';END IF;
 expected:=jsonb_build_object('owner',source.normalized_source->>'owner','epoch',source.normalized_source->>'epoch','basis',source.normalized_source->>'basisRevision','candidate',body);
 IF source.principal_key IS DISTINCT FROM 'host:'||g.id::text OR source.normalized_source->>'version' IS DISTINCT FROM 'host-clock-transition-v1' OR source.normalized_source->>'owner' IS NULL OR source.normalized_source->>'epoch' IS NULL OR source.normalized_source->>'epoch' !~ '^(0|[1-9][0-9]{0,77})$' OR source.normalized_source->>'basisRevision' IS NULL OR source.normalized_source->>'basisRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR revision<>(source.normalized_source->>'basisRevision')::numeric+1 OR source.normalized_source->>'acceptedRevision' IS DISTINCT FROM revision::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(expected) OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM cp.id::text OR source.response_payload->'data'->>'revision' IS DISTINCT FROM revision::text
 OR body->>'contextId' IS DISTINCT FROM ctx.id::text OR cp.private_state_ref IS DISTINCT FROM body->>'stateRef' OR cp.facts_ref IS DISTINCT FROM body->>'factsRef' OR jsonb_array_length(body->'events') NOT BETWEEN 1 AND 1024 OR jsonb_array_length(body->'events')<>jsonb_array_length(source.normalized_source->'eventIds') OR (SELECT count(*) FROM app.game_event WHERE source_receipt_id=source.id)<>jsonb_array_length(body->'events') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host checkpoint or exact frame membership differs from its creator';END IF;
 IF jsonb_typeof(source.normalized_source->'before') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame requires its prior clock basis';END IF;
 FOR item IN SELECT value FROM jsonb_array_elements(source.normalized_source->'before') LOOP
 IF item->>'clockRevision' IS NULL OR item->>'clockRevision' !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host prior clock revision must be canonical';END IF;
 END LOOP;
 IF cp.clock_revision IS DISTINCT FROM (SELECT max((value->>'clockRevision')::numeric)+1 FROM jsonb_array_elements(source.normalized_source->'before')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host checkpoint must advance the shared clock revision once';END IF;
 SELECT min(sequence) INTO first_sequence FROM app.game_event WHERE source_receipt_id=source.id;
 FOR item,ordinal IN SELECT value,ordinality FROM jsonb_array_elements(source.normalized_source->'eventIds') WITH ORDINALITY LOOP
 SELECT * INTO event FROM app.game_event WHERE id::text=item#>>'{}';
 IF event.id IS NULL OR event.source_receipt_id IS DISTINCT FROM source.id OR event.game_id IS DISTINCT FROM g.id OR event.context_id IS DISTINCT FROM ctx.id OR event.accepted_revision<>revision OR event.sequence<>first_sequence+ordinal-1 OR event.private_payload_ref IS DISTINCT FROM body->'events'->>(ordinal-1)::int THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host events must preserve exact creator order and artifacts';END IF;
 END LOOP;
 IF first_sequence IS DISTINCT FROM (SELECT coalesce(max(sequence),-1)+1 FROM app.game_event WHERE game_id=g.id AND accepted_revision<revision) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host frame must extend the actual prior native event frontier';END IF;
 IF (SELECT count(*) FROM app.game_activity_fact f JOIN app.game_event e ON e.id=f.source_event_id WHERE e.source_receipt_id=source.id)<>jsonb_array_length(body->'facts') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host fact set differs from accepted normalized input';END IF;
 FOR fact IN SELECT f.* FROM app.game_activity_fact f JOIN app.game_event e ON e.id=f.source_event_id WHERE e.source_receipt_id=source.id LOOP
 SELECT ordinality-1 INTO ordinal FROM jsonb_array_elements(source.normalized_source->'eventIds') WITH ORDINALITY WHERE value#>>'{}'=fact.source_event_id::text;
 item:=jsonb_build_object('eventIndex',ordinal::text,'accountId',fact.account_id,'kind',fact.kind,'value',fact.value::text,'revision',fact.fact_revision);
 IF fact.game_id IS DISTINCT FROM g.id OR NOT coalesce(body->'facts' @> jsonb_build_array(item),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='normalized fact lacks exact accepted host provenance';END IF;
 END LOOP;
 IF (SELECT count(*) FROM app.game_participation WHERE game_id=g.id AND accepted_revision=revision)<>jsonb_array_length(body->'participation') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation set differs from accepted native frame';END IF;
 FOR participant IN SELECT * FROM app.game_participation WHERE game_id=g.id AND accepted_revision=revision LOOP
 SELECT * INTO seat FROM app.match_seat WHERE id=participant.match_seat_id;
 item:=jsonb_build_object('seatId',participant.match_seat_id,'ownTurns',participant.own_turns::text,'reason',participant.reason);
 IF seat.match_id IS DISTINCT FROM m.id OR seat.controller_kind IS DISTINCT FROM 'human' OR participant.account_id IS DISTINCT FROM seat.account_id OR participant.real_game IS DISTINCT FROM (participant.own_turns>=3) OR participant.completed_at IS DISTINCT FROM app.financial_instant(source.normalized_source->>'acceptedAt') OR NOT coalesce(body->'participation' @> jsonb_build_array(item),false) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='human participation lacks exact accepted host provenance';END IF;
 END LOOP;
 terminal:=body->'terminal';IF terminal='null'::jsonb THEN terminal:=NULL;END IF;
 requested_timeout:=body->'bankTimeout';IF requested_timeout='null'::jsonb THEN requested_timeout:=NULL;END IF;
 projected_timeout:=source.normalized_source->'bankTimeout';IF projected_timeout='null'::jsonb THEN projected_timeout:=NULL;END IF;
 IF requested_timeout IS DISTINCT FROM (projected_timeout-'priorClockSourceId') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='derived bank timeout must retain the exact original accepted input';END IF;

 SELECT r.* INTO outcome FROM app.game_result r JOIN app.game_event e ON e.id=r.source_event_id WHERE e.source_receipt_id=source.id;
 IF terminal IS NULL THEN
 IF outcome.id IS NOT NULL OR source.normalized_source->>'gameResultId' IS NOT NULL OR jsonb_array_length(body->'participation')<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='nonterminal host frame cannot acquire a later result or participation';END IF;
 ELSE
 IF outcome.id IS NULL OR (SELECT count(*) FROM app.game_result r JOIN app.game_event e ON e.id=r.source_event_id WHERE e.source_receipt_id=source.id)<>1 OR outcome.id::text IS DISTINCT FROM source.normalized_source->>'gameResultId' OR outcome.game_id IS DISTINCT FROM g.id OR outcome.source_event_id::text IS DISTINCT FROM source.normalized_source->'eventIds'->>-1 OR outcome.outcome IS DISTINCT FROM terminal->>'outcome' OR outcome.winners_ref IS DISTINCT FROM terminal->>'winnersRef' OR outcome.end_reason IS DISTINCT FROM terminal->>'reason' OR to_jsonb(outcome.winner_seat_ids) IS DISTINCT FROM terminal->'winnerSeatIds' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native terminal output differs from its exact accepted frame';END IF;
 END IF;
 IF (requested_timeout IS NULL AND outcome.bank_forfeit_receipt_id IS NOT NULL) OR (requested_timeout IS NOT NULL AND (terminal IS NULL OR outcome.id IS NULL OR outcome.bank_forfeit_receipt_id IS DISTINCT FROM source.id OR outcome.bank_forfeit_seat_id::text IS DISTINCT FROM requested_timeout->>'seatId')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank result must project its actual accepted timeout';END IF;
 PERFORM app.check_native_elimination_set(source.id);
END $$;
COMMIT;
