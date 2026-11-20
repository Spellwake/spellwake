BEGIN;
CREATE OR REPLACE FUNCTION app.guard_native_response_deadline() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE src jsonb:=NEW.normalized_source; frame jsonb:=src->'prepared'->'candidate';
 item jsonb; timing record; survivors uuid[]; ingame boolean:=false; child app.command_receipt; g uuid:=NEW.target_ref::uuid;
BEGIN
 IF NOT (frame ? 'nativeEliminations') OR frame->'nativeEliminations' IN ('null'::jsonb,'[]'::jsonb) THEN RETURN NEW; END IF;
 IF src->'original'->>'kind' IS DISTINCT FROM 'clock'
  OR jsonb_array_length(frame->'nativeEliminations')<>1
  OR (frame->'bankTimeout' IS NOT NULL AND frame->'bankTimeout'<>'null'::jsonb) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiplayer response requires a supported native wait';
 END IF;
 SELECT * INTO timing FROM app.native_response_wait_for_fence(g,(src->'original'->'basis'->>'revision')::numeric,(src->'prepared'->>'fenceEpoch')::numeric);
 IF NOT FOUND OR NOT EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=g AND availability='available' AND fence_epoch=(src->'prepared'->>'fenceEpoch')::numeric) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='multiplayer opening interval unavailable';
 END IF;
 item:=frame->'nativeEliminations'->0;
 SELECT * INTO STRICT child FROM app.command_receipt WHERE id=(src->>'childReceiptId')::uuid;
 SELECT ARRAY(SELECT v::uuid FROM jsonb_array_elements_text(c.normalized_source->'candidate'->'turnResponse'->'remainingSeatIds') v
       WHERE v<>timing.seat_id::text ORDER BY v)
 INTO survivors FROM app.game_checkpoint cp JOIN app.command_receipt c ON c.id=cp.source_receipt_id
 WHERE cp.game_id=g AND cp.accepted_revision=(src->'original'->'basis'->>'revision')::numeric
  AND c.normalized_source->'candidate' ? 'turnResponse';
 IF NOT FOUND THEN
  SELECT ARRAY(SELECT v FROM unnest(a.remaining_seats) v WHERE v<>timing.seat_id ORDER BY v) INTO survivors
   FROM app.game_checkpoint cp JOIN app.native_response_adoption a ON a.checkpoint_id=cp.id
   WHERE cp.game_id=g AND cp.accepted_revision=(src->'original'->'basis'->>'revision')::numeric;
 END IF;
 ingame:=FOUND;
 IF NOT ingame THEN
  survivors:=app.native_opening_other_survivors(g,(src->'original'->'basis'->>'revision')::numeric,timing.seat_id);
 END IF;
 IF cardinality(survivors)=0 OR
   (cardinality(survivors)=1 AND (frame->'terminal'->>'reason' IS DISTINCT FROM 'native_response_timeout'
    OR frame->'terminal'->>'outcome' IS DISTINCT FROM 'win'
    OR frame->'terminal'->'winnerSeatIds' IS DISTINCT FROM to_jsonb(survivors)))
   OR (cardinality(survivors)>1 AND frame->>'terminal' IS NOT NULL AND NOT (
      ingame AND frame->'terminal'->>'reason' IS NOT DISTINCT FROM 'native_checked_losses'
      AND frame->'terminal'->>'winnersRef' IS NOT DISTINCT FROM frame->>'factsRef'
      AND frame->>'factsRef' IS NOT NULL
      AND ((frame->'terminal'->>'outcome' IS NOT DISTINCT FROM 'draw' AND frame->'terminal'->'winnerSeatIds'='[]'::jsonb)
       OR (frame->'terminal'->>'outcome' IS NOT DISTINCT FROM 'win'
           AND jsonb_array_length(frame->'terminal'->'winnerSeatIds')=1
           AND (frame->'terminal'->'winnerSeatIds'->>0)::uuid=ANY(survivors))))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening timeout must preserve its remaining seats or award the last survivor';END IF;
 IF item->>'sourceRevision' IS DISTINCT FROM src->'original'->'basis'->>'revision' OR item->>'reason' IS DISTINCT FROM 'native_response_timeout'
  OR item->>'seatId' IS DISTINCT FROM timing.seat_id::text
  OR item->>'decisionRef' IS DISTINCT FROM ('multiplayer-response-v1:'||timing.decision_ref)
  OR item->>'expiryRef' IS DISTINCT FROM src->'original'->>'inputRef'
  OR app.financial_instant(item->>'expiredAt') IS DISTINCT FROM timing.expires_at
  OR app.financial_instant(child.normalized_source->>'acceptedAt') < timing.expires_at THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiplayer timeout does not match the due opening interval';
 END IF;
 RETURN NEW;
END $$;

COMMIT;
