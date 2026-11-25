-- Stage 1.3 (Phase 3): Enable discriminated custom-card faceCard, deck customEntries/hasCustomCards,
-- and room allowCustomCards/shareHostDecks/hostLentDecks/deckName read fields.
--
-- Rollout note: In live environments, run this migration strictly AFTER deploying the Stage 4
-- application_api binary (which handles both legacy and new read shapes). In fresh dev/test
-- databases, all three migrations apply sequentially cleanly.
BEGIN;

CREATE OR REPLACE FUNCTION app.deck_face_card_json(
  p_revision uuid,
  p_face uuid,
  p_custom_face uuid DEFAULT NULL
) RETURNS jsonb
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = '' AS $$
  WITH candidates AS (
    SELECT
      'official'::text AS kind,
      e.definition_id,
      e.printing_id,
      NULL::uuid AS custom_rev_id,
      c.name AS sort_name,
      CASE e.section WHEN 'commander' THEN 0 WHEN 'companion' THEN 1 WHEN 'main' THEN 2 ELSE 3 END AS section_rank,
      (p_face IS NOT NULL AND e.definition_id = p_face) AS is_selected
    FROM app.deck_entry e
    JOIN app.card_definition c ON c.id = e.definition_id
    WHERE e.revision_id = p_revision AND e.quantity > 0

    UNION ALL

    SELECT
      'custom'::text AS kind,
      NULL::uuid AS definition_id,
      NULL::uuid AS printing_id,
      ce.custom_card_revision_id AS custom_rev_id,
      cr.name AS sort_name,
      CASE ce.section WHEN 'commander' THEN 0 WHEN 'companion' THEN 1 WHEN 'main' THEN 2 ELSE 3 END AS section_rank,
      (p_custom_face IS NOT NULL AND ce.custom_card_revision_id = p_custom_face) AS is_selected
    FROM app.deck_custom_entry ce
    JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
    WHERE ce.revision_id = p_revision AND ce.quantity > 0
  ),
  chosen AS (
    SELECT * FROM candidates
    ORDER BY is_selected DESC, section_rank ASC, sort_name ASC, COALESCE(definition_id, custom_rev_id) ASC, printing_id NULLS LAST
    LIMIT 1
  )
  SELECT CASE
    WHEN chosen.kind = 'official' THEN jsonb_build_object(
      'kind', 'official',
      'definitionId', chosen.definition_id,
      'artUrl', app.card_art_url(chosen.definition_id, chosen.printing_id)
    )
    ELSE jsonb_build_object(
      'kind', 'custom',
      'customCardRevisionId', chosen.custom_rev_id,
      'artPath', '/v1/custom-cards/revisions/' || chosen.custom_rev_id::text || '/art/0'
    )
  END
  FROM chosen
$$;

CREATE OR REPLACE FUNCTION app.read_my_deck(p_deck uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path = '' AS $$
DECLARE actor uuid := app.current_account_id(); authority jsonb; result jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT jsonb_build_object(
    'deck', jsonb_build_object(
      'id', d.id,
      'name', d.name,
      'mode', d.mode,
      'currentRevisionId', d.current_revision_id,
      'format', jsonb_build_object('id', f.id, 'key', f.key, 'name', f.name),
      'appearance', jsonb_build_object('backId', d.back_id, 'sleeveId', d.sleeve_id, 'diceId', d.dice_id, 'playmatId', d.playmat_id),
      'faceCard', app.deck_face_card_json(r.id, d.face_definition_id, d.face_custom_card_revision_id),
      'hasCustomCards', CASE WHEN r.id IS NULL THEN false ELSE EXISTS (SELECT 1 FROM app.deck_custom_entry ce WHERE ce.revision_id = r.id) END
    ),
    'revision', CASE WHEN r.id IS NULL THEN NULL ELSE jsonb_build_object(
      'id', r.id,
      'number', r.revision_number::text,
      'entries', (
        SELECT COALESCE(jsonb_agg(jsonb_build_object(
          'definitionId', e.definition_id,
          'printingId', e.printing_id,
          'section', e.section,
          'quantity', e.quantity::text
        ) ORDER BY e.section, e.definition_id, e.printing_id NULLS FIRST), '[]'::jsonb)
        FROM app.deck_entry e WHERE e.revision_id = r.id
      ),
      'customEntries', (
        SELECT COALESCE(jsonb_agg(jsonb_build_object(
          'customCardRevisionId', ce.custom_card_revision_id,
          'section', ce.section,
          'quantity', ce.quantity::text
        ) ORDER BY ce.section, ce.custom_card_revision_id), '[]'::jsonb)
        FROM app.deck_custom_entry ce WHERE ce.revision_id = r.id
      )
    ) END
  )
  INTO result
  FROM app.deck d
  JOIN app.format f ON f.id = d.format_id
  LEFT JOIN app.deck_revision r ON r.id = d.current_revision_id AND r.deck_id = d.id AND r.sealed_at IS NOT NULL
  WHERE d.id = p_deck AND d.account_id = actor AND d.source_kind = 'account';
  IF result IS NULL THEN RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('deck', 'unavailable')); END IF;
  RETURN jsonb_build_object('status', 'success', 'data', result);
END $$;

CREATE OR REPLACE FUNCTION app.read_my_deck_library(p_after uuid, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path = '' AS $$
DECLARE actor uuid := app.current_account_id(); authority jsonb; result jsonb;
BEGIN
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT COALESCE(jsonb_agg(page.entry ORDER BY page.id), '[]'::jsonb) INTO result FROM (
    SELECT d.id, jsonb_build_object(
      'id', d.id,
      'name', d.name,
      'mode', d.mode,
      'currentRevisionId', d.current_revision_id,
      'format', jsonb_build_object('id', f.id, 'key', f.key, 'name', f.name),
      'appearance', jsonb_build_object('backId', d.back_id, 'sleeveId', d.sleeve_id, 'diceId', d.dice_id, 'playmatId', d.playmat_id),
      'faceCard', app.deck_face_card_json(r.id, d.face_definition_id, d.face_custom_card_revision_id),
      'hasCustomCards', CASE WHEN r.id IS NULL THEN false ELSE EXISTS (SELECT 1 FROM app.deck_custom_entry ce WHERE ce.revision_id = r.id) END
    ) AS entry
    FROM app.deck d
    JOIN app.format f ON f.id = d.format_id
    LEFT JOIN app.deck_revision r ON r.id = d.current_revision_id AND r.deck_id = d.id AND r.sealed_at IS NOT NULL
    WHERE d.account_id = actor AND d.source_kind = 'account' AND (p_after IS NULL OR d.id > p_after)
    ORDER BY d.id LIMIT p_limit
  ) page;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object(
    'items', result,
    'nextCursor', CASE WHEN jsonb_array_length(result) = p_limit THEN result->-1->>'id' ELSE NULL END
  ));
END $$;

CREATE OR REPLACE FUNCTION app.read_current_room_slots(p_room uuid, p_after uuid, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  lobby app.room;
  items jsonb;
  cursor_id uuid;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_room IS NULL OR p_room = '00000000-0000-0000-0000-000000000000'::uuid OR p_after = '00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  IF NOT app.room_visible(p_room) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room;

  SELECT COALESCE(jsonb_agg(body ORDER BY id), '[]'::jsonb) INTO items FROM (
    SELECT s.id, jsonb_build_object(
      'id', s.id,
      'accountId', s.account_id,
      'seatIndex', s.seat_index::text,
      'controllerKind', s.controller_kind,
      'ready', s.ready,
      'deckRevisionId', CASE
        WHEN s.controller_kind = 'human' AND s.account_id = actor THEN s.deck_revision_id
        WHEN lobby.mode = 'open' AND NOT lobby.competitive AND s.controller_kind = 'ai' AND (actor = lobby.host_id OR actor = s.sponsor_id) THEN s.deck_revision_id
        ELSE NULL
      END,
      'deckName', CASE
        WHEN s.deck_revision_id IS NULL THEN NULL
        WHEN s.controller_kind = 'human' AND s.account_id = actor THEN d.name
        WHEN lobby.mode = 'open' AND NOT lobby.competitive AND s.controller_kind = 'ai' AND (actor = lobby.host_id OR actor = s.sponsor_id) THEN d.name
        WHEN lobby.mode = 'open' AND NOT lobby.competitive AND lobby.share_host_decks AND d.account_id = lobby.host_id THEN d.name
        ELSE NULL
      END
    ) AS body
    FROM app.room_slot s
    LEFT JOIN app.deck_revision dr ON dr.id = s.deck_revision_id
    LEFT JOIN app.deck d ON d.id = dr.deck_id
    WHERE s.room_id = p_room AND s.retired_at IS NULL AND (p_after IS NULL OR s.id > p_after)
    ORDER BY s.id LIMIT p_limit
  ) page;

  IF jsonb_array_length(items) = p_limit THEN cursor_id := (items->-1->>'id')::uuid; END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('items', items, 'nextCursor', cursor_id));
END $$;

CREATE OR REPLACE FUNCTION app.read_visible_rooms(p_after uuid, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path = '' AS $$
DECLARE actor uuid := app.current_account_id(); authority jsonb; items jsonb; cursor_id uuid;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_after = '00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  SELECT COALESCE(jsonb_agg(body ORDER BY id), '[]'::jsonb) INTO items FROM (
    SELECT r.id, jsonb_build_object(
      'id', r.id, 'hostId', r.host_id, 'formatId', r.format_id, 'profileId', r.profile_id,
      'mode', r.mode, 'competitive', r.competitive, 'visibility', r.visibility, 'state', r.state,
      'revision', r.revision::text, 'structure', r.structure, 'configurationId', r.configuration_id,
      'targetSeatCount', r.target_seat_count::text, 'countdownAt', r.countdown_at,
      'allowCustomCards', r.allow_custom_cards, 'shareHostDecks', r.share_host_decks
    ) AS body
    FROM app.room r WHERE p_after IS NULL OR r.id > p_after ORDER BY r.id LIMIT p_limit
  ) page;
  IF jsonb_array_length(items) = p_limit THEN cursor_id := (items->-1->>'id')::uuid; END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', actor, 'items', items, 'nextCursor', cursor_id));
END $$;

CREATE OR REPLACE FUNCTION app.read_visible_room(p_room uuid, p_after uuid, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path = '' AS $$
DECLARE actor uuid := app.current_account_id(); authority jsonb; room_data jsonb; slots jsonb; lent jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_room IS NULL OR p_room = '00000000-0000-0000-0000-000000000000'::uuid OR p_after = '00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  SELECT jsonb_build_object(
    'id', r.id, 'hostId', r.host_id, 'formatId', r.format_id, 'profileId', r.profile_id,
    'mode', r.mode, 'competitive', r.competitive, 'visibility', r.visibility, 'state', r.state,
    'revision', r.revision::text, 'structure', r.structure, 'configurationId', r.configuration_id,
    'targetSeatCount', r.target_seat_count::text, 'countdownAt', r.countdown_at,
    'allowCustomCards', r.allow_custom_cards, 'shareHostDecks', r.share_host_decks
  ) INTO room_data FROM app.room r WHERE r.id = p_room;
  IF room_data IS NULL THEN RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable')); END IF;
  slots := app.read_current_room_slots(p_room, p_after, p_limit);
  IF slots->>'status' <> 'success' THEN RETURN slots; END IF;
  lent := app.read_host_shared_decks(p_room);
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object(
    'accountId', actor,
    'room', room_data,
    'slots', slots->'data',
    'hostLentDecks', lent
  ));
END $$;

CREATE OR REPLACE FUNCTION app.read_room_enriched(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  r app.room;
  fmt app.format;
  slots jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO r FROM app.room WHERE id = p_room;
  IF r.id IS NULL OR NOT EXISTS (
    SELECT 1 FROM app.room_slot s WHERE s.room_id = p_room AND s.account_id = actor AND s.retired_at IS NULL
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  SELECT * INTO fmt FROM app.format WHERE id = r.format_id;

  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'slotId', s.id,
    'seatIndex', s.seat_index,
    'controllerKind', s.controller_kind,
    'aiTier', CASE WHEN s.controller_kind = 'ai' THEN s.ai_tier ELSE NULL END,
    'accountId', s.account_id,
    'username', CASE WHEN a.state = 'deleted' THEN 'Deleted player' ELSE a.username END,
    'supporter', CASE WHEN a.id IS NOT NULL THEN app.active_supporter_tier(a.id) IS NOT NULL ELSE false END,
    'selectedBadge', prof.selected_badge,
    'cardArtProfile', CASE WHEN prof.id IS NOT NULL THEN app.profile_art_snapshot(prof) ELSE NULL END,
    'deckRevisionId', CASE
      WHEN s.controller_kind = 'human' AND s.account_id = actor THEN s.deck_revision_id
      WHEN r.mode = 'open' AND NOT r.competitive AND s.controller_kind = 'ai' AND (actor = r.host_id OR actor = s.sponsor_id) THEN s.deck_revision_id
      ELSE NULL
    END,
    'deckName', CASE
      WHEN s.deck_revision_id IS NULL THEN NULL
      WHEN s.controller_kind = 'human' AND s.account_id = actor THEN d.name
      WHEN r.mode = 'open' AND NOT r.competitive AND s.controller_kind = 'ai' AND (actor = r.host_id OR actor = s.sponsor_id) THEN d.name
      WHEN r.mode = 'open' AND NOT r.competitive AND r.share_host_decks AND d.account_id = r.host_id THEN d.name
      ELSE NULL
    END,
    'ready', s.ready,
    'teamRef', s.team_ref
  ) ORDER BY s.seat_index), '[]'::jsonb)
  INTO slots
  FROM app.room_slot s
  LEFT JOIN app.account a ON a.id = s.account_id
  LEFT JOIN app.profile prof ON prof.account_id = a.id
  LEFT JOIN app.deck_revision dr ON dr.id = s.deck_revision_id
  LEFT JOIN app.deck d ON d.id = dr.deck_id
  WHERE s.room_id = p_room AND s.retired_at IS NULL;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'room', jsonb_build_object(
        'id', r.id,
        'hostId', r.host_id,
        'formatId', r.format_id,
        'formatKey', fmt.key,
        'formatName', fmt.name,
        'profileId', r.profile_id,
        'mode', r.mode,
        'competitive', r.competitive,
        'visibility', r.visibility,
        'state', r.state,
        'revision', r.revision::text,
        'countdownAt', CASE WHEN r.countdown_at IS NOT NULL THEN to_char(r.countdown_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') ELSE NULL END,
        'aiDifficulty', COALESCE(r.ai_difficulty, 'standard'),
        'planechaseEnabled', r.planechase_enabled,
        'rangeOfInfluence', r.range_of_influence,
        'allowCustomCards', r.allow_custom_cards,
        'shareHostDecks', r.share_host_decks
      ),
      'slots', slots,
      'hostLentDecks', app.read_host_shared_decks(p_room)
    )
  );
END $$;

COMMIT;
