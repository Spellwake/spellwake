BEGIN;
-- Event codec claims are explicit original terms, not proof of native semantics.
-- Preserve absent historical markers and original-first retries. All unrelated
-- clock/locking/accounting bodies retain their237/231 behavior. No new tables.
CREATE OR REPLACE FUNCTION app.accept_reserved_game(p_match uuid,p_expected numeric,p_game uuid,p_owner text,p_opening jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; receipt app.command_receipt; fingerprint text; context_id uuid; at_time timestamptz; next_clock numeric; ante_source app.ante_commitment; original_room uuid; initial_waiter uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(p_opening->k) IS DISTINCT FROM 'string' OR length(btrim(p_opening->>k)) NOT BETWEEN 1 AND 2048)
 OR jsonb_typeof(p_opening->'contextId') IS DISTINCT FROM 'string' OR (p_opening->>'contextId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR (p_opening->>'contextId')='00000000-0000-0000-0000-000000000000'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening) k WHERE k NOT IN ('contextId','snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef','clockObservation','eventSchemaRevision')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized opening artifacts'; END IF;

 IF p_opening ? 'eventSchemaRevision' AND p_opening->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid opening event schema';END IF;
 IF p_opening ? 'clockObservation' AND (
  jsonb_typeof(p_opening->'clockObservation') IS DISTINCT FROM 'object'
  OR p_opening->'clockObservation'->'reason' IS DISTINCT FROM '"native_decision"'::jsonb
  OR p_opening->'clockObservation'->'acceptedRevision' IS DISTINCT FROM '"0"'::jsonb
  OR jsonb_typeof(p_opening->'clockObservation'->'seatId') IS DISTINCT FROM 'string'
  OR p_opening->'clockObservation'->>'seatId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  OR p_opening->'clockObservation'->>'proofRef' IS DISTINCT FROM p_opening->>'stateRef'
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening->'clockObservation') k WHERE k NOT IN('reason','seatId','acceptedRevision','proofRef'))
 ) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid original opening clock observation';END IF;
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
 IF p_opening ? 'clockObservation' THEN
  SELECT id INTO initial_waiter FROM app.match_seat WHERE match_id=m.id AND id=(p_opening->'clockObservation'->>'seatId')::uuid;
  IF initial_waiter IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening chooser must belong to the copied match roster';END IF;
  IF m.structure='bo3' AND EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=m.id AND c.remaining_ms=0) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='exhausted match bank must settle before another opening';END IF;
 END IF;
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
 UPDATE app.match_clock c SET paused=NOT(c.policy_ref='bo3-overall-bank-v1' AND coalesce(c.match_seat_id=initial_waiter,false) AND c.remaining_ms>0),last_accounted_at=at_time,clock_revision=next_clock,eligibility_receipt_id=CASE WHEN c.policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE c.eligibility_receipt_id END FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 UPDATE app.command_receipt SET normalized_source=coalesce(normalized_source,'{}'::jsonb)||jsonb_build_object('matchId',m.id,'gameId',p_game,'owner',p_owner,'clockValues',app.match_clock_snapshot(m.id)) WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;

CREATE OR REPLACE FUNCTION app.accept_native_opening(p_match uuid,p_basis numeric,p_game uuid,p_owner text,p_opening jsonb,p_native_epoch text,p_runtime_id text,p_program_source_id text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;child app.command_receipt;terms jsonb;reservation jsonb;response jsonb;native_context_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native opening requires read committed retry';END IF;
 IF p_match IS NULL OR p_game IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_match,p_game)
  OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256
  OR NOT app.native_u64_is_canonical(p_native_epoch) OR p_runtime_id IS NULL OR p_runtime_id !~ '^[0-9a-f]{64}$'
  OR p_program_source_id IS NULL OR p_program_source_id !~ '^[0-9a-f]{64}$'
  OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native opening original';END IF;
 terms:=jsonb_build_object('version','native-opening-v1','matchId',p_match,'gameId',p_game,'owner',p_owner,
  'originalBasis',p_basis::text,'opening',p_opening,'nativeEpoch',p_native_epoch,'runtimeId',p_runtime_id,'programSourceId',p_program_source_id);
 receipt:=app.begin_native_source_receipt('native-opening:'||p_game::text,p_game::text,'native_opening_v1',p_game,terms,NULL,NULL);
 -- Exact retained original wins before reservation lookup: it is now active.
 IF receipt.state<>'pending' THEN PERFORM app.check_native_opening_receipt(receipt.id);RETURN receipt.response_payload;END IF;
 -- Preserve exact historical originals before requiring new native semantics.
 IF p_opening->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native opening requires transition event schema';END IF;
 IF jsonb_typeof(p_opening->'contextId') IS DISTINCT FROM 'string' OR p_opening->>'contextId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native opening context';END IF;
 native_context_id:=(p_opening->>'contextId')::uuid;
 IF NOT app.native_artifact_ref_is_valid(p_opening->>'snapshotRef',p_game,native_context_id,'initial')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'stateRef',p_game,native_context_id,'state')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'factsRef',p_game,native_context_id,'facts')
  OR NOT app.native_artifact_ref_is_valid(p_opening->>'eventRef',p_game,native_context_id,'event') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native opening artifacts have wrong scope';END IF;
 reservation:=app.read_native_admission_source(p_game);
 IF reservation->>'status' IS DISTINCT FROM 'success' OR reservation->'data'->>'matchId' IS DISTINCT FROM p_match::text
  OR reservation->'data'->>'matchRevision' IS DISTINCT FROM p_basis::text THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native opening reservation unavailable; retry current source';END IF;
 IF reservation->'data'->>'structure'='bo3' AND NOT(p_opening ? 'clockObservation') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native BO3 opening requires its actual initial waiting observation';END IF;
 response:=app.accept_reserved_game(p_match,p_basis,p_game,p_owner,p_opening);
 SELECT * INTO STRICT child FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text
  AND scope='game_opening_v1' AND request_ref=p_game::text;
 UPDATE app.command_receipt SET normalized_source=terms||jsonb_build_object('childReceiptId',child.id) WHERE id=receipt.id;
 response:=app.finish_account_command(receipt.id,true,response->'data');
 PERFORM app.check_native_opening_receipt(receipt.id);
 RETURN response;
END $$;

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
 IF source.normalized_source ? 'eventSchemaRevision' THEN
  IF source.normalized_source->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid opening creator event schema';END IF;
  original:=original||jsonb_build_object('eventSchemaRevision',source.normalized_source->'eventSchemaRevision');
 END IF;
 IF source.normalized_source ? 'clockObservation' THEN
  original:=original||jsonb_build_object('clockObservation',source.normalized_source->'clockObservation');
  item:=original->'clockObservation';
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR item->'reason' IS DISTINCT FROM '"native_decision"'::jsonb
   OR item->'acceptedRevision' IS DISTINCT FROM '"0"'::jsonb OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string'
   OR item->>'proofRef' IS DISTINCT FROM cp.private_state_ref
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN('reason','seatId','acceptedRevision','proofRef'))
   OR NOT EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND s.id::text=item->>'seatId') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening creator requires exact bound decision source';END IF;
 END IF;
 IF EXISTS(SELECT 1 FROM jsonb_array_elements(clock_values) value JOIN app.match_seat s ON s.id::text=value->>'seatId' JOIN app.match_clock bank ON bank.match_seat_id=s.id
  WHERE value->'paused' IS DISTINCT FROM to_jsonb(NOT(bank.policy_ref='bo3-overall-bank-v1' AND coalesce(s.id::text=original->'clockObservation'->>'seatId',false)))
   OR(value->'paused'='false'::jsonb AND (value->>'remainingMs')::numeric<=0)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening clock snapshot must preserve exact initial waiting eligibility';END IF;
 expected:=jsonb_build_object('match' ,m.id,'game',g.id,'owner',source.normalized_source->>'owner','opening',original);
 SELECT * INTO event FROM app.game_event WHERE source_receipt_id=source.id;
 IF source.principal_key IS DISTINCT FROM 'match-opening:'||m.id::text OR source.request_ref IS DISTINCT FROM g.id::text OR source.payload_hash IS DISTINCT FROM app.auth_json_hash(expected) OR source.normalized_source->>'version' IS DISTINCT FROM 'game-opening-v1' OR source.normalized_source->>'owner' IS NULL OR ctx.id::text IS DISTINCT FROM original->>'contextId' OR cp.accepted_revision<>0 OR cp.private_state_ref IS DISTINCT FROM original->>'stateRef' OR cp.facts_ref IS DISTINCT FROM original->>'factsRef' OR ctx.snapshot_ref IS DISTINCT FROM original->>'snapshotRef' OR ctx.context_revision IS DISTINCT FROM original->>'contextRevision' OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM g.id::text OR source.response_payload->'data'->>'contextId' IS DISTINCT FROM ctx.id::text
 OR (SELECT count(*) FROM app.game_event WHERE source_receipt_id=source.id)<>1 OR event.game_id IS DISTINCT FROM g.id OR event.context_id IS DISTINCT FROM ctx.id OR event.sequence<>0 OR event.accepted_revision<>0 OR event.private_payload_ref IS DISTINCT FROM original->>'eventRef' OR EXISTS(SELECT 1 FROM app.game_activity_fact WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_result WHERE source_event_id=event.id) OR EXISTS(SELECT 1 FROM app.game_participation WHERE game_id=g.id AND accepted_revision=0) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening frame differs from its exact accepted source';END IF;
 RETURN;
 END IF;
 body:=source.normalized_source->'candidate';revision:=cp.accepted_revision;
 IF body ? 'eventSchemaRevision' AND body->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid candidate creator event schema';END IF;
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
 -- The old id::text comparison scanned all Event rows for each source ID.
 -- Preserve canonical-text equality (including malformed/uppercase rejection)
 -- while using the existing UUID primary-key index. Profile retained separately.
 SELECT * INTO event FROM app.game_event WHERE id=CASE
  WHEN app.native_uuid_is_canonical(item#>>'{}') THEN (item#>>'{}')::uuid ELSE NULL END;
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

CREATE OR REPLACE FUNCTION app.apply_accept_host_candidate(p_receipt uuid,p_game uuid,p_owner text,p_epoch numeric,p_basis numeric,p_request uuid,p_candidate jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_game app.game; host app.operational_state; receipt app.command_receipt;
 fingerprint text; context_id uuid; checkpoint_id uuid; event_ids uuid[]:='{}'; event_id uuid; result_id uuid;
 item jsonb; next_sequence numeric; next_revision numeric; next_clock numeric; terminal jsonb;
 accepted_at timestamptz; before_clocks jsonb; timeout jsonb; exhausted uuid[]; losing uuid; inherited uuid; discovered_match uuid; seat_count bigint; clock_count bigint;
BEGIN
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt FOR UPDATE;
 IF receipt.id IS NULL OR receipt.state<>'pending' OR receipt.scope IS DISTINCT FROM 'host_candidate_v1'
  OR receipt.principal_key IS DISTINCT FROM 'host:'||p_game::text OR receipt.target_ref IS DISTINCT FROM p_game::text
  OR receipt.request_ref IS DISTINCT FROM p_request::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('owner',p_owner,'epoch',p_epoch::text,'basis',p_basis::text,'candidate',p_candidate)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='locked accept_host_candidate core requires its preclaimed exact identity';END IF;
 SELECT match_id INTO discovered_match FROM app.game WHERE id=p_game;
 IF discovered_match IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('game','unavailable')); END IF;
 PERFORM id FROM app.match WHERE id=discovered_match FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.match WHERE id=discovered_match AND availability<>'available') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('availability','match recovering'));END IF;
 SELECT * INTO source_game FROM app.game WHERE id=p_game FOR UPDATE;
 IF source_game.id IS NULL OR source_game.match_id IS DISTINCT FROM discovered_match THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='host game parent changed before source fence'; END IF;
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game FOR NO KEY UPDATE;
 IF host.id IS NULL OR host.owner_instance_ref<>p_owner OR host.fence_epoch<>p_epoch OR host.availability<>'available' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('fence','stale or unavailable')); END IF;
 IF source_game.state<>'active' OR source_game.accepted_revision<>p_basis THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or terminal')); END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_candidate) k WHERE k NOT IN ('contextId','stateRef','factsRef','events','facts','clocks','terminal','participation','bankTimeout','nativeEliminations','eventSchemaRevision'))
 OR jsonb_typeof(p_candidate->'contextId') IS DISTINCT FROM 'string'
 OR (p_candidate->>'contextId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(p_candidate->'stateRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'stateRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'factsRef') IS DISTINCT FROM 'string' OR length(btrim(p_candidate->>'factsRef')) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_candidate->'events') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'facts') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'clocks') IS DISTINCT FROM 'array'
 OR jsonb_typeof(p_candidate->'participation') IS DISTINCT FROM 'array' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate artifact fields'; END IF;
 IF p_candidate ? 'eventSchemaRevision' AND p_candidate->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate event schema';END IF;
 IF jsonb_array_length(p_candidate->'events') NOT BETWEEN 1 AND 1024
 OR jsonb_array_length(p_candidate->'facts')>4096 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='candidate exceeds accepted transaction bounds'; END IF;
 context_id:=(p_candidate->>'contextId')::uuid;
 IF NOT EXISTS(SELECT 1 FROM app.game_context WHERE id=context_id AND game_id=p_game) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('context','unavailable')); END IF;
 terminal:=p_candidate->'terminal';
 IF terminal='null'::jsonb THEN terminal:=NULL; END IF;
 IF terminal IS NOT NULL AND (jsonb_typeof(terminal)<>'object'
  OR (terminal->>'outcome') IS NULL OR terminal->>'outcome' NOT IN ('win','draw','no_contest')
  OR jsonb_typeof(terminal->'winnersRef') IS DISTINCT FROM 'string' OR length(btrim(terminal->>'winnersRef')) NOT BETWEEN 1 AND 2048
  OR jsonb_typeof(terminal->'winnerSeatIds') IS DISTINCT FROM 'array'
  OR jsonb_typeof(terminal->'reason') IS DISTINCT FROM 'string' OR length(btrim(terminal->>'reason')) NOT BETWEEN 1 AND 256
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(terminal) k WHERE k NOT IN ('outcome','winnersRef','reason','winnerSeatIds'))) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid terminal candidate'; END IF;
 IF terminal IS NOT NULL THEN
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(terminal->'winnerSeatIds') x WHERE jsonb_typeof(x) IS DISTINCT FROM 'string' OR x#>>'{}' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized winning seat'; END IF;
  PERFORM app.validate_normalized_winners(source_game.match_id,terminal->>'outcome',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x));
 END IF;
 SELECT count(*) INTO seat_count FROM app.match_seat WHERE match_id=source_game.match_id;
 SELECT count(*) INTO clock_count FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id;
 IF seat_count=0 OR clock_count<>seat_count OR jsonb_array_length(p_candidate->'clocks')<>seat_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','complete match clock state required')); END IF;
 FOR item IN SELECT v FROM jsonb_array_elements(p_candidate->'clocks') v ORDER BY v->>'seatId' LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string'
   OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'clockRevision') IS DISTINCT FROM 'string' OR (item->>'clockRevision') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'remainingMs') IS DISTINCT FROM 'string' OR (item->>'remainingMs') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'paused') IS DISTINCT FROM 'boolean'
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','clockRevision','remainingMs','paused','eligibility')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid candidate clock'; END IF;
  PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id
   WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id
    AND c.clock_revision=(item->>'clockRevision')::numeric AND c.remaining_ms>=(item->>'remainingMs')::numeric FOR NO KEY UPDATE OF c;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','stale or invalid')); END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref IN('bo3-overall-bank-v1','bo1-bank-inactive-v1') AND remaining_ms<>(item->>'remainingMs')::numeric) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','send the current bank; elapsed debit is authoritative'));END IF;
  IF NOT (item->>'paused')::boolean AND EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo1-bank-inactive-v1') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','BO1 overall bank is inactive'));END IF;
  IF EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=(item->>'seatId')::uuid AND policy_ref='bo3-overall-bank-v1' AND (NOT (item->>'paused')::boolean OR NOT paused AND terminal IS NULL)) THEN
   IF item->'eligibility' IS NULL OR jsonb_typeof(item->'eligibility') IS DISTINCT FROM 'object' OR (CASE WHEN (item->>'paused')::boolean THEN item->'eligibility'->>'reason' IS DISTINCT FROM 'native_wait' ELSE item->'eligibility'->>'reason' IS DISTINCT FROM 'native_priority' AND item->'eligibility'->>'reason' IS DISTINCT FROM 'native_decision' END) OR(item->'eligibility'->>'reason'='native_decision' AND item->'eligibility'->>'proofRef' IS DISTINCT FROM p_candidate->>'stateRef') OR item->'eligibility'->>'acceptedRevision' IS DISTINCT FROM p_basis::text OR jsonb_typeof(item->'eligibility'->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(item->'eligibility'->>'proofRef')) NOT BETWEEN 1 AND 2048 OR EXISTS(SELECT 1 FROM jsonb_object_keys(item->'eligibility') k WHERE k NOT IN('reason','acceptedRevision','proofRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bank eligibility change requires native priority or wait source at current basis';END IF;
  ELSIF item ? 'eligibility' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='inactive clock cannot claim native priority eligibility';END IF;

 END LOOP;
 IF (SELECT count(DISTINCT v->>'seatId') FROM jsonb_array_elements(p_candidate->'clocks') v)<>seat_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','duplicate seat')); END IF;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id;
 IF terminal IS NULL AND jsonb_array_length(p_candidate->'participation')<>0 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='only terminal candidates carry final participation'; END IF;
 IF terminal IS NOT NULL AND jsonb_array_length(p_candidate->'participation')<>(SELECT count(*) FROM app.match_seat WHERE match_id=source_game.match_id AND controller_kind='human') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='terminal candidate requires every human participation total'; END IF;
 PERFORM app.check_selected_clock_eligibility(c.id) FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1';
 next_revision:=p_basis+1; accepted_at:=clock_timestamp();before_clocks:=app.match_clock_snapshot(source_game.match_id);
 SELECT coalesce(array_agg(c.match_seat_id ORDER BY c.match_seat_id),'{}'::uuid[]) INTO exhausted FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1' AND (c.remaining_ms=0 OR NOT c.paused AND app.clock_elapsed_ms(c.last_accounted_at,accepted_at)>=c.remaining_ms);
 timeout:=p_candidate->'bankTimeout';IF timeout='null'::jsonb THEN timeout:=NULL;END IF;
 IF timeout IS NULL THEN
 IF cardinality(exhausted)>0 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('clocks','overall bank requires its timeout transition'));END IF;
 ELSE
 IF jsonb_typeof(timeout) IS DISTINCT FROM 'object' OR timeout->>'kind' IS DISTINCT FROM 'overall_bank' OR jsonb_typeof(timeout->'seatId') IS DISTINCT FROM 'string' OR timeout->>'seatId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR jsonb_typeof(timeout->'clockRevision') IS DISTINCT FROM 'string' OR timeout->>'clockRevision' !~ '^(0|[1-9][0-9]{0,77})$' OR jsonb_typeof(timeout->'proofRef') IS DISTINCT FROM 'string' OR length(btrim(timeout->>'proofRef')) NOT BETWEEN 1 AND 2048 OR EXISTS(SELECT 1 FROM jsonb_object_keys(timeout) k WHERE k NOT IN('kind','seatId','clockRevision','proofRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized bank-timeout source';END IF;
 losing:=(timeout->>'seatId')::uuid;
 IF cardinality(exhausted)=0 THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='bank timeout is not due at this acceptance boundary';END IF;
 IF exhausted IS DISTINCT FROM ARRAY[losing] OR seat_count<>2 OR NOT EXISTS(SELECT 1 FROM app.match WHERE id=source_game.match_id AND structure='bo3') OR terminal IS NULL OR terminal->>'outcome'<>'win' OR jsonb_array_length(terminal->'winnerSeatIds')<>1 OR terminal->'winnerSeatIds' @> jsonb_build_array(losing::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout requires one exhausted loser and actual sole opposing native winner';END IF;
 PERFORM app.check_selected_clock_eligibility(c.id) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=source_game.match_id AND c.policy_ref='bo3-overall-bank-v1';
 SELECT c.eligibility_receipt_id INTO inherited FROM app.match_clock c WHERE c.match_seat_id=losing AND c.clock_revision=(timeout->>'clockRevision')::numeric AND c.policy_ref='bo3-overall-bank-v1' AND (NOT c.paused OR c.remaining_ms=0 AND app.bank_exhaustion_source(c.eligibility_receipt_id,losing));
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='bank timeout clock basis or inherited exhaustion source differs';END IF;
 timeout:=timeout||jsonb_build_object('priorClockSourceId',inherited);
 END IF;
 UPDATE app.game SET accepted_revision=next_revision,state=CASE WHEN terminal IS NULL THEN 'active' WHEN terminal->>'outcome'='no_contest' THEN 'no_contest' ELSE 'completed' END WHERE id=p_game;
 FOR item IN SELECT v FROM jsonb_array_elements(p_candidate->'clocks') v ORDER BY v->>'seatId' LOOP
  UPDATE app.match_clock SET remaining_ms=CASE WHEN policy_ref='bo3-overall-bank-v1' AND NOT paused THEN greatest(0,remaining_ms-app.clock_elapsed_ms(last_accounted_at,accepted_at)) ELSE (item->>'remainingMs')::numeric END,clock_revision=next_clock,
   paused=CASE WHEN terminal IS NOT NULL THEN true ELSE (item->>'paused')::boolean END,last_accounted_at=accepted_at,eligibility_receipt_id=CASE WHEN policy_ref='bo3-overall-bank-v1' THEN receipt.id ELSE eligibility_receipt_id END
   WHERE match_seat_id=(item->>'seatId')::uuid;
 END LOOP;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision,source_receipt_id)
 VALUES(p_game,context_id,next_revision,p_candidate->>'stateRef',p_candidate->>'factsRef',next_clock,receipt.id) RETURNING id INTO checkpoint_id;
 SELECT coalesce(max(sequence),-1)+1 INTO next_sequence FROM app.game_event WHERE game_id=p_game;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'events') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'string' OR length(btrim(item#>>'{}')) NOT BETWEEN 1 AND 2048 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private event artifact'; END IF;
  INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref,source_receipt_id)
  VALUES(p_game,context_id,next_sequence,next_revision,item#>>'{}',receipt.id) RETURNING id INTO event_id;
  event_ids:=array_append(event_ids,event_id); next_sequence:=next_sequence+1;
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'facts') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'eventIndex') IS DISTINCT FROM 'string'
   OR (item->>'eventIndex') !~ '^(0|[1-9][0-9]{0,3})$' OR (item->>'eventIndex')::integer>=cardinality(event_ids)
   OR jsonb_typeof(item->'accountId') IS DISTINCT FROM 'string' OR (item->>'accountId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'kind') IS DISTINCT FROM 'string' OR length(btrim(item->>'kind')) NOT BETWEEN 1 AND 128
   OR jsonb_typeof(item->'revision') IS DISTINCT FROM 'string' OR length(btrim(item->>'revision')) NOT BETWEEN 1 AND 128
   OR jsonb_typeof(item->'value') IS DISTINCT FROM 'string' OR (item->>'value') !~ '^(0|[1-9][0-9]{0,77})$'
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('eventIndex','accountId','kind','revision','value')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid accepted activity fact'; END IF;
  INSERT INTO app.game_activity_fact(game_id,source_event_id,account_id,kind,value,fact_revision)
  VALUES(p_game,event_ids[(item->>'eventIndex')::integer+1],(item->>'accountId')::uuid,item->>'kind',(item->>'value')::numeric,item->>'revision');
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_candidate->'participation') LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string'
   OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'ownTurns') IS DISTINCT FROM 'string' OR (item->>'ownTurns') !~ '^(0|[1-9][0-9]{0,77})$'
   OR jsonb_typeof(item->'reason') IS DISTINCT FROM 'string' OR length(btrim(item->>'reason')) NOT BETWEEN 1 AND 256
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','ownTurns','reason')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid accepted participation'; END IF;
  INSERT INTO app.game_participation(game_id,match_seat_id,account_id,own_turns,real_game,reason,completed_at,accepted_revision)
  SELECT p_game,s.id,s.account_id,(item->>'ownTurns')::numeric,(item->>'ownTurns')::numeric>=3,item->>'reason',accepted_at,next_revision
  FROM app.match_seat s WHERE s.id=(item->>'seatId')::uuid AND s.match_id=source_game.match_id AND s.controller_kind='human';
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='participation seat is not a human in this match'; END IF;
 END LOOP;
 IF terminal IS NOT NULL THEN
  INSERT INTO app.game_result(game_id,source_event_id,outcome,winners_ref,end_reason,winner_seat_ids,bank_forfeit_receipt_id,bank_forfeit_seat_id)
  VALUES(p_game,event_ids[cardinality(event_ids)],terminal->>'outcome',terminal->>'winnersRef',terminal->>'reason',ARRAY(SELECT x::uuid FROM jsonb_array_elements_text(terminal->'winnerSeatIds') x),CASE WHEN timeout IS NOT NULL THEN receipt.id END,losing) RETURNING id INTO result_id;
  INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state)
  VALUES(source_game.match_id,p_game,result_id::text,'match_lifecycle','game_outcome','pending');
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','host-clock-transition-v1','gameId',p_game,'matchId',source_game.match_id,'basisRevision',p_basis::text,'acceptedRevision',next_revision::text,'acceptedAt',to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'before',before_clocks,'after',app.match_clock_snapshot(source_game.match_id),'owner',p_owner,'epoch',p_epoch::text,'candidate',p_candidate,'eventIds',to_jsonb(event_ids),'bankTimeout',timeout,'gameResultId',result_id) WHERE id=receipt.id;
 PERFORM app.record_native_eliminations(receipt.id);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',checkpoint_id,'revision',next_revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.submit_native_player_command(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor jsonb;parent app.command_receipt;child app.command_receipt;
 native_game_id uuid;native_match_id uuid;native_seat_id uuid;source jsonb;basis_state jsonb;view_receipt app.command_receipt;
 canonical jsonb;ack jsonb;checkpoint_id uuid;normalized jsonb;prepared_valid boolean;is_accept boolean;is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native player commit requires read committed retry';END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  player:=app.lock_session_account(p_subject,p_session);
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_player_original_is_valid(p_original) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 native_game_id:=(p_original->'basis'->>'gameId')::uuid;
 SELECT g.match_id INTO native_match_id FROM app.game g WHERE g.id=native_game_id;
 IF is_human THEN
  SELECT jsonb_build_object('seatId',seat.id,'kind','human','accountId',player.id,'grantId',NULL,'bindingId',NULL,'bindingEpoch',NULL) INTO actor
   FROM app.match_seat seat WHERE seat.match_id=native_match_id AND seat.account_id=player.id AND seat.controller_kind='human';
 ELSE actor:=app.lock_native_controller_actor(p_grant,p_epoch,p_secret_ref,native_match_id);END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 native_seat_id:=(actor->>'seatId')::uuid;
 parent:=app.begin_native_source_receipt('native-seat:'||native_seat_id::text,p_original->>'requestId','native_player_v1',native_game_id,
  jsonb_build_object('actorSeatId',native_seat_id,'original',p_original),player.id,p_grant);
 -- This lookup precedes current basis, host fence, new handles and preparation.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 prepared_valid:=app.native_player_prepared_is_valid(p_prepared,p_original,native_seat_id);
 is_accept:=prepared_valid AND p_prepared->>'version'='native-player-accept-v1';
 IF is_accept AND p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native player acceptance requires transition event schema';END IF;
 BEGIN
  IF is_accept THEN
   child:=app.claim_accept_host_candidate(native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new native original has preexisting canonical child';END IF;
  END IF;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
  IF NOT prepared_valid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='current native original requires valid trusted preparation';END IF;
  IF p_prepared->>'runtimeId' IS DISTINCT FROM source->'opening'->>'runtimeId'
   OR p_prepared->>'programSourceId' IS DISTINCT FROM source->'opening'->>'programSourceId' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation has different original runtime source';END IF;
  SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(p_prepared->>'viewReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
  IF view_receipt.id IS NULL OR view_receipt.target_ref IS DISTINCT FROM native_game_id::text
   OR view_receipt.normalized_source->'basis' IS DISTINCT FROM p_original->'basis'
   OR view_receipt.normalized_source->>'viewerSeatId' IS DISTINCT FROM native_seat_id::text
   OR view_receipt.normalized_source->>'disclosure' IS DISTINCT FROM 'live' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native preparation lacks own exact issued View';END IF;
  PERFORM app.check_native_view_receipt(view_receipt.id);
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,
   'prepared',p_prepared,'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL);
  IF NOT is_accept THEN
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,false,p_prepared->'errors');
  ELSE
   IF p_original->'basis'->>'revision'='18446744073709551615' THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='native revision exhausted';END IF;
   canonical:=app.apply_accept_host_candidate(child.id,native_game_id,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,
    (p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
   IF canonical->>'status' IS DISTINCT FROM 'success' THEN
    IF canonical->'data' ? 'basis' THEN
     basis_state:=app.native_original_basis_state(p_original,app.lock_native_game_source(native_game_id));
     IF basis_state->>'state'='stale' THEN RAISE EXCEPTION USING ERRCODE='SW003',MESSAGE='verified stale native original';END IF;
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canonical basis failure disagrees with locked native source';
    ELSIF canonical->'data' ?| ARRAY['game','context','availability','fence','clocks'] THEN
     RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native operational candidate not accepted';
    ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unexpected canonical native rejection';END IF;
   END IF;
   checkpoint_id:=(canonical->'data'->>'resourceId')::uuid;
   normalized:=normalized||jsonb_build_object('childReceiptId',child.id,'acceptedCheckpointId',checkpoint_id);
   UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
   ack:=app.finish_account_command(parent.id,true,jsonb_build_object('requestId',p_original->>'requestId','actorSeatId',native_seat_id,
    'gameId',native_game_id,'contextId',p_original->'basis'->>'contextId','epoch',p_original->'basis'->>'epoch','revision',canonical->'data'->>'revision'));
  END IF;
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
 EXCEPTION WHEN SQLSTATE 'SW003' THEN
  -- PL/pgSQL variables survive rollback; never reuse a rolled-back child, frame,
  -- response or source. Parent/account/controller scope predates this savepoint.
  child:=NULL;source:=NULL;basis_state:=NULL;view_receipt:=NULL;canonical:=NULL;ack:=NULL;checkpoint_id:=NULL;normalized:=NULL;
  source:=app.lock_native_game_source(native_game_id);
  PERFORM app.recheck_native_player_actor(p_subject,p_session,p_grant,p_epoch,p_secret_ref,actor,native_match_id);
  IF is_accept AND(p_prepared->>'owner' IS DISTINCT FROM source->'native'->'operational'->>'ownerRef'
   OR p_prepared->>'fenceEpoch' IS DISTINCT FROM source->'native'->'operational'->>'fenceEpoch') THEN
   RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native prepared operational fence lost';END IF;
  basis_state:=app.native_original_basis_state(p_original,source);
  IF basis_state->>'state' IS DISTINCT FROM 'stale' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native stale observation changed before rejection fence';END IF;
  normalized:=jsonb_build_object('version','native-player-v1','actorSeatId',native_seat_id,'original',p_original,'prepared',NULL,
   'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',NULL,'acceptedCheckpointId',NULL,'rejectionSource',basis_state->'rejectionSource');
  UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
  ack:=app.finish_account_command(parent.id,false,jsonb_build_object('basis','stale'));
 END;
 RETURN ack;
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object(CASE WHEN is_human THEN 'session' ELSE 'actor' END,CASE WHEN is_human THEN 'unauthorized' ELSE 'unavailable' END));
WHEN SQLSTATE 'SW002' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;

CREATE OR REPLACE FUNCTION app.commit_native_host_input(p_game uuid,p_request uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;source jsonb;canonical jsonb;normalized jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system commit requires read committed retry';END IF;
 PERFORM pg_advisory_xact_lock_shared(hashtextextended('supply-population-cutoff-v1',0));
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.native_host_original_is_valid(p_original)
  OR p_original->'basis'->>'gameId' IS DISTINCT FROM p_game::text THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system original';END IF;
 parent:=app.begin_native_source_receipt('native-system:'||p_game::text,p_request::text,'native_host_input_v1',p_game,p_original,NULL,NULL);
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 IF p_prepared->'candidate'->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='new native system acceptance requires transition event schema';END IF;
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system preparation';END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('owner','fenceEpoch','candidate'))
  OR jsonb_typeof(p_prepared->'owner') IS DISTINCT FROM 'string' OR length(btrim(p_prepared->>'owner')) NOT BETWEEN 1 AND 256
  OR jsonb_typeof(p_prepared->'fenceEpoch') IS DISTINCT FROM 'string' OR p_prepared->>'fenceEpoch' !~ '^(0|[1-9][0-9]{0,77})$'
  OR NOT app.native_candidate_refs_are_valid(p_prepared->'candidate',p_game,(p_original->'basis'->>'contextId')::uuid) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native system candidate source';END IF;
 child:=app.claim_accept_host_candidate(p_game,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,(p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
 IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new system original has preexisting canonical child';END IF;
 source:=app.lock_native_game_source(p_game);
 IF app.native_original_basis_state(p_original,source)->>'state' IS DISTINCT FROM 'current' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system basis changed';END IF;
 IF p_original->>'runtimeId' IS DISTINCT FROM source->'opening'->>'runtimeId' OR p_original->>'programSourceId' IS DISTINCT FROM source->'opening'->>'programSourceId' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native system original runtime differs';END IF;
 IF p_original->'basis'->>'revision'='18446744073709551615' THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='native revision exhausted';END IF;
 canonical:=app.apply_accept_host_candidate(child.id,p_game,p_prepared->>'owner',(p_prepared->>'fenceEpoch')::numeric,(p_original->'basis'->>'revision')::numeric,parent.id,p_prepared->'candidate');
 IF canonical->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native system operational candidate not accepted';END IF;
 normalized:=jsonb_build_object('version','native-host-commit-v1','original',p_original,'prepared',p_prepared,
  'openingReceiptId',source->'opening'->>'receiptId','childReceiptId',child.id,'acceptedCheckpointId',canonical->'data'->>'resourceId');
 UPDATE app.command_receipt SET normalized_source=normalized WHERE id=parent.id;
 RETURN app.finish_account_command(parent.id,true,canonical->'data');
END $$;

-- Reconcile an already retained system original without loading native artifacts
-- or consulting current admission/lease/runtime. This function never creates a
-- command receipt; the existing mutator remains the concurrent acceptance fence.
CREATE FUNCTION app.read_native_system_disposition(p_game uuid,p_request uuid,p_original jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; principal text; scope_name text; opening jsonb;
 native_context uuid; key text; role_name text;
BEGIN
 IF p_game IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_request)
 OR jsonb_typeof(p_original) IS DISTINCT FROM 'object' OR octet_length(p_original::text)>1048576 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained native system original';END IF;
 IF p_original->>'version'='native-host-input-v1' THEN
  IF NOT app.native_host_original_is_valid(p_original) OR p_original->'basis'->>'gameId' IS DISTINCT FROM p_game::text THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained native system input';END IF;
  principal:='native-system:'||p_game::text;scope_name:='native_host_input_v1';
 ELSIF p_original->>'version'='native-opening-v1' THEN
  IF p_request<>p_game OR p_original->>'gameId' IS DISTINCT FROM p_game::text
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_original) k WHERE k NOT IN('version','matchId','gameId','owner','originalBasis','opening','nativeEpoch','runtimeId','programSourceId'))
   OR EXISTS(SELECT 1 FROM unnest(ARRAY['matchId','gameId','owner','originalBasis','nativeEpoch','runtimeId','programSourceId']) k WHERE jsonb_typeof(p_original->k) IS DISTINCT FROM 'string')
   OR NOT app.native_uuid_is_canonical(p_original->>'matchId')
   OR length(btrim(p_original->>'owner')) NOT BETWEEN 1 AND 256
   OR p_original->>'originalBasis' !~ '^(0|[1-9][0-9]{0,77})$'
   OR NOT app.native_u64_is_canonical(p_original->>'nativeEpoch')
   OR p_original->>'runtimeId' !~ '^[0-9a-f]{64}$' OR p_original->>'programSourceId' !~ '^[0-9a-f]{64}$'
   OR jsonb_typeof(p_original->'opening') IS DISTINCT FROM 'object' OR octet_length((p_original->'opening')::text)>16384 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained native opening terms';END IF;
  opening:=p_original->'opening';
  IF NOT app.native_uuid_is_canonical(opening->>'contextId')
   OR jsonb_typeof(opening->'contextId') IS DISTINCT FROM 'string'
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(opening) k WHERE k NOT IN('contextId','snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef','clockObservation','eventSchemaRevision'))
   OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(opening->k) IS DISTINCT FROM 'string' OR length(btrim(opening->>k)) NOT BETWEEN 1 AND 2048)
   OR (opening ? 'eventSchemaRevision' AND opening->'eventSchemaRevision' IS DISTINCT FROM '"native-transition-v1"'::jsonb) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained native opening artifacts';END IF;
  native_context:=(opening->>'contextId')::uuid;
  FOR key,role_name IN SELECT * FROM (VALUES('snapshotRef','initial'),('stateRef','state'),('factsRef','facts'),('eventRef','event')) AS roles(key_name,role_value) LOOP
   IF NOT app.native_artifact_ref_is_valid(opening->>key,p_game,native_context,role_name) THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained opening artifact scope';END IF;
  END LOOP;
  IF opening ? 'clockObservation' AND (
    jsonb_typeof(opening->'clockObservation') IS DISTINCT FROM 'object'
    OR opening->'clockObservation'->'reason' IS DISTINCT FROM '"native_decision"'::jsonb
    OR opening->'clockObservation'->'acceptedRevision' IS DISTINCT FROM '"0"'::jsonb
    OR jsonb_typeof(opening->'clockObservation'->'seatId') IS DISTINCT FROM 'string'
    OR NOT app.native_uuid_is_canonical(opening->'clockObservation'->>'seatId')
    OR opening->'clockObservation'->>'proofRef' IS DISTINCT FROM opening->>'stateRef'
    OR EXISTS(SELECT 1 FROM jsonb_object_keys(opening->'clockObservation') k WHERE k NOT IN('reason','seatId','acceptedRevision','proofRef'))
  ) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid retained opening clock observation';END IF;
  principal:='native-opening:'||p_game::text;scope_name:='native_opening_v1';
 ELSE RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='unsupported retained native system original';
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key=principal AND scope=scope_name AND request_ref=p_request::text;
 IF receipt.id IS NULL THEN
  RETURN jsonb_build_object('status','success','data',jsonb_build_object('state','unknown','response',NULL));END IF;
 IF receipt.target_ref IS DISTINCT FROM p_game::text OR receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(p_original) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='retained native original is not terminal';END IF;
 IF scope_name='native_opening_v1' THEN PERFORM app.check_native_opening_receipt(receipt.id);
 ELSE PERFORM app.check_native_commit_receipt(receipt.id);END IF;
 IF receipt.state NOT IN('accepted','rejected') OR jsonb_typeof(receipt.response_payload) IS DISTINCT FROM 'object'
  OR (receipt.state='accepted' AND receipt.response_payload->>'status' IS DISTINCT FROM 'success')
  OR (receipt.state='rejected' AND receipt.response_payload->>'status' IS DISTINCT FROM 'fail') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retained native disposition is inconsistent';END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('state',receipt.state,'response',receipt.response_payload));
END $$;

-- A page contains whole immutable checkpoint groups through one selected anchor.
-- A newer current tip cannot change this prefix. Private references are not
-- participant authorization, native semantics, or public artifact delivery.
CREATE FUNCTION app.read_native_transition_sources(p_game uuid,p_context uuid,p_anchor_checkpoint uuid,p_after_revision numeric,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source jsonb;anchor app.game_checkpoint;checkpoint_row app.game_checkpoint;
 creator app.command_receipt;body jsonb;marker jsonb;events jsonb;groups jsonb:='[]'::jsonb;
 last_revision numeric;last_sequence numeric;next_revision numeric;result jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_game IS NULL OR p_context IS NULL OR p_anchor_checkpoint IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_anchor_checkpoint)
  OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32
  OR (p_after_revision IS NOT NULL AND p_after_revision::text !~ '^(0|[1-9][0-9]{0,77})$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded anchored native history request required';END IF;
 source:=app.read_native_game_source(p_game);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'contextId' IS DISTINCT FROM p_context::text THEN RETURN unavailable;END IF;
 SELECT * INTO anchor FROM app.game_checkpoint WHERE id=p_anchor_checkpoint AND game_id=p_game AND context_id=p_context
  AND accepted_revision<=(source->'data'->>'acceptedRevision')::numeric AND source_receipt_id IS NOT NULL;
 IF anchor.id IS NULL THEN RETURN unavailable;END IF;
 PERFORM app.check_native_creator_frame(anchor.source_receipt_id);
 IF p_after_revision IS NOT NULL AND (p_after_revision>anchor.accepted_revision OR NOT EXISTS(
  SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context AND accepted_revision=p_after_revision)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native history cursor is outside its anchor';END IF;
 SELECT max(sequence) INTO last_sequence FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND accepted_revision<=anchor.accepted_revision;
 IF last_sequence IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history anchor has no event prefix';END IF;
 FOR checkpoint_row IN SELECT * FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
   AND accepted_revision<=anchor.accepted_revision AND (p_after_revision IS NULL OR accepted_revision>p_after_revision)
   ORDER BY accepted_revision LIMIT p_limit LOOP
  PERFORM app.check_native_creator_frame(checkpoint_row.source_receipt_id);
  SELECT * INTO STRICT creator FROM app.command_receipt WHERE id=checkpoint_row.source_receipt_id;
  body:=CASE WHEN creator.scope='game_opening_v1' THEN creator.normalized_source ELSE creator.normalized_source->'candidate' END;
  -- check_native_creator_frame rejects explicit null/unknown raw markers before
  -- ->> projects historical absence to the nullable inspection representation.
  marker:=CASE WHEN body ? 'eventSchemaRevision' THEN body->'eventSchemaRevision' ELSE 'null'::jsonb END;
  SELECT coalesce(jsonb_agg(jsonb_build_object(
    'eventId',e.id,'sequence',e.sequence::text,'acceptedRevision',e.accepted_revision::text,
    'privatePayloadRef',e.private_payload_ref,'acceptedAt',to_char(e.accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'creatorReceiptId',e.source_receipt_id,'facts',(
      SELECT coalesce(jsonb_agg(jsonb_build_object('accountId',f.account_id,'kind',f.kind,'factRevision',f.fact_revision,'value',f.value::text)
        ORDER BY f.account_id,f.kind,f.fact_revision,f.id),'[]'::jsonb)
      FROM app.game_activity_fact f WHERE f.source_event_id=e.id)) ORDER BY e.sequence),'[]'::jsonb)
    INTO events FROM app.game_event e WHERE e.game_id=p_game AND e.context_id=p_context AND e.source_receipt_id=creator.id;
  IF jsonb_array_length(events)=0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history checkpoint has no exact creator events';END IF;
  groups:=groups||jsonb_build_array(jsonb_build_object('checkpointId',checkpoint_row.id,'acceptedRevision',checkpoint_row.accepted_revision::text,
    'stateRef',checkpoint_row.private_state_ref,'factsRef',checkpoint_row.facts_ref,'creatorReceiptId',creator.id,
    'eventSchemaRevision',marker,'events',events));
  last_revision:=checkpoint_row.accepted_revision;
 END LOOP;
 IF last_revision IS NOT NULL AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
  AND accepted_revision>last_revision AND accepted_revision<=anchor.accepted_revision) THEN next_revision:=last_revision;END IF;
 result:=jsonb_build_object('status','success','data',jsonb_build_object('anchor',jsonb_build_object(
  'gameId',p_game,'contextId',p_context,'anchorCheckpointId',anchor.id,'anchorStateRef',anchor.private_state_ref,
  'anchorCreatorReceiptId',anchor.source_receipt_id,'throughAcceptedRevision',anchor.accepted_revision::text,'throughEventSequence',last_sequence::text),
  'groups',groups,'nextAfterRevision',next_revision::text));
 IF octet_length(result::text)>8388608 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native history response exceeds bounded source budget';END IF;
 RETURN result;
END $$;

REVOKE ALL ON FUNCTION app.read_native_system_disposition(uuid,uuid,jsonb),
 app.read_native_transition_sources(uuid,uuid,uuid,numeric,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_native_system_disposition(uuid,uuid,jsonb),
 app.read_native_transition_sources(uuid,uuid,uuid,numeric,integer) TO service_role;
COMMIT;
