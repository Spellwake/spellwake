BEGIN;

-- The cursor lets later rooms advance even when older admissions need repair.
CREATE TABLE app.countdown_dispatch_cursor (
  singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
  scan_through timestamptz,
  due_at timestamptz NOT NULL DEFAULT '-infinity',
  room_id uuid NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000'
);
INSERT INTO app.countdown_dispatch_cursor DEFAULT VALUES;
ALTER TABLE app.countdown_dispatch_cursor ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.countdown_dispatch_cursor FORCE ROW LEVEL SECURITY;
REVOKE ALL ON app.countdown_dispatch_cursor FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

CREATE INDEX room_countdown_due_idx ON app.room (countdown_at, id) WHERE state = 'countdown';
CREATE INDEX command_receipt_countdown_basis_idx
  ON app.command_receipt (target_ref, (response_payload #>> '{data,revision}'))
  WHERE state = 'accepted' AND scope IN ('qualified_countdown_intent_v1', 'qualified_ready_intent_v1');

-- Run as privileged postgres in a standalone Cron transaction. Reservation
-- and all room and member locks remain in the existing canonical command.
CREATE FUNCTION app.dispatch_validated_countdowns(p_limit integer)
RETURNS jsonb LANGUAGE plpgsql
SET search_path = '' SET lock_timeout = '250ms' AS $$
DECLARE
  position app.countdown_dispatch_cursor;
  candidate record;
  cutoff timestamptz;
  outcome jsonb;
  examined integer := 0;
  attempted integer := 0;
  reserved integer := 0;
  rejected integer := 0;
  deferred integer := 0;
  failed integer := 0;
BEGIN
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 8 THEN
    RAISE EXCEPTION USING ERRCODE='22023', MESSAGE='invalid countdown dispatch limit';
  END IF;

  SELECT * INTO position FROM app.countdown_dispatch_cursor WHERE singleton FOR UPDATE SKIP LOCKED;
  IF NOT FOUND THEN
    RETURN jsonb_build_object('state', 'busy');
  END IF;
  cutoff := coalesce(position.scan_through, statement_timestamp());
  position.scan_through := cutoff;

  FOR candidate IN
    WITH due_rooms AS MATERIALIZED (
      SELECT id, revision, countdown_at FROM app.room
      WHERE state = 'countdown' AND countdown_at <= cutoff
        AND (countdown_at, id) > (position.due_at, position.room_id)
      ORDER BY countdown_at, id
      LIMIT p_limit
    )
    SELECT room.id, room.revision, room.countdown_at, original.id AS receipt_id
    FROM due_rooms room
    LEFT JOIN LATERAL (
      SELECT receipt.id FROM app.command_receipt receipt
      WHERE receipt.state = 'accepted' AND receipt.target_ref = room.id::text
        AND receipt.scope IN ('qualified_countdown_intent_v1', 'qualified_ready_intent_v1')
        AND receipt.response_payload #>> '{data,revision}' = room.revision::text
        AND receipt.response_payload #>> '{data,resourceId}' = room.id::text
        AND receipt.normalized_source #>> '{sourceTerms,room,id}' = room.id::text
        AND receipt.normalized_source #>> '{sourceTerms,room,revision}' = (room.revision - 1)::text
        AND ((receipt.scope = 'qualified_ready_intent_v1' AND receipt.normalized_source ? 'roomStartPolicy')
          OR (receipt.scope = 'qualified_countdown_intent_v1' AND receipt.normalized_source ? 'constructedPolicy'
            AND receipt.normalized_source #>> '{sourceTerms,room,state}' = 'lobby'))
      ORDER BY receipt.id
      LIMIT 1
    ) original ON true
    ORDER BY room.countdown_at, room.id
  LOOP
    examined := examined + 1;
    position.due_at := candidate.countdown_at;
    position.room_id := candidate.id;
    CONTINUE WHEN candidate.receipt_id IS NULL;
    attempted := attempted + 1;
    BEGIN
      outcome := app.start_validated_countdown(candidate.id, candidate.revision, candidate.receipt_id);
      -- Check reservation constraints inside this candidate's subtransaction.
      -- A sweep attempts one reservation, preserving normal deferred semantics.
      SET CONSTRAINTS ALL IMMEDIATE;
      IF outcome->>'status' = 'success' THEN
        reserved := reserved + 1;
      ELSE
        rejected := rejected + 1;
      END IF;
    EXCEPTION
      WHEN serialization_failure OR deadlock_detected OR lock_not_available THEN
        deferred := deferred + 1;
      WHEN OTHERS THEN
        failed := failed + 1;
        RAISE WARNING 'Countdown reservation failed for room % (SQLSTATE %)', candidate.id, SQLSTATE;
    END;
    EXIT;
  END LOOP;

  -- A fixed cutoff makes every pass finite, including under a busy intake.
  -- Reset after the last page to retry deferred and newly due rooms.
  IF attempted = 0 AND examined < p_limit THEN
    position.scan_through := NULL;
    position.due_at := '-infinity';
    position.room_id := '00000000-0000-0000-0000-000000000000';
  END IF;
  UPDATE app.countdown_dispatch_cursor
    SET scan_through = position.scan_through, due_at = position.due_at, room_id = position.room_id
    WHERE singleton AND (scan_through, due_at, room_id)
      IS DISTINCT FROM (position.scan_through, position.due_at, position.room_id);

  RETURN jsonb_build_object('state', 'finished', 'examined', examined, 'attempted', attempted, 'reserved', reserved,
    'rejected', rejected, 'deferred', deferred, 'failed', failed);
END $$;
REVOKE ALL ON FUNCTION app.dispatch_validated_countdowns(integer)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

COMMIT;
