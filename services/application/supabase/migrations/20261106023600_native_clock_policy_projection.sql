BEGIN;
-- Server-only observation of one accepted frontier. STABLE preserves the calling
-- query's MVCC snapshot across all reads and creator checks. This takes no locks:
-- a later command must still revalidate its operational fence and accepted basis.
CREATE OR REPLACE FUNCTION app.read_native_game_source(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE g app.game; m app.match; ctx app.game_context; original app.game_checkpoint;
 current_checkpoint app.game_checkpoint; operational app.operational_state;
 seats jsonb; policy_clocks jsonb; seat_count bigint; last_sequence numeric;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_game IS NULL OR p_game='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN unavailable;END IF;
 SELECT * INTO g FROM app.game WHERE id=p_game;
 IF g.id IS NULL OR g.state NOT IN ('active','completed','no_contest') THEN RETURN unavailable;END IF;
 SELECT * INTO m FROM app.match WHERE id=g.match_id;
 -- Reuse the read-only admission/start predicates, not the locking frontier
 -- validator. The original room need not remain live for historical games.
 IF m.id IS NULL OR m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL
 OR m.admission_qualification_ref IS NULL OR length(btrim(m.admission_qualification_ref))=0
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.principal_key='admission:'||m.id::text
   AND source.scope='match_decks_v1' AND source.target_ref=m.id::text AND source.state='accepted'
   AND source.response_payload->'data'->>'releaseId'=m.accepted_release_id::text
   AND source.response_payload->'data'->>'qualificationRef'=m.admission_qualification_ref)
 OR NOT EXISTS(SELECT 1 FROM app.game game0 JOIN app.command_receipt source
   ON source.scope='room_start_v1' AND source.principal_key='room-start:'||m.room_id::text
   AND source.target_ref=m.room_id::text AND source.state='accepted'
   AND source.response_payload->'data'->>'resourceId'=m.id::text
   AND source.response_payload->'data'->>'gameId'=game0.id::text
   WHERE game0.match_id=m.id AND game0.game_index=0) THEN RETURN unavailable;END IF;
 SELECT count(*),jsonb_agg(jsonb_build_object('seatId',s.id,'seatIndex',s.seat_index::text,
   'accountId',s.account_id,'controllerKind',s.controller_kind,'sourceRevisionId',s.source_revision_id,
   'deckSnapshotRef',s.deck_snapshot_ref) ORDER BY s.seat_index) INTO seat_count,seats
 FROM app.match_seat s WHERE s.match_id=m.id;
 IF seat_count NOT BETWEEN 2 AND 6 OR EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id
   AND (s.deck_accepted_at IS NULL OR s.source_revision_id IS NULL OR s.deck_snapshot_ref IS NULL)) THEN RETURN unavailable;END IF;
 SELECT * INTO ctx FROM app.game_context WHERE game_id=g.id;
 -- Pre-native historical rows are not promoted into native sources.
 IF ctx.id IS NULL THEN RETURN unavailable;END IF;
 SELECT * INTO original FROM app.game_checkpoint WHERE game_id=g.id AND accepted_revision=0;
 SELECT * INTO current_checkpoint FROM app.game_checkpoint WHERE game_id=g.id ORDER BY accepted_revision DESC LIMIT 1;
 SELECT * INTO operational FROM app.operational_state WHERE game_id=g.id;
 IF ctx.release_id IS DISTINCT FROM m.accepted_release_id OR ctx.profile_id IS DISTINCT FROM m.profile_id
 OR original.id IS NULL OR original.context_id IS DISTINCT FROM ctx.id OR original.source_receipt_id IS NULL
 OR current_checkpoint.id IS NULL OR current_checkpoint.context_id IS DISTINCT FROM ctx.id
 OR current_checkpoint.accepted_revision IS DISTINCT FROM g.accepted_revision OR current_checkpoint.source_receipt_id IS NULL
 OR operational.id IS NULL OR (SELECT count(*) FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=m.id)<>seat_count
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native source frontier is incomplete or inconsistent';END IF;
 PERFORM app.check_native_creator_frame(original.source_receipt_id);
 IF current_checkpoint.id<>original.id THEN PERFORM app.check_native_creator_frame(current_checkpoint.source_receipt_id);END IF;
 SELECT max(sequence) INTO last_sequence FROM app.game_event WHERE game_id=g.id AND context_id=ctx.id;
 IF last_sequence IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted native source has no event frontier';END IF;
 -- Enrich this observer only. The canonical helper is retained unchanged for
 -- historical receipt/creator/incident comparisons and supplies the five fields.
 SELECT coalesce(jsonb_agg(entry.value||jsonb_build_object('policyRef',c.policy_ref)
   ORDER BY c.match_seat_id),'[]'::jsonb) INTO policy_clocks
 FROM jsonb_array_elements(app.match_clock_snapshot(m.id)) AS entry(value)
 JOIN app.match_clock c ON c.match_seat_id=(entry.value->>'seatId')::uuid
 JOIN app.match_seat seat ON seat.id=c.match_seat_id AND seat.match_id=m.id;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object(
   'version','native-game-source-v1','gameId',g.id,'matchId',m.id,'contextId',ctx.id,
   'releaseId',ctx.release_id,'profileId',ctx.profile_id,'gameState',g.state,'matchState',m.state,
   'matchAvailability',m.availability,'acceptedRevision',g.accepted_revision::text,
   'contextRevision',ctx.context_revision,'initialRef',ctx.snapshot_ref,
   'checkpoint',jsonb_build_object('id',current_checkpoint.id,'acceptedRevision',current_checkpoint.accepted_revision::text,
     'stateRef',current_checkpoint.private_state_ref,'clockRevision',current_checkpoint.clock_revision::text,
     'creatorReceiptId',current_checkpoint.source_receipt_id),
   'operational',jsonb_build_object('ownerRef',operational.owner_instance_ref,'fenceEpoch',operational.fence_epoch::text,
     'revision',operational.revision::text,'availability',operational.availability,
     'recoveryCheckpointId',operational.recovery_checkpoint_id),
   'seats',seats,'matchClocks',policy_clocks,'lastEventSequence',last_sequence::text));
END $$;
COMMIT;
