BEGIN;

CREATE FUNCTION app.read_commander_handoff_page(p_release uuid,p_after uuid,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander handoff page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT handoff.id FROM app.durable_handoff handoff
     JOIN app.match m ON m.id=handoff.match_id
     JOIN app.format f ON f.id=m.format_id AND f.key='commander'
     WHERE m.accepted_release_id=p_release AND handoff.state='pending'
       AND (handoff.destination,handoff.kind) IN (
         ('game_host','ready_match'),('game_host','reserved_game'),
         ('match_lifecycle','game_outcome'),('match_settlement','match_outcome'))
       AND (p_after IS NULL OR handoff.id>p_after)
     ORDER BY handoff.id LIMIT p_limit
   ) job
 ),'[]'::jsonb));
END $$;
REVOKE ALL ON FUNCTION app.read_commander_handoff_page(uuid,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_commander_handoff_page(uuid,uuid,integer) TO service_role;

-- Keep the existing constructed worker away from reservations pinned to the Commander runtime.
CREATE OR REPLACE FUNCTION app.read_native_match_delivery_page(p_owner text,p_lane text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_owner IS NULL OR btrim(p_owner)='' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery page'; END IF;
 IF p_lane='handoff' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT h.id,jsonb_build_object('id',h.id,'kind',h.kind,'gameId',h.game_id) body FROM app.durable_handoff h
   JOIN app.match m ON m.id=h.match_id
   JOIN app.format f ON f.id=m.format_id
   WHERE f.key<>'commander' AND h.state='pending' AND h.kind IN ('ready_match','reserved_game','game_outcome','match_outcome','between_deadline')
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

COMMIT;
