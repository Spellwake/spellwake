BEGIN;

-- Read only the caller's own room or accepted match seat. Match room IDs and
-- retired room slots remain unavailable through ordinary table grants.
CREATE FUNCTION app.read_room_game(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  handoff jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  SELECT jsonb_build_object(
    'state', 'admitted', 'accountId', actor, 'roomId', p_room,
    'matchId', m.id, 'seatId', seat.id,
    'game', CASE WHEN g.id IS NOT NULL THEN jsonb_build_object('id', g.id, 'state', g.state, 'contextId', context.id) END
  ) INTO handoff
  FROM app.match m
  JOIN app.match_seat seat ON seat.match_id = m.id AND seat.account_id = actor AND seat.controller_kind = 'human'
  LEFT JOIN app.game g ON g.match_id = m.id AND g.id = coalesce(m.reserved_game_id, m.current_game_id)
  LEFT JOIN app.game_context context ON context.game_id = g.id
  WHERE m.room_id = p_room AND app.can_read_match(m.id);

  IF handoff IS NULL THEN
    SELECT jsonb_build_object('state', 'waiting', 'accountId', actor, 'roomId', r.id, 'roomState', r.state)
    INTO handoff
    FROM app.room r
    JOIN app.room_slot slot ON slot.room_id = r.id AND slot.account_id = actor
      AND slot.controller_kind = 'human' AND slot.retired_at IS NULL
    WHERE r.id = p_room AND r.state IN ('lobby', 'countdown')
      AND EXISTS (SELECT 1 FROM app.account a WHERE a.id = actor AND a.state = 'active');
  END IF;

  IF handoff IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  RETURN jsonb_build_object('status', 'success', 'data', handoff);
END $$;

REVOKE ALL ON FUNCTION app.read_room_game(uuid) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_room_game(uuid) TO authenticated;

COMMIT;
