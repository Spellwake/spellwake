BEGIN;
-- Appending host entropy rebinds the same pending native opening decision.
-- Carry its expiry only through a linked accepted child naming the same seat.
CREATE OR REPLACE FUNCTION app.native_response_wait_at(p_game uuid,p_revision numeric)
RETURNS TABLE(seat_id uuid,decision_ref text,expires_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 WITH RECURSIVE waits(rev,seat,proof,expiry) AS (
  SELECT 0::numeric,w.seat_id,w.decision_ref,w.expires_at
  FROM app.native_opening_response_wait(p_game) w
  UNION ALL
  SELECT cp.accepted_revision,s.id,cp.private_state_ref,
   CASE WHEN s.id=w.seat AND (original.scope='native_host_input_v1'
     OR original.normalized_source->>'actorSeatId'<>w.seat::text)
    THEN w.expiry ELSE app.financial_instant(child.normalized_source->>'acceptedAt')+interval '120 seconds' END
  FROM waits w JOIN app.game_checkpoint cp ON cp.game_id=p_game AND cp.accepted_revision=w.rev+1
  JOIN app.command_receipt child ON child.id=cp.source_receipt_id AND child.scope='host_candidate_v1' AND child.state='accepted'
  JOIN app.command_receipt original ON original.id=child.request_ref::uuid AND original.state='accepted'
   AND original.target_ref=p_game::text AND original.normalized_source->>'childReceiptId'=child.id::text
  JOIN app.game g ON g.id=p_game
  JOIN app.match_seat s ON s.match_id=g.match_id AND s.id::text=child.normalized_source->'candidate'->>'openingResponseSeatId'
  WHERE w.rev<p_revision AND (
    (original.scope='native_player_v1' AND (
      (original.normalized_source->'original'->'action'->>'kind'='choose' AND original.normalized_source->>'actorSeatId'=w.seat::text)
      OR (original.normalized_source->'original'->'action'->>'kind'='concede' AND
       (original.normalized_source->>'actorSeatId'=w.seat::text OR s.id=w.seat))))
    OR (original.scope='native_host_input_v1' AND original.normalized_source->'original'->>'kind'='entropy'
        AND s.id=w.seat)
    OR (original.scope='native_host_input_v1' AND original.normalized_source->'original'->>'kind'='clock'
        AND child.normalized_source->'candidate'->'nativeEliminations'->0->>'seatId'=w.seat::text AND s.id<>w.seat))
 )
 SELECT seat,proof,expiry FROM waits WHERE rev=p_revision
$$;
COMMIT;
