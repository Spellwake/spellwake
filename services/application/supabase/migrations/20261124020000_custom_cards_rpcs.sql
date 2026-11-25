-- Stage 1.3 (Phase 1): Shared SQL helpers, Custom Card CRUD/Art/Reveal RPCs,
-- Deck & Lobby lending/custom RPCs, AI slot deck assignment, Native Preparation/Admission/BO3
-- v1/v2 support, and GDPR account minimization v3 (validating both historical v2 and new v3).
--
-- Phase 1 live rollout discipline:
-- - Explicitly DROP prior arities of extended functions and recreate with DEFAULT trailing args
--   and exact REVOKE/GRANT in the same transaction to prevent PostgREST PGRST203 overload errors.
-- - Keep deck_face_card_json emitting legacy { definitionId, artUrl } until Phase 3 enablement
--   (20261124030000_enable_custom_card_reads.sql) runs AFTER Stage 4 application_api deploys.
BEGIN;

-- ---------------------------------------------------------------------------
-- 1. Shared SQL Helpers
-- ---------------------------------------------------------------------------

CREATE FUNCTION app.deck_revision_has_custom_cards(p_revision uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT EXISTS (
    SELECT 1 FROM app.deck_custom_entry WHERE revision_id = p_revision
  )
$$;

CREATE FUNCTION app.deck_revision_card_totals(p_revision uuid)
RETURNS TABLE(entry_count bigint, total_quantity numeric)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT
    (SELECT count(*) FROM app.deck_entry WHERE revision_id = p_revision)
      + (SELECT count(*) FROM app.deck_custom_entry WHERE revision_id = p_revision) AS entry_count,
    COALESCE((SELECT sum(quantity) FROM app.deck_entry WHERE revision_id = p_revision), 0::numeric)
      + COALESCE((SELECT sum(quantity) FROM app.deck_custom_entry WHERE revision_id = p_revision), 0::numeric) AS total_quantity
$$;

CREATE FUNCTION app.room_seat_can_use_deck(
  p_room uuid,
  p_actor uuid,
  p_revision uuid,
  p_controller_kind text
) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  lobby app.room;
  rev app.deck_revision;
  lib app.deck;
  has_custom boolean;
BEGIN
  IF p_room IS NULL OR p_revision IS NULL OR p_controller_kind NOT IN ('human', 'ai') THEN
    RETURN false;
  END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room;
  SELECT * INTO rev FROM app.deck_revision WHERE id = p_revision AND sealed_at IS NOT NULL;
  IF lobby.id IS NULL OR rev.id IS NULL THEN
    RETURN false;
  END IF;
  SELECT * INTO lib FROM app.deck WHERE id = rev.deck_id;
  IF lib.id IS NULL OR lib.mode <> lobby.mode OR lib.format_id <> lobby.format_id THEN
    RETURN false;
  END IF;

  has_custom := app.deck_revision_has_custom_cards(rev.id);
  IF has_custom AND (NOT lobby.allow_custom_cards OR lobby.mode <> 'open' OR lobby.competitive) THEN
    RETURN false;
  END IF;
  IF has_custom AND EXISTS (
    SELECT 1
    FROM app.deck_custom_entry ce
    JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
    WHERE ce.revision_id = rev.id AND NOT cr.stage2_playable
  ) THEN
    RETURN false;
  END IF;

  IF p_controller_kind = 'human' THEN
    IF lib.source_kind <> 'account' THEN
      RETURN false;
    END IF;
    IF lib.account_id = p_actor THEN
      RETURN true;
    END IF;
    RETURN lobby.mode = 'open'
      AND NOT lobby.competitive
      AND lobby.share_host_decks
      AND lib.account_id = lobby.host_id;
  ELSE
    -- AI seat: system_pool, sponsor's deck, or host's lent deck in Open Play non-competitive rooms.
    IF lib.source_kind = 'system_pool' THEN
      RETURN NOT has_custom;
    END IF;
    IF lib.source_kind <> 'account' THEN
      RETURN false;
    END IF;
    IF lib.account_id = p_actor THEN
      RETURN true;
    END IF;
    RETURN lobby.mode = 'open'
      AND NOT lobby.competitive
      AND lobby.share_host_decks
      AND lib.account_id = lobby.host_id;
  END IF;
END $$;

CREATE FUNCTION app.custom_card_revision_json(p_rev app.custom_card_revision) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT jsonb_build_object(
    'revisionId', p_rev.id,
    'customCardId', p_rev.custom_card_id,
    'revisionNumber', p_rev.revision_number::text,
    'rulesSha256', p_rev.rules_sha256,
    'name', p_rev.name,
    'layout', p_rev.layout,
    'rarity', p_rev.rarity,
    'faces', p_rev.faces_rules_json,
    'visualFrameAccents', p_rev.visual_frame_accents_json,
    'flavorTexts', p_rev.flavor_texts_json,
    'collectorNote', p_rev.collector_note,
    'stage1Parsed', p_rev.stage1_parsed,
    'stage2Playable', p_rev.stage2_playable,
    'compilerRevision', p_rev.compiler_revision,
    'art', COALESCE((
      SELECT jsonb_agg(jsonb_build_object(
        'faceIndex', a.face_index,
        'mimeType', a.mime_type,
        'artSha256', a.art_sha256,
        'hasArt', a.art_bytes IS NOT NULL,
        'cropX', a.crop_x,
        'cropY', a.crop_y,
        'zoom', a.zoom,
        'artist', a.artist
      ) ORDER BY a.face_index)
      FROM app.custom_card_revision_art a
      WHERE a.revision_id = p_rev.id
    ), '[]'::jsonb),
    'createdAt', to_char(p_rev.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
  )
$$;

-- ---------------------------------------------------------------------------
-- 2. Custom Card CRUD, Art & Match Reveal RPCs
-- ---------------------------------------------------------------------------

CREATE FUNCTION app.create_custom_card_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_payload jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  card_id uuid;
  rev_id uuid;
  art_item jsonb;
  art_bytes_decoded bytea;
  v_name text;
  v_layout text;
  v_rarity text;
  v_rules_sha text;
  v_compiler_rev text;
  v_faces jsonb;
  v_accents jsonb;
  v_flavors jsonb;
  v_note text;
  v_stage1 boolean;
  v_stage2 boolean;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR jsonb_typeof(p_payload) IS DISTINCT FROM 'object' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  v_name := btrim(COALESCE(p_payload->>'name', ''));
  v_layout := COALESCE(p_payload->>'layout', '');
  v_rarity := COALESCE(p_payload->>'rarity', '');
  v_rules_sha := COALESCE(p_payload->>'rulesSha256', '');
  v_compiler_rev := COALESCE(p_payload->>'compilerRevision', '');
  v_faces := p_payload->'facesRules';
  v_accents := COALESCE(p_payload->'visualFrameAccents', '[]'::jsonb);
  v_flavors := COALESCE(p_payload->'flavorTexts', '[]'::jsonb);
  v_note := NULLIF(p_payload->>'collectorNote', '');
  v_stage1 := COALESCE((p_payload->>'stage1Parsed')::boolean, false);
  v_stage2 := COALESCE((p_payload->>'stage2Playable')::boolean, false);

  receipt := app.begin_account_command(
    player.id,
    p_request,
    'custom_card_create_v1',
    player.id::text,
    jsonb_build_object(
      'name', v_name,
      'layout', v_layout,
      'rarity', v_rarity,
      'rulesSha256', v_rules_sha,
      'compilerRevision', v_compiler_rev
    )
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  IF length(v_name) NOT BETWEEN 1 AND 120
     OR v_layout NOT IN ('normal', 'transform', 'modal_dfc', 'adventure', 'split', 'planeswalker', 'saga')
     OR v_rarity NOT IN ('common', 'uncommon', 'rare', 'mythic')
     OR v_rules_sha !~ '^[0-9a-f]{64}$'
     OR length(btrim(v_compiler_rev)) NOT BETWEEN 1 AND 64
     OR jsonb_typeof(v_faces) IS DISTINCT FROM 'array'
     OR jsonb_array_length(v_faces) NOT BETWEEN 1 AND 2 THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'invalid custom card payload'));
  END IF;

  BEGIN
    INSERT INTO app.custom_card(account_id)
    VALUES (player.id)
    RETURNING id INTO card_id;

    INSERT INTO app.custom_card_revision(
      custom_card_id, revision_number, rules_sha256, name, layout, rarity,
      faces_rules_json, visual_frame_accents_json, flavor_texts_json, collector_note,
      stage1_parsed, stage2_playable, compiler_revision
    ) VALUES (
      card_id, 1, v_rules_sha, v_name, v_layout, v_rarity,
      v_faces, v_accents, v_flavors, v_note,
      v_stage1, v_stage2, v_compiler_rev
    ) RETURNING id INTO rev_id;

    IF jsonb_typeof(p_payload->'art') = 'array' THEN
      FOR art_item IN SELECT value FROM jsonb_array_elements(p_payload->'art') LOOP
        art_bytes_decoded := decode(art_item->>'artBase64', 'base64');
        INSERT INTO app.custom_card_revision_art(
          revision_id, face_index, mime_type, art_bytes, art_sha256,
          crop_x, crop_y, zoom, artist
        ) VALUES (
          rev_id,
          (art_item->>'faceIndex')::smallint,
          art_item->>'mimeType',
          art_bytes_decoded,
          art_item->>'artSha256',
          COALESCE((art_item->>'cropX')::double precision, 0),
          COALESCE((art_item->>'cropY')::double precision, 0),
          COALESCE((art_item->>'zoom')::double precision, 1),
          NULLIF(art_item->>'artist', '')
        );
      END LOOP;
    END IF;

    UPDATE app.custom_card
    SET current_revision_id = rev_id, updated_at = clock_timestamp()
    WHERE id = card_id;
  EXCEPTION
    WHEN check_violation THEN
      IF SQLERRM = 'custom card limit of 250 active cards reached' THEN
        RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('limit', 'active_card_limit_reached'));
      END IF;
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'invalid custom card payload'));
  END;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', card_id, 'revisionId', rev_id, 'revisionNumber', '1')
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.save_custom_card_revision_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_card_id uuid,
  p_expected_revision uuid,
  p_payload jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  card app.custom_card;
  next_number numeric;
  rev_id uuid;
  art_item jsonb;
  art_bytes_decoded bytea;
  v_name text;
  v_layout text;
  v_rarity text;
  v_rules_sha text;
  v_compiler_rev text;
  v_faces jsonb;
  v_accents jsonb;
  v_flavors jsonb;
  v_note text;
  v_stage1 boolean;
  v_stage2 boolean;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_card_id IS NULL OR p_card_id = '00000000-0000-0000-0000-000000000000'::uuid
     OR jsonb_typeof(p_payload) IS DISTINCT FROM 'object' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  v_name := btrim(COALESCE(p_payload->>'name', ''));
  v_layout := COALESCE(p_payload->>'layout', '');
  v_rarity := COALESCE(p_payload->>'rarity', '');
  v_rules_sha := COALESCE(p_payload->>'rulesSha256', '');
  v_compiler_rev := COALESCE(p_payload->>'compilerRevision', '');
  v_faces := p_payload->'facesRules';
  v_accents := COALESCE(p_payload->'visualFrameAccents', '[]'::jsonb);
  v_flavors := COALESCE(p_payload->'flavorTexts', '[]'::jsonb);
  v_note := NULLIF(p_payload->>'collectorNote', '');
  v_stage1 := COALESCE((p_payload->>'stage1Parsed')::boolean, false);
  v_stage2 := COALESCE((p_payload->>'stage2Playable')::boolean, false);

  receipt := app.begin_account_command(
    player.id,
    p_request,
    'custom_card_revision_save_v1',
    p_card_id::text,
    jsonb_build_object(
      'expectedRevisionId', p_expected_revision,
      'name', v_name,
      'layout', v_layout,
      'rarity', v_rarity,
      'rulesSha256', v_rules_sha,
      'compilerRevision', v_compiler_rev
    )
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO card
  FROM app.custom_card
  WHERE id = p_card_id AND account_id = player.id
  FOR UPDATE;
  IF card.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'unavailable'));
  END IF;
  IF p_expected_revision IS NOT NULL AND card.current_revision_id IS DISTINCT FROM p_expected_revision THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'conflict'));
  END IF;

  SELECT COALESCE(max(revision_number), 0) + 1 INTO next_number
  FROM app.custom_card_revision
  WHERE custom_card_id = p_card_id;

  BEGIN
    INSERT INTO app.custom_card_revision(
      custom_card_id, revision_number, rules_sha256, name, layout, rarity,
      faces_rules_json, visual_frame_accents_json, flavor_texts_json, collector_note,
      stage1_parsed, stage2_playable, compiler_revision
    ) VALUES (
      p_card_id, next_number, v_rules_sha, v_name, v_layout, v_rarity,
      v_faces, v_accents, v_flavors, v_note,
      v_stage1, v_stage2, v_compiler_rev
    ) RETURNING id INTO rev_id;

    IF jsonb_typeof(p_payload->'art') = 'array' THEN
      FOR art_item IN SELECT value FROM jsonb_array_elements(p_payload->'art') LOOP
        IF art_item ? 'artBase64' AND jsonb_typeof(art_item->'artBase64') = 'string' THEN
          art_bytes_decoded := decode(art_item->>'artBase64', 'base64');
          INSERT INTO app.custom_card_revision_art(
            revision_id, face_index, mime_type, art_bytes, art_sha256,
            crop_x, crop_y, zoom, artist
          ) VALUES (
            rev_id,
            (art_item->>'faceIndex')::smallint,
            art_item->>'mimeType',
            art_bytes_decoded,
            art_item->>'artSha256',
            COALESCE((art_item->>'cropX')::double precision, 0),
            COALESCE((art_item->>'cropY')::double precision, 0),
            COALESCE((art_item->>'zoom')::double precision, 1),
            NULLIF(art_item->>'artist', '')
          );
        ELSIF art_item ? 'copyFromRevisionId' AND card.current_revision_id IS NOT NULL THEN
          INSERT INTO app.custom_card_revision_art(
            revision_id, face_index, mime_type, art_bytes, art_sha256,
            crop_x, crop_y, zoom, artist
          )
          SELECT
            rev_id,
            (art_item->>'faceIndex')::smallint,
            prior.mime_type,
            prior.art_bytes,
            prior.art_sha256,
            COALESCE((art_item->>'cropX')::double precision, prior.crop_x),
            COALESCE((art_item->>'cropY')::double precision, prior.crop_y),
            COALESCE((art_item->>'zoom')::double precision, prior.zoom),
            CASE WHEN art_item ? 'artist' THEN NULLIF(art_item->>'artist', '') ELSE prior.artist END
          FROM app.custom_card_revision_art prior
          JOIN app.custom_card_revision prior_rev ON prior_rev.id = prior.revision_id
          WHERE prior.revision_id = (art_item->>'copyFromRevisionId')::uuid
            AND prior_rev.custom_card_id = p_card_id
            AND prior.face_index = (art_item->>'faceIndex')::smallint
            AND prior.art_bytes IS NOT NULL;
        END IF;
      END LOOP;
    END IF;

    UPDATE app.custom_card
    SET current_revision_id = rev_id, updated_at = clock_timestamp()
    WHERE id = p_card_id;
  EXCEPTION
    WHEN check_violation THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'invalid custom card payload'));
  END;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', p_card_id, 'revisionId', rev_id, 'revisionNumber', next_number::text)
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.archive_custom_card_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_card_id uuid,
  p_archived boolean
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  card app.custom_card;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_card_id IS NULL OR p_card_id = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_archived IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id,
    p_request,
    'custom_card_archive_v1',
    p_card_id::text,
    jsonb_build_object('archived', p_archived)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO card FROM app.custom_card WHERE id = p_card_id AND account_id = player.id FOR UPDATE;
  IF card.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'unavailable'));
  END IF;

  BEGIN
    UPDATE app.custom_card
    SET archived_at = CASE WHEN p_archived THEN COALESCE(archived_at, clock_timestamp()) ELSE NULL END,
        updated_at = clock_timestamp()
    WHERE id = p_card_id;
  EXCEPTION
    WHEN check_violation THEN
      IF SQLERRM = 'custom card limit of 250 active cards reached' THEN
        RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('limit', 'active_card_limit_reached'));
      END IF;
      RAISE;
  END;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', p_card_id, 'archived', p_archived)
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.delete_custom_card_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_card_id uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  card app.custom_card;
  is_referenced boolean;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_card_id IS NULL OR p_card_id = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id,
    p_request,
    'custom_card_delete_v1',
    p_card_id::text,
    jsonb_build_object('cardId', p_card_id)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO card FROM app.custom_card WHERE id = p_card_id AND account_id = player.id FOR UPDATE;
  IF card.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'unavailable'));
  END IF;

  is_referenced := EXISTS (
    SELECT 1 FROM app.deck_custom_entry dce
    JOIN app.custom_card_revision r ON r.id = dce.custom_card_revision_id
    WHERE r.custom_card_id = p_card_id
  ) OR EXISTS (
    SELECT 1 FROM app.deck d
    JOIN app.custom_card_revision r ON r.id = d.face_custom_card_revision_id
    WHERE r.custom_card_id = p_card_id
  ) OR EXISTS (
    SELECT 1 FROM app.match_revealed_custom_card mr
    JOIN app.custom_card_revision r ON r.id = mr.custom_card_revision_id
    WHERE r.custom_card_id = p_card_id
  );

  IF is_referenced THEN
    UPDATE app.custom_card
    SET archived_at = COALESCE(archived_at, clock_timestamp()), updated_at = clock_timestamp()
    WHERE id = p_card_id;
    RETURN app.finish_account_command(
      receipt.id,
      true,
      jsonb_build_object('resourceId', p_card_id, 'disposition', 'archived')
    );
  END IF;

  BEGIN
    PERFORM set_config('spellwake.deleting_unreferenced_custom_card', 'on', true);
    UPDATE app.custom_card SET current_revision_id = NULL WHERE id = p_card_id;
    DELETE FROM app.custom_card_revision_art
    WHERE revision_id IN (SELECT id FROM app.custom_card_revision WHERE custom_card_id = p_card_id);
    DELETE FROM app.custom_card_revision WHERE custom_card_id = p_card_id;
    DELETE FROM app.custom_card WHERE id = p_card_id;
  EXCEPTION
    WHEN foreign_key_violation THEN
      UPDATE app.custom_card
      SET archived_at = COALESCE(archived_at, clock_timestamp()), updated_at = clock_timestamp()
      WHERE id = p_card_id;
      RETURN app.finish_account_command(
        receipt.id,
        true,
        jsonb_build_object('resourceId', p_card_id, 'disposition', 'archived')
      );
  END;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', p_card_id, 'disposition', 'deleted')
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.read_my_custom_cards(
  p_after uuid,
  p_limit integer,
  p_include_archived boolean DEFAULT false,
  p_search text DEFAULT NULL
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  items jsonb;
  cursor_id uuid;
  active_count bigint;
  q text := NULLIF(btrim(COALESCE(p_search, '')), '');
BEGIN
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
     OR p_after = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  SELECT count(*) INTO active_count
  FROM app.custom_card
  WHERE account_id = actor AND archived_at IS NULL;

  SELECT COALESCE(jsonb_agg(page.entry ORDER BY page.id), '[]'::jsonb)
  INTO items
  FROM (
    SELECT c.id,
      jsonb_build_object(
        'id', c.id,
        'archivedAt', CASE WHEN c.archived_at IS NOT NULL THEN to_char(c.archived_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') ELSE NULL END,
        'createdAt', to_char(c.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
        'updatedAt', to_char(c.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
        'currentRevision', app.custom_card_revision_json(r)
      ) AS entry
    FROM app.custom_card c
    JOIN app.custom_card_revision r ON r.id = c.current_revision_id
    WHERE c.account_id = actor
      AND (COALESCE(p_include_archived, false) OR c.archived_at IS NULL)
      AND (p_after IS NULL OR c.id > p_after)
      AND (q IS NULL OR r.name ILIKE '%' || q || '%')
    ORDER BY c.id
    LIMIT p_limit
  ) page;

  IF jsonb_array_length(items) = p_limit THEN
    cursor_id := (items->-1->>'id')::uuid;
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'activeCount', active_count,
      'activeLimit', 250,
      'items', items,
      'nextCursor', cursor_id
    )
  );
END $$;

CREATE FUNCTION app.read_custom_card_art_for_owner(
  p_actor uuid,
  p_revision_id uuid,
  p_face_index smallint
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  art_row app.custom_card_revision_art;
BEGIN
  IF p_actor IS NULL OR p_revision_id IS NULL OR p_face_index NOT BETWEEN 0 AND 1 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  SELECT a.* INTO art_row
  FROM app.custom_card_revision_art a
  JOIN app.custom_card_revision r ON r.id = a.revision_id
  JOIN app.custom_card c ON c.id = r.custom_card_id
  WHERE a.revision_id = p_revision_id
    AND a.face_index = p_face_index
    AND c.account_id = p_actor
    AND a.art_bytes IS NOT NULL;

  IF art_row.revision_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'mimeType', art_row.mime_type,
      'artSha256', art_row.art_sha256,
      'artBase64', encode(art_row.art_bytes, 'base64')
    )
  );
END $$;

CREATE FUNCTION app.read_lent_deck_face_art(
  p_actor uuid,
  p_room_id uuid,
  p_deck_id uuid,
  p_custom_card_revision_id uuid
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  lobby app.room;
  art_row app.custom_card_revision_art;
BEGIN
  IF p_actor IS NULL OR p_room_id IS NULL OR p_deck_id IS NULL OR p_custom_card_revision_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  SELECT * INTO lobby FROM app.room WHERE id = p_room_id;
  IF lobby.id IS NULL
     OR lobby.mode <> 'open'
     OR lobby.competitive
     OR NOT lobby.share_host_decks
     OR NOT EXISTS (
       SELECT 1 FROM app.room_slot s
       WHERE s.room_id = p_room_id
         AND s.retired_at IS NULL
         AND s.controller_kind = 'human'
         AND s.account_id = p_actor
     ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  IF NOT EXISTS (
    SELECT 1 FROM app.deck d
    WHERE d.id = p_deck_id
      AND d.account_id = lobby.host_id
      AND d.source_kind = 'account'
      AND d.mode = 'open'
      AND d.format_id = lobby.format_id
      AND d.current_revision_id IS NOT NULL
      AND (
        d.face_custom_card_revision_id = p_custom_card_revision_id
        OR EXISTS (
          SELECT 1 FROM app.deck_custom_entry ce
          WHERE ce.revision_id = d.current_revision_id
            AND ce.custom_card_revision_id = p_custom_card_revision_id
        )
      )
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  SELECT a.* INTO art_row
  FROM app.custom_card_revision_art a
  WHERE a.revision_id = p_custom_card_revision_id
    AND a.face_index = 0
    AND a.art_bytes IS NOT NULL;

  IF art_row.revision_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'mimeType', art_row.mime_type,
      'artSha256', art_row.art_sha256,
      'artBase64', encode(art_row.art_bytes, 'base64')
    )
  );
END $$;

CREATE FUNCTION app.record_revealed_custom_cards(
  p_game_id uuid,
  p_context_id uuid,
  p_seat_index smallint,
  p_revision_ids uuid[]
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  inserted_count integer := 0;
BEGIN
  IF p_game_id IS NULL OR p_context_id IS NULL OR p_seat_index NOT BETWEEN 0 AND 5 THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid match reveal parameters';
  END IF;
  IF p_revision_ids IS NULL OR cardinality(p_revision_ids) = 0 THEN
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('inserted', 0));
  END IF;

  INSERT INTO app.match_revealed_custom_card(game_id, context_id, seat_index, custom_card_revision_id)
  SELECT p_game_id, p_context_id, p_seat_index, r.id
  FROM unnest(p_revision_ids) AS candidate(id)
  JOIN app.custom_card_revision r ON r.id = candidate.id
  ON CONFLICT (game_id, context_id, seat_index, custom_card_revision_id) DO NOTHING;

  GET DIAGNOSTICS inserted_count = ROW_COUNT;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('inserted', inserted_count));
END $$;

CREATE FUNCTION app.read_revealed_match_custom_card(
  p_actor uuid,
  p_game_id uuid,
  p_context_id uuid,
  p_custom_card_revision_id uuid
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  seat app.match_seat;
  m app.match;
  rev app.custom_card_revision;
  allowed boolean;
BEGIN
  IF p_actor IS NULL OR p_game_id IS NULL OR p_context_id IS NULL OR p_custom_card_revision_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('card', 'unavailable'));
  END IF;

  SELECT ms.*
  INTO seat
  FROM app.game g
  JOIN app.match mt ON mt.id = g.match_id
  JOIN app.match_seat ms ON ms.match_id = mt.id AND ms.account_id = p_actor AND ms.controller_kind = 'human'
  WHERE g.id = p_game_id;

  IF seat.id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('card', 'unavailable'));
  END IF;

  SELECT * INTO m FROM app.match WHERE id = seat.match_id;
  allowed := EXISTS (
    SELECT 1 FROM app.match_revealed_custom_card mr
    WHERE mr.game_id = p_game_id
      AND mr.context_id = p_context_id
      AND mr.seat_index = seat.seat_index
      AND mr.custom_card_revision_id = p_custom_card_revision_id
  ) OR (
    m.state IN ('completed', 'no_contest')
    AND EXISTS (
      SELECT 1 FROM app.match_seat any_seat
      JOIN app.deck_custom_entry ce ON ce.revision_id = any_seat.source_revision_id
      WHERE any_seat.match_id = m.id AND ce.custom_card_revision_id = p_custom_card_revision_id
    )
  );

  IF NOT allowed THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('card', 'unavailable'));
  END IF;

  SELECT * INTO rev FROM app.custom_card_revision WHERE id = p_custom_card_revision_id;
  IF rev.id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('card', 'unavailable'));
  END IF;

  RETURN jsonb_build_object('status', 'success', 'data', app.custom_card_revision_json(rev));
END $$;

CREATE FUNCTION app.read_revealed_match_custom_card_art(
  p_actor uuid,
  p_game_id uuid,
  p_context_id uuid,
  p_custom_card_revision_id uuid,
  p_face_index smallint
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  card_check jsonb;
  art_row app.custom_card_revision_art;
BEGIN
  IF p_face_index NOT BETWEEN 0 AND 1 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;
  card_check := app.read_revealed_match_custom_card(p_actor, p_game_id, p_context_id, p_custom_card_revision_id);
  IF card_check->>'status' IS DISTINCT FROM 'success' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  SELECT * INTO art_row
  FROM app.custom_card_revision_art
  WHERE revision_id = p_custom_card_revision_id
    AND face_index = p_face_index
    AND art_bytes IS NOT NULL;

  IF art_row.revision_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('art', 'unavailable'));
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'mimeType', art_row.mime_type,
      'artSha256', art_row.art_sha256,
      'artBase64', encode(art_row.art_bytes, 'base64')
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 3. PostgREST Overload Hygiene & Extended Deck RPCs
-- ---------------------------------------------------------------------------

-- Keep Phase 1 deck_face_card_json emitting legacy { definitionId, artUrl } so live
-- application_api decoders never break before 20261124030000_enable_custom_card_reads.sql.
DROP FUNCTION IF EXISTS app.deck_face_card_json(uuid, uuid);
CREATE FUNCTION app.deck_face_card_json(
  p_revision uuid,
  p_face uuid,
  p_custom_face uuid DEFAULT NULL
) RETURNS jsonb
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = '' AS $$
  WITH chosen AS (
    SELECT e.definition_id, e.printing_id
    FROM app.deck_entry e
    JOIN app.card_definition c ON c.id = e.definition_id
    WHERE e.revision_id = p_revision AND e.quantity > 0
    ORDER BY (e.definition_id IS NOT DISTINCT FROM p_face) DESC,
      CASE e.section WHEN 'commander' THEN 0 WHEN 'companion' THEN 1 WHEN 'main' THEN 2 ELSE 3 END,
      c.name, e.definition_id, e.printing_id NULLS LAST
    LIMIT 1
  )
  SELECT jsonb_build_object(
    'definitionId', chosen.definition_id,
    'artUrl', app.card_art_url(chosen.definition_id, chosen.printing_id)
  )
  FROM chosen
$$;
REVOKE ALL ON FUNCTION app.deck_face_card_json(uuid, uuid, uuid)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.deck_face_card_json(uuid, uuid, uuid) TO authenticated;

DROP FUNCTION IF EXISTS app.save_deck_contents(uuid, uuid, uuid, jsonb, uuid);
CREATE FUNCTION app.save_deck_contents(
  p_actor uuid,
  p_deck uuid,
  p_expected_revision uuid,
  p_entries jsonb,
  p_request uuid,
  p_custom_entries jsonb DEFAULT '[]'::jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt;
  saved app.deck;
  revision uuid;
  next_number numeric;
  entry jsonb;
  v_definition uuid;
  v_printing uuid;
  v_custom_rev uuid;
  section_name text;
  count_text text;
  terms jsonb;
  custom_arr jsonb := COALESCE(p_custom_entries, '[]'::jsonb);
BEGIN
  terms := jsonb_build_object('expectedRevision', p_expected_revision, 'entries', p_entries);
  IF jsonb_typeof(custom_arr) = 'array' AND jsonb_array_length(custom_arr) > 0 THEN
    terms := terms || jsonb_build_object('customEntries', custom_arr);
  END IF;

  receipt := app.begin_account_command(p_actor, p_request, 'deck_save', p_deck::text, terms);
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;

  PERFORM id FROM app.account WHERE id = p_actor AND state = 'active' FOR NO KEY UPDATE;
  IF NOT FOUND THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO saved
  FROM app.deck
  WHERE id = p_deck AND account_id = p_actor AND source_kind = 'account'
  FOR NO KEY UPDATE;
  IF NOT FOUND THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck unavailable'));
  END IF;

  IF saved.current_revision_id IS DISTINCT FROM p_expected_revision THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'deck changed; reload before saving'));
  END IF;

  IF p_entries IS NULL OR jsonb_typeof(p_entries) <> 'array' OR jsonb_typeof(custom_arr) <> 'array' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('entries', 'expected deck entry array'));
  END IF;

  IF jsonb_array_length(p_entries) + jsonb_array_length(custom_arr) > 1000 THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('entries', 'too many deck entries'));
  END IF;

  IF jsonb_array_length(custom_arr) > 0 AND saved.mode <> 'open' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('customEntries', 'custom cards require open play'));
  END IF;

  BEGIN
    FOR entry IN SELECT value FROM jsonb_array_elements(p_entries) LOOP
      IF jsonb_typeof(entry) <> 'object'
         OR (entry - ARRAY['definitionId', 'printingId', 'section', 'quantity']) <> '{}'::jsonb
         OR jsonb_typeof(entry->'definitionId') IS DISTINCT FROM 'string'
         OR jsonb_typeof(entry->'section') IS DISTINCT FROM 'string'
         OR jsonb_typeof(entry->'quantity') IS DISTINCT FROM 'string'
         OR (entry ? 'printingId' AND jsonb_typeof(entry->'printingId') NOT IN ('string', 'null')) THEN
        RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid entry shape';
      END IF;
      v_definition := (entry->>'definitionId')::uuid;
      v_printing := (entry->>'printingId')::uuid;
      section_name := entry->>'section';
      count_text := entry->>'quantity';
      IF section_name !~ '^[a-z][a-z0-9_]{0,31}$'
         OR count_text !~ '^[1-9][0-9]{0,77}$'
         OR NOT EXISTS (SELECT 1 FROM app.card_definition WHERE id = v_definition)
         OR (v_printing IS NOT NULL AND NOT EXISTS (SELECT 1 FROM app.printing WHERE id = v_printing AND definition_id = v_definition)) THEN
        RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid deck entry';
      END IF;
    END LOOP;

    IF EXISTS (
      SELECT 1 FROM jsonb_array_elements(p_entries) e
      GROUP BY (e->>'definitionId')::uuid, (e->>'printingId')::uuid, e->>'section'
      HAVING count(*) > 1
    ) THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'duplicate deck entry';
    END IF;

    FOR entry IN SELECT value FROM jsonb_array_elements(custom_arr) LOOP
      IF jsonb_typeof(entry) <> 'object'
         OR (entry - ARRAY['customCardRevisionId', 'section', 'quantity']) <> '{}'::jsonb
         OR jsonb_typeof(entry->'customCardRevisionId') IS DISTINCT FROM 'string'
         OR jsonb_typeof(entry->'section') IS DISTINCT FROM 'string'
         OR jsonb_typeof(entry->'quantity') IS DISTINCT FROM 'string' THEN
        RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid custom entry shape';
      END IF;
      v_custom_rev := (entry->>'customCardRevisionId')::uuid;
      section_name := entry->>'section';
      count_text := entry->>'quantity';
      IF section_name !~ '^[a-z][a-z0-9_]{0,31}$'
         OR count_text !~ '^[1-9][0-9]{0,77}$'
         OR NOT EXISTS (
           SELECT 1
           FROM app.custom_card_revision cr
           JOIN app.custom_card cc ON cc.id = cr.custom_card_id
           WHERE cr.id = v_custom_rev AND cc.account_id = p_actor
         ) THEN
        RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid custom deck entry';
      END IF;
    END LOOP;

    IF EXISTS (
      SELECT 1 FROM jsonb_array_elements(custom_arr) e
      GROUP BY (e->>'customCardRevisionId')::uuid, e->>'section'
      HAVING count(*) > 1
    ) THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'duplicate custom deck entry';
    END IF;
  EXCEPTION WHEN invalid_parameter_value OR invalid_text_representation THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('entries', 'invalid deck entries'));
  END;

  SELECT COALESCE(max(revision_number), 0) + 1 INTO next_number
  FROM app.deck_revision WHERE deck_id = p_deck;

  INSERT INTO app.deck_revision(deck_id, revision_number, created_at)
  VALUES (p_deck, next_number, clock_timestamp())
  RETURNING id INTO revision;

  INSERT INTO app.deck_entry(revision_id, definition_id, printing_id, section, quantity)
  SELECT revision, (e->>'definitionId')::uuid, (e->>'printingId')::uuid, e->>'section', (e->>'quantity')::numeric
  FROM jsonb_array_elements(p_entries) e;

  INSERT INTO app.deck_custom_entry(revision_id, custom_card_revision_id, section, quantity)
  SELECT revision, (e->>'customCardRevisionId')::uuid, e->>'section', (e->>'quantity')::numeric
  FROM jsonb_array_elements(custom_arr) e;

  UPDATE app.deck_revision SET sealed_at = clock_timestamp() WHERE id = revision;
  UPDATE app.deck SET current_revision_id = revision WHERE id = p_deck;

  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', revision));
END $$;
REVOKE ALL ON FUNCTION app.save_deck_contents(uuid, uuid, uuid, jsonb, uuid, jsonb)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

DROP FUNCTION IF EXISTS app.save_deck_command(uuid, uuid, uuid, uuid, uuid, jsonb);
CREATE FUNCTION app.save_deck_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_deck uuid,
  p_expected_revision uuid,
  p_entries jsonb,
  p_custom_entries jsonb DEFAULT '[]'::jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  result jsonb;
  saved app.deck;
  custom_arr jsonb := COALESCE(p_custom_entries, '[]'::jsonb);
  keep_official boolean;
  keep_custom boolean;
  next_official uuid := NULL;
  next_custom uuid := NULL;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  result := app.save_deck_contents(player.id, p_deck, p_expected_revision, p_entries, p_request, custom_arr);
  IF result->>'status' = 'success' THEN
    SELECT * INTO saved
    FROM app.deck
    WHERE id = p_deck AND account_id = player.id AND source_kind = 'account'
    FOR UPDATE;

    IF saved.id IS NOT NULL AND saved.current_revision_id IS NOT NULL THEN
      keep_official := saved.face_definition_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM app.deck_entry e
        WHERE e.revision_id = saved.current_revision_id
          AND e.definition_id = saved.face_definition_id
          AND e.quantity > 0
      );
      keep_custom := saved.face_custom_card_revision_id IS NOT NULL AND EXISTS (
        SELECT 1 FROM app.deck_custom_entry ce
        WHERE ce.revision_id = saved.current_revision_id
          AND ce.custom_card_revision_id = saved.face_custom_card_revision_id
          AND ce.quantity > 0
      );

      IF keep_official THEN
        next_official := saved.face_definition_id;
        next_custom := NULL;
      ELSIF keep_custom THEN
        next_official := NULL;
        next_custom := saved.face_custom_card_revision_id;
      ELSIF jsonb_typeof(p_entries) = 'array' AND jsonb_array_length(p_entries) > 0 THEN
        next_official := (p_entries->0->>'definitionId')::uuid;
        next_custom := NULL;
      ELSIF jsonb_typeof(custom_arr) = 'array' AND jsonb_array_length(custom_arr) > 0 THEN
        next_official := NULL;
        next_custom := (custom_arr->0->>'customCardRevisionId')::uuid;
      END IF;

      -- Atomic single-UPDATE preserves mutual-exclusivity CHECK across all transitions.
      UPDATE app.deck
      SET face_definition_id = next_official,
          face_custom_card_revision_id = next_custom
      WHERE id = p_deck;
    END IF;
  END IF;

  RETURN result;
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;
REVOKE ALL ON FUNCTION app.save_deck_command(uuid, uuid, uuid, uuid, uuid, jsonb, jsonb)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.save_deck_command(uuid, uuid, uuid, uuid, uuid, jsonb, jsonb) TO service_role;

CREATE OR REPLACE FUNCTION app.valid_deck_settings_change(p_settings jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path = '' AS $$
BEGIN
  IF jsonb_typeof(p_settings) IS DISTINCT FROM 'object' THEN RETURN false; END IF;
  IF (p_settings ? 'faceCardId') AND (p_settings ? 'faceCustomCardRevisionId') THEN RETURN false; END IF;
  IF p_settings ? 'faceCardId' AND (
    jsonb_typeof(p_settings->'faceCardId') IS DISTINCT FROM 'string'
    OR (p_settings->>'faceCardId')::uuid = '00000000-0000-0000-0000-000000000000'::uuid
  ) THEN RETURN false; END IF;
  IF p_settings ? 'faceCustomCardRevisionId' AND (
    jsonb_typeof(p_settings->'faceCustomCardRevisionId') IS DISTINCT FROM 'string'
    OR (p_settings->>'faceCustomCardRevisionId')::uuid = '00000000-0000-0000-0000-000000000000'::uuid
  ) THEN RETURN false; END IF;
  RETURN app.valid_deck_settings(p_settings - 'faceCardId' - 'faceCustomCardRevisionId');
EXCEPTION WHEN invalid_text_representation THEN RETURN false;
END $$;

CREATE OR REPLACE FUNCTION app.change_deck_settings_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_deck uuid,
  p_expected jsonb,
  p_settings jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  saved app.deck;
  observed jsonb;
  appearance jsonb;
  next_face_def uuid;
  next_face_custom uuid;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_deck IS NULL OR p_deck = '00000000-0000-0000-0000-000000000000'::uuid
     OR NOT app.valid_deck_settings(p_expected)
     OR NOT app.valid_deck_settings_change(p_settings)
     OR octet_length(jsonb_build_object('expected', p_expected, 'settings', p_settings)::text) > 4096 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id, p_request, 'deck_settings', p_deck::text,
    jsonb_build_object('expected', p_expected, 'settings', p_settings)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO saved
  FROM app.deck
  WHERE id = p_deck AND account_id = player.id AND source_kind = 'account'
  FOR NO KEY UPDATE;
  IF NOT FOUND THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck unavailable'));
  END IF;

  observed := jsonb_build_object(
    'name', saved.name,
    'mode', saved.mode,
    'formatId', saved.format_id,
    'appearance', jsonb_build_object(
      'backId', saved.back_id,
      'sleeveId', saved.sleeve_id,
      'diceId', saved.dice_id,
      'playmatId', saved.playmat_id
    )
  );
  IF observed IS DISTINCT FROM p_expected THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('settings', 'deck settings changed; reload before saving'));
  END IF;

  IF NOT EXISTS (SELECT 1 FROM app.format WHERE id = (p_settings->>'formatId')::uuid) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'invalid deck metadata'));
  END IF;

  IF p_settings->>'mode' <> 'open' AND EXISTS (
    SELECT 1 FROM app.deck_revision r
    JOIN app.deck_custom_entry ce ON ce.revision_id = r.id
    WHERE r.deck_id = saved.id
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('mode', 'custom cards require open play'));
  END IF;

  appearance := p_settings->'appearance';
  IF NOT app.has_cosmetic(player.id, (appearance->>'backId')::uuid, 'back')
     OR NOT app.has_cosmetic(player.id, (appearance->>'sleeveId')::uuid, 'sleeve')
     OR NOT app.has_cosmetic(player.id, (appearance->>'diceId')::uuid, 'dice')
     OR (appearance->>'playmatId' IS NOT NULL AND NOT app.has_cosmetic(player.id, (appearance->>'playmatId')::uuid, 'playmat')) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('cosmetics', 'cosmetic selection unavailable'));
  END IF;

  next_face_def := saved.face_definition_id;
  next_face_custom := saved.face_custom_card_revision_id;

  IF p_settings ? 'faceCardId' THEN
    next_face_def := (p_settings->>'faceCardId')::uuid;
    next_face_custom := NULL;
    IF NOT EXISTS (
      SELECT 1 FROM app.deck_entry e
      WHERE e.revision_id = saved.current_revision_id
        AND e.definition_id = next_face_def
        AND e.quantity > 0
    ) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('faceCard', 'card not in deck'));
    END IF;
  ELSIF p_settings ? 'faceCustomCardRevisionId' THEN
    next_face_def := NULL;
    next_face_custom := (p_settings->>'faceCustomCardRevisionId')::uuid;
    IF NOT EXISTS (
      SELECT 1 FROM app.deck_custom_entry ce
      WHERE ce.revision_id = saved.current_revision_id
        AND ce.custom_card_revision_id = next_face_custom
        AND ce.quantity > 0
    ) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('faceCard', 'card not in deck'));
    END IF;
  END IF;

  UPDATE app.deck
  SET name = btrim(p_settings->>'name'),
      mode = p_settings->>'mode',
      format_id = (p_settings->>'formatId')::uuid,
      back_id = (appearance->>'backId')::uuid,
      sleeve_id = (appearance->>'sleeveId')::uuid,
      dice_id = (appearance->>'diceId')::uuid,
      playmat_id = (appearance->>'playmatId')::uuid,
      face_definition_id = next_face_def,
      face_custom_card_revision_id = next_face_custom
  WHERE id = p_deck;

  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_deck));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE OR REPLACE FUNCTION app.read_deck_command(
  p_subject uuid,
  p_session uuid,
  p_kind text,
  p_command jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  request_id uuid;
  scope_name text;
  target text;
  terms jsonb;
  format_id uuid;
  deck_id uuid;
  expected uuid;
  cmd_keys text[];
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF jsonb_typeof(p_command) IS DISTINCT FROM 'object'
     OR octet_length(p_command::text) > 264192
     OR jsonb_typeof(p_command->'requestId') IS DISTINCT FROM 'string' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  request_id := (p_command->>'requestId')::uuid;
  IF request_id = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  SELECT array_agg(key ORDER BY key) INTO cmd_keys FROM jsonb_object_keys(p_command) key;

  IF p_kind = 'create' THEN
    IF cmd_keys IS DISTINCT FROM ARRAY['formatId', 'mode', 'name', 'requestId']::text[]
       OR jsonb_typeof(p_command->'name') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p_command->'mode') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p_command->'formatId') IS DISTINCT FROM 'string' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
    END IF;
    format_id := (p_command->>'formatId')::uuid;
    scope_name := 'deck_create';
    target := player.id::text;
    terms := jsonb_build_object('name', p_command->>'name', 'mode', p_command->>'mode', 'format', format_id);
  ELSIF p_kind = 'save' THEN
    IF (cmd_keys IS DISTINCT FROM ARRAY['deckId', 'entries', 'expectedRevisionId', 'requestId']::text[]
        AND cmd_keys IS DISTINCT FROM ARRAY['customEntries', 'deckId', 'entries', 'expectedRevisionId', 'requestId']::text[])
       OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
       OR jsonb_typeof(p_command->'entries') IS DISTINCT FROM 'array'
       OR jsonb_typeof(p_command->'expectedRevisionId') NOT IN ('string', 'null')
       OR (p_command ? 'customEntries' AND jsonb_typeof(p_command->'customEntries') IS DISTINCT FROM 'array') THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
    END IF;
    deck_id := (p_command->>'deckId')::uuid;
    expected := (p_command->>'expectedRevisionId')::uuid;
    scope_name := 'deck_save';
    target := deck_id::text;
    terms := jsonb_build_object('expectedRevision', expected, 'entries', p_command->'entries');
    IF p_command ? 'customEntries' AND jsonb_array_length(p_command->'customEntries') > 0 THEN
      terms := terms || jsonb_build_object('customEntries', p_command->'customEntries');
    END IF;
  ELSIF p_kind = 'settings' THEN
    IF cmd_keys IS DISTINCT FROM ARRAY['deckId', 'expected', 'requestId', 'settings']::text[]
       OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
       OR NOT app.valid_deck_settings(p_command->'expected')
       OR NOT app.valid_deck_settings_change(p_command->'settings')
       OR octet_length(p_command::text) > 4096 THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
    END IF;
    deck_id := (p_command->>'deckId')::uuid;
    IF deck_id = '00000000-0000-0000-0000-000000000000'::uuid THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
    END IF;
    scope_name := 'deck_settings';
    target := deck_id::text;
    terms := jsonb_build_object('expected', p_command->'expected', 'settings', p_command->'settings');
  ELSE
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  SELECT * INTO receipt
  FROM app.command_receipt
  WHERE principal_key = 'human:' || player.id::text
    AND scope = scope_name
    AND request_ref = request_id::text;
  IF FOUND AND (
    receipt.target_ref IS DISTINCT FROM target
    OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to(terms::text, 'UTF8')), 'hex')
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
  END IF;
  RETURN jsonb_build_object('status', 'success', 'data', app.read_account_command(player.id, scope_name, request_id));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN invalid_text_representation THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
END $$;

CREATE OR REPLACE FUNCTION app.delete_deck_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_deck uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  target app.deck;
  affected_rooms uuid[];
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_deck IS NULL OR p_deck = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id, p_request, 'deck_delete_v1', p_deck::text, jsonb_build_object('deckId', p_deck)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO target FROM app.deck WHERE id = p_deck AND account_id = player.id FOR UPDATE;
  IF target.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck unavailable'));
  END IF;

  IF EXISTS (
    SELECT 1 FROM app.queue_entry q
    WHERE q.account_id = player.id AND q.state = 'queued'
      AND q.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck)
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck in active queue'));
  END IF;

  -- Clear ANY pregame lobby/countdown slot referencing this deck (owner, lent guest, or AI seat).
  SELECT array_agg(DISTINCT r.id) INTO affected_rooms
  FROM app.room_slot rs
  JOIN app.room r ON r.id = rs.room_id
  WHERE r.state IN ('lobby', 'countdown')
    AND rs.retired_at IS NULL
    AND rs.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);

  IF affected_rooms IS NOT NULL THEN
    UPDATE app.ai_runner_binding b
    SET observed_state = 'running'
    FROM app.room_slot rs
    WHERE b.room_slot_id = rs.id
      AND rs.room_id = ANY(affected_rooms)
      AND rs.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck)
      AND b.ready_epoch IS NOT NULL;

    UPDATE app.room_slot rs
    SET deck_revision_id = NULL, ready = false
    WHERE rs.room_id = ANY(affected_rooms)
      AND rs.retired_at IS NULL
      AND rs.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);

    UPDATE app.room
    SET state = 'lobby', countdown_at = NULL, revision = revision + 1
    WHERE id = ANY(affected_rooms) AND state IN ('lobby', 'countdown');
  END IF;

  BEGIN
    PERFORM set_config('spellwake.deleting_unreferenced_deck', 'on', true);
    DELETE FROM app.deck_custom_entry WHERE revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);
    DELETE FROM app.deck_entry WHERE revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);
    UPDATE app.deck SET current_revision_id = NULL, face_definition_id = NULL, face_custom_card_revision_id = NULL WHERE id = p_deck;
    DELETE FROM app.deck_revision WHERE deck_id = p_deck;
    DELETE FROM app.deck WHERE id = p_deck;
  EXCEPTION
    WHEN foreign_key_violation THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck referenced by match history'));
  END;

  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_deck));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
END $$;

CREATE OR REPLACE FUNCTION app.duplicate_deck_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_source_deck uuid,
  p_name text
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  source app.deck;
  new_deck_id uuid;
  new_rev_id uuid;
  clean_name text;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  clean_name := NULLIF(btrim(COALESCE(p_name, '')), '');
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_source_deck IS NULL OR p_source_deck = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id, p_request, 'deck_duplicate_v1', p_source_deck::text,
    jsonb_build_object('sourceDeckId', p_source_deck, 'name', clean_name)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO source FROM app.deck WHERE id = p_source_deck AND account_id = player.id FOR SHARE;
  IF source.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck unavailable'));
  END IF;

  clean_name := COALESCE(clean_name, left(source.name, 56) || ' (Copy)');
  IF length(clean_name) > 64 THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'invalid deck metadata'));
  END IF;

  INSERT INTO app.deck(
    account_id, source_kind, mode, format_id, name,
    back_id, sleeve_id, dice_id, playmat_id, pet_id,
    face_definition_id, face_custom_card_revision_id
  ) VALUES (
    player.id, source.source_kind, source.mode, source.format_id, clean_name,
    source.back_id, source.sleeve_id, source.dice_id, source.playmat_id, source.pet_id,
    source.face_definition_id, source.face_custom_card_revision_id
  ) RETURNING id INTO new_deck_id;

  IF source.current_revision_id IS NOT NULL THEN
    INSERT INTO app.deck_revision(deck_id, revision_number, created_at)
    VALUES (new_deck_id, 1, statement_timestamp())
    RETURNING id INTO new_rev_id;

    INSERT INTO app.deck_entry(revision_id, definition_id, printing_id, section, quantity)
    SELECT new_rev_id, definition_id, printing_id, section, quantity
    FROM app.deck_entry
    WHERE revision_id = source.current_revision_id;

    INSERT INTO app.deck_custom_entry(revision_id, custom_card_revision_id, section, quantity)
    SELECT new_rev_id, custom_card_revision_id, section, quantity
    FROM app.deck_custom_entry
    WHERE revision_id = source.current_revision_id;

    UPDATE app.deck_revision SET sealed_at = statement_timestamp() WHERE id = new_rev_id;
    UPDATE app.deck SET current_revision_id = new_rev_id WHERE id = new_deck_id;
  END IF;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', new_deck_id, 'revisionId', new_rev_id)
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
END $$;

-- ---------------------------------------------------------------------------
-- 4. Matchmaking & Limited Isolation Guards
-- ---------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION app.enqueue_matchmaking_intent(
  p_actor uuid, p_mode text, p_competitive boolean, p_alternatives jsonb, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt;
  alternative jsonb;
  choice jsonb;
  revision_id uuid;
  library app.deck;
  profile app.table_profile_revision;
  configuration jsonb;
  normalized jsonb := '[]';
  choices jsonb;
  ordinal bigint := 0;
  entry_id uuid;
  first_id uuid;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'queue intake requires read committed retry';
  END IF;
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid queue request';
  END IF;
  IF octet_length(jsonb_build_object('mode', p_mode, 'competitive', p_competitive, 'alternatives', p_alternatives)::text) > 262144 THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'queue intent exceeds command envelope';
  END IF;
  receipt := app.begin_account_command(p_actor, p_request, 'queue_intent_v1', p_actor::text, jsonb_build_object('mode', p_mode, 'competitive', p_competitive, 'alternatives', p_alternatives));
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  PERFORM id FROM app.account WHERE id = p_actor AND state = 'active' FOR NO KEY UPDATE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable')); END IF;
  IF p_mode IS NULL OR p_mode NOT IN ('open', 'collection') OR p_competitive IS NULL OR (p_competitive AND p_mode <> 'collection') OR jsonb_typeof(p_alternatives) IS DISTINCT FROM 'array' OR octet_length(p_alternatives::text) > 262144 THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('intent', 'invalid envelope'));
  END IF;
  IF jsonb_array_length(p_alternatives) NOT BETWEEN 1 AND 32 OR (SELECT count(DISTINCT x->>'deckRevisionId') FROM jsonb_array_elements(p_alternatives) x) <> jsonb_array_length(p_alternatives) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('alternatives', 'distinct bounded decks required'));
  END IF;
  FOR alternative IN SELECT * FROM jsonb_array_elements(p_alternatives) LOOP
    IF jsonb_typeof(alternative) IS DISTINCT FROM 'object' OR jsonb_typeof(alternative->'deckRevisionId') IS DISTINCT FROM 'string' OR alternative->>'deckRevisionId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
       OR jsonb_typeof(alternative->'choices') IS DISTINCT FROM 'array' OR EXISTS (SELECT 1 FROM jsonb_object_keys(alternative) k WHERE k NOT IN ('deckRevisionId', 'choices')) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('alternative', 'invalid shape'));
    END IF;
    IF jsonb_array_length(alternative->'choices') NOT BETWEEN 1 AND 128 OR (SELECT count(DISTINCT (x->>'profileId', x->>'configurationKey')) FROM jsonb_array_elements(alternative->'choices') x) <> jsonb_array_length(alternative->'choices') THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('choices', 'distinct bounded configurations required'));
    END IF;
    revision_id := (alternative->>'deckRevisionId')::uuid;
    SELECT d.* INTO library FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = revision_id AND r.sealed_at IS NOT NULL AND d.source_kind = 'account' AND d.account_id = p_actor AND d.mode = p_mode FOR SHARE OF r, d;
    IF library.id IS NULL OR app.deck_revision_has_custom_cards(revision_id) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'owned sealed revision required'));
    END IF;
    choices := '[]';
    FOR choice IN SELECT * FROM jsonb_array_elements(alternative->'choices') LOOP
      IF jsonb_typeof(choice) IS DISTINCT FROM 'object' OR EXISTS (SELECT 1 FROM unnest(ARRAY['profileId', 'configurationKey', 'maximumAiSeats', 'maximumSponsoredAiSeats']) k WHERE jsonb_typeof(choice->k) IS DISTINCT FROM 'string')
         OR choice->>'profileId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR length(btrim(choice->>'configurationKey')) NOT BETWEEN 1 AND 128
         OR choice->>'maximumAiSeats' !~ '^[0-5]$' OR choice->>'maximumSponsoredAiSeats' !~ '^[0-5]$'
         OR EXISTS (SELECT 1 FROM jsonb_object_keys(choice) k WHERE k NOT IN ('profileId', 'configurationKey', 'maximumAiSeats', 'maximumSponsoredAiSeats')) THEN
        RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('choice', 'invalid shape'));
      END IF;
      SELECT * INTO profile FROM app.table_profile_revision WHERE id = (choice->>'profileId')::uuid FOR SHARE;
      SELECT x INTO configuration FROM jsonb_array_elements(profile.matchmaking_config->'configurations') x WHERE x->>'key' = choice->>'configurationKey';
      IF configuration IS NULL OR configuration->>'formatId' IS DISTINCT FROM library.format_id::text OR (choice->>'maximumSponsoredAiSeats')::integer > (choice->>'maximumAiSeats')::integer
         OR (choice->>'maximumAiSeats')::integer >= (configuration->>'seatCount')::integer OR (p_competitive AND (choice->>'maximumAiSeats') <> '0') THEN
        RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('choice', 'unsupported format, profile or AI allowance'));
      END IF;
      choices := choices || jsonb_build_array(choice || jsonb_build_object('configuration', configuration, 'influence', profile.matchmaking_config->'influence', 'planechase', profile.matchmaking_config->'planechase', 'qualificationRef', profile.matchmaking_config->'qualificationRef'));
    END LOOP;
    normalized := normalized || jsonb_build_array(jsonb_build_object('deckRevisionId', revision_id, 'formatId', library.format_id, 'choices', choices));
  END LOOP;
  UPDATE app.command_receipt SET normalized_source = jsonb_build_object('version', 'queue-intent-v1', 'mode', p_mode, 'competitive', p_competitive, 'alternatives', normalized) WHERE id = receipt.id;
  FOR alternative IN SELECT * FROM jsonb_array_elements(normalized) LOOP
    INSERT INTO app.queue_entry(account_id, deck_revision_id, mode, competitive, preferences_ref, state, intent_receipt_id, alternative_index, format_id, preferences)
    VALUES (p_actor, (alternative->>'deckRevisionId')::uuid, p_mode, p_competitive, 'sha256:' || receipt.payload_hash, 'queued', receipt.id, ordinal, (alternative->>'formatId')::uuid, jsonb_build_object('version', 'queue-alternative-v1', 'choices', alternative->'choices')) RETURNING id INTO entry_id;
    first_id := COALESCE(first_id, entry_id);
    ordinal := ordinal + 1;
  END LOOP;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', first_id, 'intentId', receipt.id));
END $$;

CREATE OR REPLACE FUNCTION app.limited_match_seat_pool_proof(
  p_room uuid, p_slot uuid, p_account uuid, p_format uuid, p_release uuid, p_revision uuid
) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT jsonb_build_object('slotId', s.id, 'sourceRevisionId', pool.source_revision_id,
    'poolHash', app.auth_json_hash(pool.printing_counts))
  FROM app.limited_match_seat_pool pool
  JOIN app.room r ON r.id = pool.room_id
  JOIN app.room_slot s ON s.id = p_slot AND s.room_id = r.id
  WHERE pool.room_id = p_room AND pool.account_id = p_account
    AND pool.format_id = p_format AND pool.release_id = p_release AND r.format_id = p_format
    AND r.mode = 'open' AND r.structure = 'bo1' AND NOT r.competitive AND r.visibility = 'private'
    AND NOT r.allow_custom_cards
    AND NOT app.deck_revision_has_custom_cards(p_revision)
    AND s.account_id = p_account AND s.retired_at IS NULL AND s.controller_kind = 'human' AND s.team_ref IS NULL
    AND EXISTS (SELECT 1 FROM app.limited_pilot_member member WHERE member.release_id = p_release AND member.account_id = p_account)
    AND s.deck_revision_id = p_revision
    AND NOT EXISTS (SELECT 1 FROM app.deck_entry e WHERE e.revision_id = p_revision AND e.section NOT IN ('main', 'sideboard'))
    AND COALESCE((SELECT sum(e.quantity) FROM app.deck_entry e WHERE e.revision_id = p_revision AND e.section = 'main'), 0) >= 40
    AND NOT EXISTS (
      SELECT 1 FROM app.deck_entry e WHERE e.revision_id = p_revision AND NOT EXISTS (
        SELECT 1 FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id = program.definition_revision_id
        WHERE program.release_id = p_release AND body.definition_id = e.definition_id AND program.unlimited_basic_land)
      GROUP BY e.printing_id HAVING e.printing_id IS NULL
        OR sum(e.quantity) > COALESCE((pool.printing_counts->>e.printing_id::text)::numeric, 0))
$$;

-- ---------------------------------------------------------------------------
-- 5. Lobby Lending, Custom Toggle, AI Slot Deck Assignment & Ready/Countdown
-- ---------------------------------------------------------------------------

CREATE FUNCTION app.read_host_shared_decks(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  lobby app.room;
  items jsonb;
BEGIN
  IF actor IS NULL OR p_room IS NULL THEN
    RETURN '[]'::jsonb;
  END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room;
  IF lobby.id IS NULL
     OR lobby.mode <> 'open'
     OR lobby.competitive
     OR NOT lobby.share_host_decks
     OR NOT EXISTS (
       SELECT 1 FROM app.room_slot s
       WHERE s.room_id = p_room AND s.retired_at IS NULL AND s.account_id = actor
     ) THEN
    RETURN '[]'::jsonb;
  END IF;

  SELECT COALESCE(jsonb_agg(row_json ORDER BY sort_name, deck_id), '[]'::jsonb)
  INTO items
  FROM (
    SELECT
      d.id AS deck_id,
      d.name AS sort_name,
      jsonb_build_object(
        'deckId', d.id,
        'name', d.name,
        'currentRevisionId', d.current_revision_id,
        'formatId', d.format_id,
        'hasCustomCards', app.deck_revision_has_custom_cards(d.current_revision_id),
        'cardCount', COALESCE((SELECT total_quantity::text FROM app.deck_revision_card_totals(d.current_revision_id)), '0'),
        'faceDefinitionId', d.face_definition_id,
        'faceArtUrl', CASE
          WHEN d.face_definition_id IS NOT NULL THEN app.card_art_url(d.face_definition_id, NULL)
          ELSE NULL
        END,
        'faceCustomCardRevisionId', d.face_custom_card_revision_id,
        'faceArtSha256', (
          SELECT a.art_sha256
          FROM app.custom_card_revision_art a
          WHERE a.revision_id = d.face_custom_card_revision_id
            AND a.face_index = 0
            AND a.art_bytes IS NOT NULL
        )
      ) AS row_json
    FROM app.deck d
    JOIN app.deck_revision r ON r.id = d.current_revision_id AND r.sealed_at IS NOT NULL
    WHERE d.account_id = lobby.host_id
      AND d.source_kind = 'account'
      AND d.mode = 'open'
      AND d.format_id = lobby.format_id
      AND (lobby.allow_custom_cards OR NOT app.deck_revision_has_custom_cards(d.current_revision_id))
    ORDER BY d.name, d.id
    LIMIT 100
  ) limited;

  RETURN items;
END $$;

DROP FUNCTION IF EXISTS app.update_room_options(uuid, uuid, text, boolean, integer);
CREATE FUNCTION app.update_room_options(
  p_room uuid,
  p_format_id uuid,
  p_ai_difficulty text,
  p_planechase_enabled boolean,
  p_range_of_influence integer,
  p_allow_custom_cards boolean DEFAULT NULL,
  p_share_host_decks boolean DEFAULT NULL
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  r app.room;
  next_allow_custom boolean;
  next_share_decks boolean;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  SELECT * INTO r FROM app.room WHERE id = p_room FOR UPDATE;
  IF r.id IS NULL OR r.host_id <> actor OR r.state <> 'lobby' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  IF p_format_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM app.format WHERE id = p_format_id) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('format', 'invalid'));
  END IF;
  IF p_ai_difficulty IS NOT NULL AND p_ai_difficulty NOT IN ('casual', 'standard', 'competitive') THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('aiDifficulty', 'invalid'));
  END IF;
  IF p_range_of_influence IS NOT NULL AND p_range_of_influence NOT BETWEEN 1 AND 5 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('rangeOfInfluence', 'invalid'));
  END IF;

  next_allow_custom := COALESCE(p_allow_custom_cards, r.allow_custom_cards);
  next_share_decks := COALESCE(p_share_host_decks, r.share_host_decks);

  IF (next_allow_custom OR next_share_decks) AND (r.mode <> 'open' OR r.competitive) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('mode', 'open_play_required'));
  END IF;

  IF ((next_allow_custom AND NOT r.allow_custom_cards) OR (next_share_decks AND NOT r.share_host_decks))
     AND app.active_supporter_tier(actor) IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('supporter', 'supporter_required'));
  END IF;

  UPDATE app.room
  SET
    format_id = COALESCE(p_format_id, format_id),
    ai_difficulty = COALESCE(p_ai_difficulty, ai_difficulty),
    planechase_enabled = COALESCE(p_planechase_enabled, planechase_enabled),
    range_of_influence = COALESCE(p_range_of_influence, range_of_influence),
    allow_custom_cards = next_allow_custom,
    share_host_decks = next_share_decks,
    revision = revision + 1
  WHERE id = p_room
  RETURNING * INTO r;

  -- Clear any seated deck that is no longer permitted after toggling custom cards, lending, or format.
  UPDATE app.room_slot s
  SET deck_revision_id = NULL, ready = false
  WHERE s.room_id = p_room
    AND s.retired_at IS NULL
    AND s.deck_revision_id IS NOT NULL
    AND NOT app.room_seat_can_use_deck(
      p_room,
      CASE WHEN s.controller_kind = 'human' THEN s.account_id ELSE s.sponsor_id END,
      s.deck_revision_id,
      s.controller_kind
    );

  IF p_format_id IS NOT NULL OR p_allow_custom_cards IS NOT NULL OR p_share_host_decks IS NOT NULL THEN
    UPDATE app.room_slot SET ready = false WHERE room_id = p_room AND retired_at IS NULL AND controller_kind = 'human';
  END IF;

  RETURN app.read_room_enriched(p_room);
END $$;
REVOKE ALL ON FUNCTION app.update_room_options(uuid, uuid, text, boolean, integer, boolean, boolean)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.update_room_options(uuid, uuid, text, boolean, integer, boolean, boolean) TO authenticated;

CREATE OR REPLACE FUNCTION app.select_room_deck(
  p_actor uuid, p_room uuid, p_expected numeric, p_revision uuid, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid room request';
  END IF;
  receipt := app.begin_account_command(p_actor, p_request, 'room_deck', COALESCE(p_room::text, ''),
    jsonb_build_object('room', p_room, 'expected', p_expected, 'revision', p_revision));
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  PERFORM id FROM app.account WHERE id = p_actor AND state = 'active' FOR NO KEY UPDATE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable')); END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room FOR NO KEY UPDATE;
  IF lobby.id IS NULL OR lobby.state NOT IN ('lobby', 'countdown') OR NOT EXISTS (
    SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND account_id = p_actor AND controller_kind = 'human'
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('room', 'unavailable'));
  END IF;
  IF p_expected IS NULL OR NOT (p_expected >= 0 AND p_expected < power(10::numeric, 78) AND scale(p_expected) = 0)
     OR p_expected <> lobby.revision THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'conflict'));
  END IF;
  SELECT * INTO STRICT slot FROM app.room_slot
  WHERE retired_at IS NULL AND room_id = p_room AND account_id = p_actor AND controller_kind = 'human' FOR NO KEY UPDATE;
  IF p_revision IS NOT NULL THEN
    PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = p_revision FOR SHARE OF r, d;
    IF NOT app.room_seat_can_use_deck(p_room, p_actor, p_revision, 'human') THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable'));
    END IF;
  END IF;
  IF slot.deck_revision_id IS DISTINCT FROM p_revision THEN
    UPDATE app.room_slot SET deck_revision_id = p_revision, ready = false WHERE id = slot.id;
    UPDATE app.room SET state = 'lobby', countdown_at = NULL, revision = revision + 1 WHERE id = p_room;
  END IF;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_room));
END $$;

CREATE OR REPLACE FUNCTION app.apply_set_room_ready(
  p_receipt uuid, p_actor uuid, p_room uuid, p_expected numeric, p_ready boolean, p_revision uuid, p_qualification text, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE receipt app.command_receipt; lobby app.room; slot app.room_slot;
BEGIN
  SELECT * INTO receipt FROM app.command_receipt WHERE id = p_receipt FOR UPDATE;
  IF receipt.id IS NULL OR receipt.state <> 'pending' OR receipt.scope IS DISTINCT FROM 'room_ready'
     OR receipt.principal_key IS DISTINCT FROM 'human:' || p_actor::text OR receipt.target_ref IS DISTINCT FROM COALESCE(p_room::text, '')
     OR receipt.request_ref IS DISTINCT FROM p_request::text
     OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis', p_expected, 'ready', p_ready, 'revision', p_revision, 'qualification', p_qualification)) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'locked set_room_ready core requires its preclaimed exact identity';
  END IF;
  PERFORM id FROM app.account WHERE id = p_actor AND (state = 'active' OR (state = 'deletion_pending' AND p_ready = false)) FOR NO KEY UPDATE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable')); END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room FOR NO KEY UPDATE;
  IF lobby.id IS NULL OR lobby.state NOT IN ('lobby', 'countdown') THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('room', 'unavailable'));
  END IF;
  IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_expected <> lobby.revision THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'conflict'));
  END IF;
  SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND account_id = p_actor AND controller_kind = 'human' FOR NO KEY UPDATE;
  IF slot.id IS NULL OR p_ready IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('ready', 'unavailable'));
  END IF;
  IF p_ready THEN
    IF p_revision IS NULL OR p_revision IS DISTINCT FROM slot.deck_revision_id
       OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'qualified selected revision required'));
    END IF;
    PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = p_revision FOR SHARE OF r, d;
    IF NOT app.room_seat_can_use_deck(p_room, p_actor, p_revision, 'human') THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'qualified selected revision required'));
    END IF;
  END IF;
  IF slot.ready IS DISTINCT FROM p_ready THEN
    UPDATE app.room_slot SET ready = p_ready WHERE id = slot.id;
    IF NOT p_ready THEN
      UPDATE app.room SET state = 'lobby', countdown_at = NULL WHERE id = p_room;
    ELSE
      PERFORM app.refresh_room_countdown(p_room, false);
    END IF;
    UPDATE app.room SET revision = revision + 1 WHERE id = p_room RETURNING * INTO lobby;
  END IF;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_room, 'revision', lobby.revision::text));
END $$;

CREATE OR REPLACE FUNCTION app.refresh_room_countdown(p_room uuid, p_force boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE lobby app.room; profile app.table_profile_revision; occupied bigint;
BEGIN
  IF p_force IS NULL THEN RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'countdown trigger kind required'; END IF;
  SELECT * INTO STRICT lobby FROM app.room WHERE id = p_room FOR NO KEY UPDATE;
  IF lobby.state = 'countdown' OR lobby.state <> 'lobby' THEN RETURN false; END IF;
  SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id = lobby.profile_id;
  SELECT count(*) INTO occupied FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND (account_id IS NOT NULL OR controller_kind = 'ai');
  IF EXISTS (SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND controller_kind = 'ai' AND NOT app.ai_slot_ready(id)) THEN
    RETURN false;
  END IF;
  IF profile.admission_qualification_ref IS NULL
     OR occupied NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
     OR EXISTS (
       SELECT 1 FROM app.room_slot s
       WHERE s.retired_at IS NULL AND s.room_id = p_room
         AND (s.account_id IS NOT NULL OR s.controller_kind = 'ai')
         AND NOT app.room_seat_can_use_deck(
           p_room,
           CASE WHEN s.controller_kind = 'human' THEN s.account_id ELSE s.sponsor_id END,
           s.deck_revision_id,
           s.controller_kind
         )
     ) THEN
    RETURN false;
  END IF;
  IF occupied <> profile.seat_capacity THEN RETURN false; END IF;
  IF NOT p_force AND EXISTS (
    SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND (account_id IS NOT NULL OR controller_kind = 'ai') AND NOT ready
  ) THEN RETURN false; END IF;
  UPDATE app.room SET state = 'countdown', countdown_at = clock_timestamp() + interval '10 seconds' WHERE id = p_room;
  RETURN true;
END $$;

CREATE OR REPLACE FUNCTION app.bootstrap_ai_slot(
  p_actor uuid, p_slot uuid, p_expected numeric, p_revision uuid, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE receipt app.command_receipt; slot app.room_slot; lobby app.room; actors uuid[]; binding_id uuid;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI bootstrap requires read committed retry';
  END IF;
  IF p_slot IS NULL OR p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid AI bootstrap envelope';
  END IF;
  receipt := app.begin_account_command(p_actor, p_request, 'ai_bootstrap_v1', p_slot::text, jsonb_build_object('basis', p_expected::text, 'revision', p_revision));
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id;
  actors := ARRAY[lobby.host_id, slot.sponsor_id, p_actor];
  PERFORM id FROM app.account WHERE id = ANY(actors) ORDER BY id FOR NO KEY UPDATE;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id FOR NO KEY UPDATE;
  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot FOR NO KEY UPDATE;
  IF slot.room_id IS DISTINCT FROM lobby.id OR (slot.sponsor_id IS NOT NULL AND NOT (slot.sponsor_id = ANY(actors))) OR (lobby.host_id IS NOT NULL AND NOT (lobby.host_id = ANY(actors))) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'bootstrap source changed before account/room fence';
  END IF;
  IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind <> 'ai' OR slot.sponsor_id IS NULL OR lobby.state NOT IN ('lobby', 'countdown') OR lobby.revision <> p_expected
     OR p_actor NOT IN (slot.sponsor_id, lobby.host_id) OR EXISTS (SELECT 1 FROM app.account WHERE id = ANY(actors) AND state <> 'active')
     OR EXISTS (SELECT 1 FROM app.ai_runner_binding WHERE room_slot_id = p_slot AND desired_state = 'running') THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('slot', 'stale or unavailable'));
  END IF;
  IF p_revision IS NOT NULL THEN
    PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = p_revision FOR SHARE OF r, d;
    IF NOT app.room_seat_can_use_deck(lobby.id, slot.sponsor_id, p_revision, 'ai') THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable'));
    END IF;
  END IF;
  UPDATE app.command_receipt SET normalized_source = jsonb_build_object('version', 'ai-bootstrap-v1', 'slotId', slot.id, 'sponsorId', slot.sponsor_id, 'roomId', lobby.id, 'configurationId', lobby.configuration_id, 'epoch', '1', 'startKey', receipt.id::text) WHERE id = receipt.id;
  INSERT INTO app.ai_runner_binding(room_slot_id, binding_epoch, desired_state, observed_state, bootstrap_receipt_id, epoch_receipt_id)
  VALUES (slot.id, 1, 'running', 'pending', receipt.id, receipt.id) RETURNING id INTO binding_id;
  UPDATE app.room_slot SET deck_revision_id = p_revision, ready = false WHERE id = slot.id;
  UPDATE app.room_slot SET ready = false WHERE room_id = lobby.id AND retired_at IS NULL AND controller_kind = 'human';
  UPDATE app.room SET state = 'lobby', countdown_at = NULL, revision = revision + 1 WHERE id = lobby.id;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', binding_id, 'epoch', '1', 'startKey', receipt.id));
END $$;

CREATE FUNCTION app.set_ai_slot_deck(
  p_actor uuid,
  p_slot uuid,
  p_expected numeric,
  p_revision uuid,
  p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt;
  slot app.room_slot;
  lobby app.room;
  original_room uuid;
  original_host uuid;
  original_sponsor uuid;
  actors uuid[];
  changed boolean;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI deck change requires read committed retry';
  END IF;
  IF p_actor IS NULL OR p_slot IS NULL OR p_request IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_actor, p_slot, p_request)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid AI deck envelope';
  END IF;

  receipt := app.begin_account_command(
    p_actor, p_request, 'ai_deck_set_v1', p_slot::text,
    jsonb_build_object('basis', p_expected::text, 'deckRevisionId', p_revision)
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;

  SELECT s.room_id, r.host_id, s.sponsor_id
  INTO original_room, original_host, original_sponsor
  FROM app.room_slot s JOIN app.room r ON r.id = s.room_id
  WHERE s.id = p_slot;

  SELECT array_agg(DISTINCT a ORDER BY a) INTO actors
  FROM unnest(ARRAY[p_actor, original_host, original_sponsor]) a WHERE a IS NOT NULL;
  PERFORM id FROM app.account WHERE id = ANY(actors) ORDER BY id FOR NO KEY UPDATE;
  PERFORM id FROM app.queue_entry WHERE claimed_room_id = original_room ORDER BY id FOR NO KEY UPDATE;

  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot FOR NO KEY UPDATE;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id FOR NO KEY UPDATE;
  IF (slot.room_id, slot.sponsor_id, lobby.host_id) IS DISTINCT FROM (original_room, original_sponsor, original_host) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI deck account scope changed before fence';
  END IF;

  IF EXISTS (SELECT 1 FROM app.account WHERE id = ANY(actors) AND state <> 'active') THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable'));
  END IF;
  IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind <> 'ai'
     OR lobby.id IS NULL OR lobby.state NOT IN ('lobby', 'countdown')
     OR (p_actor <> lobby.host_id AND p_actor <> slot.sponsor_id) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('slot', 'stale or unavailable'));
  END IF;
  IF lobby.revision <> p_expected THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'conflict'));
  END IF;
  IF lobby.queue_claim_receipt_id IS NOT NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('queue', 'cancel the claimed room before changing its roster'));
  END IF;

  IF p_revision IS NOT NULL THEN
    PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = p_revision FOR SHARE OF r, d;
    IF NOT app.room_seat_can_use_deck(lobby.id, slot.sponsor_id, p_revision, 'ai') THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable'));
    END IF;
  END IF;

  changed := slot.deck_revision_id IS DISTINCT FROM p_revision;
  IF changed THEN
    -- Update deck_revision_id (which may fire ai_deck_ready_invalidation -> guard_ai_ready_lifecycle,
    -- already incrementing app.room.revision when the AI seat was previously ready).
    UPDATE app.room_slot SET deck_revision_id = p_revision, ready = false WHERE id = slot.id;
    UPDATE app.room_slot SET ready = false WHERE room_id = lobby.id AND retired_at IS NULL AND controller_kind = 'human';
    UPDATE app.room
    SET state = 'lobby',
        countdown_at = NULL,
        configuration_id = gen_random_uuid(),
        revision = CASE WHEN revision = p_expected THEN revision + 1 ELSE revision END
    WHERE id = lobby.id
    RETURNING * INTO lobby;
  END IF;

  RETURN app.finish_account_command(
    receipt.id, true, jsonb_build_object('resourceId', slot.id, 'revision', lobby.revision::text)
  );
END $$;

CREATE FUNCTION app.set_ai_slot_deck_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_slot uuid,
  p_expected numeric,
  p_revision uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  actor_id uuid;
  original_room uuid;
  original_host uuid;
  original_sponsor uuid;
  slot app.room_slot;
  lobby app.room;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI deck change requires read committed retry';
  END IF;
  PERFORM id FROM auth.users WHERE id = p_subject FOR KEY SHARE;
  SELECT id INTO actor_id FROM app.account WHERE auth_subject = p_subject;
  IF actor_id IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'verified live account required';
  END IF;
  SELECT s.room_id, r.host_id, s.sponsor_id
  INTO original_room, original_host, original_sponsor
  FROM app.room_slot s JOIN app.room r ON r.id = s.room_id
  WHERE s.id = p_slot;
  PERFORM id FROM app.account WHERE id IN (actor_id, original_host, original_sponsor) ORDER BY id FOR NO KEY UPDATE;
  player := app.lock_session_account(p_subject, p_session);
  IF player.id IS DISTINCT FROM actor_id THEN
    RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'verified account changed';
  END IF;
  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id;
  IF (slot.room_id, slot.sponsor_id, lobby.host_id) IS DISTINCT FROM (original_room, original_sponsor, original_host) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI deck scope changed before session fence';
  END IF;
  IF p_request IS NULL OR p_slot IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_request, p_slot)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  RETURN app.set_ai_slot_deck(player.id, p_slot, p_expected, p_revision, p_request);
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.read_set_ai_slot_deck_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_slot uuid,
  p_expected numeric,
  p_revision uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI deck lookup requires read committed retry';
  END IF;
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_slot IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_request, p_slot)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  SELECT * INTO receipt
  FROM app.command_receipt
  WHERE principal_key = 'human:' || player.id::text
    AND scope = 'ai_deck_set_v1'
    AND request_ref = p_request::text;
  IF FOUND AND (
    receipt.target_ref IS DISTINCT FROM p_slot::text
    OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis', p_expected::text, 'deckRevisionId', p_revision))
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
  END IF;
  RETURN jsonb_build_object('status', 'success', 'data', app.read_account_command(player.id, 'ai_deck_set_v1', p_request));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
END $$;

CREATE OR REPLACE FUNCTION app.commit_ai_ready_confirmation(
  p_grant uuid, p_epoch numeric, p_revision uuid, p_qualification text, p_authentication text, p_configuration uuid, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room;
  original_sponsor uuid; source_binding uuid; source_slot uuid; fingerprint text; changed boolean; accepted jsonb;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI ready confirmation requires read committed retry'; END IF;
  IF p_configuration IS NULL OR p_grant IS NULL OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_revision IS NULL
     OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 OR p_authentication IS NULL OR length(btrim(p_authentication)) NOT BETWEEN 1 AND 2048
     OR p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid authenticated AI ready envelope'; END IF;
  SELECT b.id, b.room_slot_id, s.sponsor_id INTO source_binding, source_slot, original_sponsor
  FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id = g.binding_id JOIN app.room_slot s ON s.id = b.room_slot_id WHERE g.id = p_grant;
  IF source_binding IS NULL THEN RAISE EXCEPTION USING ERRCODE = '23503', MESSAGE = 'AI controller grant unavailable'; END IF;
  fingerprint := encode(sha256(convert_to(jsonb_build_object('epoch', p_epoch::text, 'revision', p_revision, 'qualification', p_qualification, 'authentication', p_authentication, 'configuration', p_configuration)::text, 'UTF8')), 'hex');
  INSERT INTO app.command_receipt(controller_grant_id, principal_key, scope, target_ref, request_ref, payload_hash, state, result_ref)
  VALUES (p_grant, 'controller:' || p_grant::text, 'ai_ready_v1', source_binding::text, p_request::text, fingerprint, 'pending', '') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
  IF receipt.id IS NULL THEN
    SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key = 'controller:' || p_grant::text AND scope = 'ai_ready_v1' AND request_ref = p_request::text FOR UPDATE;
    IF receipt.target_ref <> source_binding::text OR receipt.payload_hash <> fingerprint THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'AI ready source terms conflict'; END IF;
    IF receipt.state = 'pending' THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI ready disposition pending'; END IF;
    RETURN receipt.response_payload;
  END IF;
  PERFORM id FROM app.account WHERE id = original_sponsor AND state = 'active' FOR NO KEY UPDATE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('sponsor', 'unavailable')); END IF;
  SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id = source_slot;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id FOR NO KEY UPDATE;
  SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id = source_slot FOR NO KEY UPDATE;
  IF slot.id IS NULL THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('slot', 'retired or unavailable')); END IF;
  SELECT * INTO binding FROM app.ai_runner_binding WHERE id = source_binding FOR NO KEY UPDATE;
  IF slot.sponsor_id IS DISTINCT FROM original_sponsor OR binding.room_slot_id <> slot.id THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI sponsor or slot changed before fence'; END IF;
  IF lobby.configuration_id IS DISTINCT FROM p_configuration THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('configuration', 'stale')); END IF;
  IF binding.binding_epoch <> p_epoch OR binding.desired_state <> 'running' OR binding.observed_state NOT IN ('running', 'ready') OR binding.provider_runner_ref IS NULL
     OR (lobby.state NOT IN ('lobby', 'countdown') AND NOT (lobby.state = 'started' AND EXISTS (SELECT 1 FROM app.match m JOIN app.match_seat seat ON seat.match_id = m.id WHERE seat.id = binding.match_seat_id AND m.room_id = lobby.id AND m.state = 'ready')))
     OR NOT EXISTS (SELECT 1 FROM app.controller_grant WHERE id = p_grant AND binding_id = binding.id AND binding_epoch = p_epoch AND match_seat_id IS NOT DISTINCT FROM binding.match_seat_id AND state = 'active' AND (issue_receipt_id IS NULL OR delivery_state = 'delivered') FOR SHARE) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('controller', 'stale or unavailable'));
  END IF;
  PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id = r.deck_id WHERE r.id = p_revision FOR SHARE OF r, d;
  IF NOT app.room_seat_can_use_deck(lobby.id, original_sponsor, p_revision, 'ai') THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable'));
  END IF;
  IF lobby.state = 'started' AND slot.deck_revision_id IS DISTINCT FROM p_revision THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'accepted selection is immutable'));
  END IF;
  changed := slot.deck_revision_id IS DISTINCT FROM p_revision;
  UPDATE app.ai_runner_binding SET observed_state = 'ready', ready_configuration_id = p_configuration, ready_epoch = p_epoch, ready_deck_revision_id = p_revision, ready_qualification_ref = p_qualification, ready_authentication_ref = p_authentication WHERE id = binding.id;
  UPDATE app.room_slot SET deck_revision_id = p_revision, ready = true WHERE id = slot.id;
  IF lobby.state IN ('lobby', 'countdown') THEN
    IF changed THEN
      UPDATE app.room_slot SET ready = false WHERE retired_at IS NULL AND room_id = lobby.id AND controller_kind = 'human';
      UPDATE app.room SET state = 'lobby', countdown_at = NULL WHERE id = lobby.id;
    END IF;
    UPDATE app.room SET revision = revision + 1 WHERE id = lobby.id;
  END IF;
  accepted := app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', binding.id, 'epoch', p_epoch::text, 'revisionId', p_revision, 'qualificationRef', p_qualification, 'authenticationRef', p_authentication, 'configurationId', p_configuration));
  IF lobby.state IN ('lobby', 'countdown') THEN PERFORM app.refresh_room_countdown(lobby.id, false); END IF;
  RETURN accepted;
END $$;

-- ---------------------------------------------------------------------------
-- 6. Native Preparation, Admission, Match Start, Presentation & BO3 (-v1 official / -v2 custom)
-- ---------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION app.native_deck_source(p_release uuid, p_revision uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE entries jsonb; projected bigint; unique_entries bigint; selected bigint;
BEGIN
  SELECT count(*) INTO selected FROM app.deck_entry WHERE revision_id = p_revision;
  SELECT count(*), count(DISTINCT e.id), COALESCE(jsonb_agg(jsonb_build_object(
    'entryId', e.id, 'definitionId', e.definition_id, 'oracleRef', definition.oracle_ref,
    'definitionRevisionId', body.id, 'definitionRevisionRef', body.revision_ref, 'rulesTextRef', body.rules_text_ref,
    'programRef', p.program_ref, 'unlimitedBasicLand', p.unlimited_basic_land,
    'printing', CASE WHEN printing.id IS NULL THEN NULL ELSE jsonb_build_object(
      'id', printing.id, 'externalPrintingRef', printing.external_printing_ref, 'finish', printing.finish, 'setId', printing.set_id
    ) END,
    'section', e.section, 'quantity', e.quantity::text
  ) ORDER BY e.id), '[]'::jsonb)
  INTO projected, unique_entries, entries
  FROM app.deck_entry e
  JOIN app.card_definition definition ON definition.id = e.definition_id
  JOIN app.card_definition_revision body ON body.definition_id = e.definition_id
  JOIN app.release_card_program p ON p.definition_revision_id = body.id AND p.release_id = p_release
  LEFT JOIN app.command_receipt publication ON publication.id = body.catalogue_receipt_id
  LEFT JOIN app.printing printing ON printing.id = e.printing_id
  WHERE e.revision_id = p_revision AND CASE
    WHEN body.catalogue_receipt_id IS NULL THEN body.revision_ref NOT LIKE 'catalogue:%'
    ELSE publication.principal_key = 'cataloguer:scryfall-metadata-v1' AND publication.scope = 'catalogue_source'
      AND publication.state = 'accepted' AND body.revision_ref = 'catalogue:' || publication.id::text
      AND body.rules_text_ref = (publication.result_ref::jsonb->>'artifactRef') || '#oracle=' || definition.oracle_ref
    END;
  IF projected <> selected OR unique_entries <> selected THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native deck publication source is incomplete';
  END IF;
  RETURN entries;
END $$;

CREATE FUNCTION app.native_custom_deck_source(p_revision uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'entryId', ce.id,
    'customCardRevisionId', cr.id,
    'customCardId', cr.custom_card_id,
    'rulesSha256', cr.rules_sha256,
    'name', cr.name,
    'layout', cr.layout,
    'rarity', cr.rarity,
    'facesRules', cr.faces_rules_json,
    'compilerRevision', cr.compiler_revision,
    'section', ce.section,
    'quantity', ce.quantity::text
  ) ORDER BY ce.id), '[]'::jsonb)
  FROM app.deck_custom_entry ce
  JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
  WHERE ce.revision_id = p_revision
$$;

CREATE OR REPLACE FUNCTION app.read_native_room_source(
  p_room uuid, p_basis numeric, p_release uuid, p_legality uuid
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  lobby app.room; profile app.table_profile_revision; release app.qualified_release;
  slot app.room_slot; selected app.deck_revision; entries jsonb; custom_entries jsonb;
  slots jsonb := '[]'; terms jsonb; source_status text;
  official_count bigint; custom_count bigint; total_count bigint; total_quantity numeric;
  use_v2 boolean := false;
  unavailable constant jsonb := '{"status":"fail","data":{"source":"unavailable"}}'::jsonb;
BEGIN
  IF p_room IS NULL OR p_release IS NULL OR p_legality IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_room, p_release, p_legality)
     OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RETURN unavailable;
  END IF;
  SELECT * INTO lobby FROM app.room WHERE id = p_room AND revision = p_basis AND state IN ('lobby', 'countdown');
  IF lobby.id IS NULL THEN RETURN unavailable; END IF;
  SELECT * INTO profile FROM app.table_profile_revision WHERE id = lobby.profile_id;
  SELECT * INTO release FROM app.qualified_release WHERE id = p_release;
  IF profile.id IS NULL OR profile.admission_qualification_ref IS NULL OR release.id IS NULL
     OR NOT EXISTS (SELECT 1 FROM app.legality_revision WHERE id = p_legality)
     OR (SELECT count(*) FROM app.room_slot WHERE room_id = lobby.id AND retired_at IS NULL
         AND (account_id IS NOT NULL OR sponsor_id IS NOT NULL)) NOT BETWEEN 1 AND 6 THEN
    RETURN unavailable;
  END IF;

  use_v2 := lobby.allow_custom_cards OR EXISTS (
    SELECT 1 FROM app.room_slot s
    WHERE s.room_id = lobby.id AND s.retired_at IS NULL
      AND s.deck_revision_id IS NOT NULL
      AND app.deck_revision_has_custom_cards(s.deck_revision_id)
  );

  FOR slot IN
    SELECT * FROM app.room_slot
    WHERE room_id = lobby.id AND retired_at IS NULL
      AND (account_id IS NOT NULL OR sponsor_id IS NOT NULL)
    ORDER BY seat_index
  LOOP
    entries := NULL;
    custom_entries := NULL;
    source_status := 'unselected';
    IF slot.deck_revision_id IS NOT NULL THEN
      source_status := 'unsupported';
      SELECT * INTO selected FROM app.deck_revision WHERE id = slot.deck_revision_id AND sealed_at IS NOT NULL;
      IF selected.id IS NOT NULL AND app.room_seat_can_use_deck(
        lobby.id,
        CASE WHEN slot.controller_kind = 'human' THEN slot.account_id ELSE slot.sponsor_id END,
        selected.id,
        slot.controller_kind
      ) THEN
        SELECT count(*) INTO official_count FROM app.deck_entry WHERE revision_id = selected.id;
        SELECT count(*) INTO custom_count FROM app.deck_custom_entry WHERE revision_id = selected.id;
        SELECT entry_count, total_quantity INTO total_count, total_quantity FROM app.deck_revision_card_totals(selected.id);
        IF EXISTS (
          SELECT 1 FROM app.deck_entry e WHERE e.revision_id = selected.id AND (
            (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id = p.definition_revision_id WHERE p.release_id = p_release AND b.definition_id = e.definition_id) > 1
            OR (e.printing_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM app.printing WHERE id = e.printing_id AND definition_id = e.definition_id))
          )
        ) THEN
          RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native room card identity is inconsistent';
        END IF;
        IF total_count BETWEEN 1 AND 1000 AND total_quantity <= 1000
           AND NOT EXISTS (
             SELECT 1 FROM app.deck_entry e WHERE e.revision_id = selected.id AND NOT EXISTS (
               SELECT 1 FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id = p.definition_revision_id
               WHERE p.release_id = p_release AND b.definition_id = e.definition_id
             )
           )
           AND NOT EXISTS (
             SELECT 1 FROM app.deck_custom_entry ce JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
             WHERE ce.revision_id = selected.id AND NOT cr.stage2_playable
           ) THEN
          entries := app.native_deck_source(p_release, selected.id);
          IF jsonb_array_length(entries) IS DISTINCT FROM official_count THEN
            RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native room entry projection is incomplete';
          END IF;
          IF use_v2 THEN
            custom_entries := app.native_custom_deck_source(selected.id);
            IF jsonb_array_length(custom_entries) IS DISTINCT FROM custom_count THEN
              RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native room custom entry projection is incomplete';
            END IF;
          END IF;
          source_status := 'available';
        END IF;
      END IF;
    END IF;

    slots := slots || jsonb_build_array(
      jsonb_build_object(
        'slotId', slot.id,
        'seatIndex', slot.seat_index::text,
        'accountId', slot.account_id,
        'sponsorId', slot.sponsor_id,
        'controllerKind', slot.controller_kind,
        'teamRef', slot.team_ref,
        'ready', slot.ready,
        'sourceStatus', source_status,
        'selectedRevisionId', slot.deck_revision_id,
        'entries', entries
      ) || CASE WHEN use_v2 THEN jsonb_build_object('customEntries', custom_entries) ELSE '{}'::jsonb END
    );
  END LOOP;

  terms := jsonb_build_object(
    'version', CASE WHEN use_v2 THEN 'native-room-source-v2' ELSE 'native-room-source-v1' END,
    'room', jsonb_build_object(
      'id', lobby.id, 'revision', lobby.revision::text, 'hostId', lobby.host_id, 'state', lobby.state,
      'configurationId', lobby.configuration_id, 'profileId', lobby.profile_id, 'formatId', lobby.format_id,
      'mode', lobby.mode, 'competitive', lobby.competitive, 'structure', lobby.structure,
      'countdownAt', to_char(lobby.countdown_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
    ) || CASE WHEN use_v2 THEN jsonb_build_object('allowCustomCards', lobby.allow_custom_cards, 'shareHostDecks', lobby.share_host_decks) ELSE '{}'::jsonb END,
    'profile', jsonb_build_object(
      'id', profile.id, 'key', profile.key, 'revisionRef', profile.revision_ref, 'rulesRef', profile.rules_ref,
      'planarPoolRef', profile.planar_pool_ref, 'minimumSeats', profile.minimum_seats::text,
      'seatCapacity', profile.seat_capacity::text, 'qualificationRef', profile.admission_qualification_ref
    ),
    'release', jsonb_build_object(
      'id', release.id, 'releaseRef', release.release_ref, 'kernelRef', release.kernel_ref,
      'rulesRef', release.rules_ref, 'programManifestRef', release.program_manifest_ref, 'evidenceRef', release.evidence_ref
    ),
    'legalityRevisionId', p_legality,
    'slots', slots
  );

  IF octet_length(terms::text) > 2097152 THEN RETURN unavailable; END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('sourceHash', app.auth_json_hash(terms), 'terms', terms));
END $$;

CREATE OR REPLACE FUNCTION app.read_native_between_source(
  p_decision uuid, p_basis numeric, p_selected_revision uuid
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  decision app.between_game_decision; m app.match; seat app.match_seat; selected app.deck_revision;
  original app.deck_revision; library app.deck; profile app.table_profile_revision; release app.qualified_release;
  entries jsonb; custom_entries jsonb; terms jsonb;
  official_count bigint; custom_count bigint; total_count bigint; total_quantity numeric;
  use_v2 boolean;
  unavailable constant jsonb := '{"status":"fail","data":{"source":"unavailable"}}'::jsonb;
BEGIN
  IF p_decision IS NULL OR p_selected_revision IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_decision, p_selected_revision)
     OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RETURN unavailable;
  END IF;
  SELECT * INTO decision FROM app.between_game_decision WHERE id = p_decision AND state = 'pending';
  SELECT * INTO m FROM app.match WHERE id = decision.match_id;
  IF decision.id IS NULL OR m.id IS NULL OR m.revision <> p_basis OR m.state <> 'between_games' OR m.availability <> 'available'
     OR m.current_game_id IS NOT NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL
     OR decision.after_game_id IS NULL OR NOT EXISTS (SELECT 1 FROM app.game WHERE id = decision.after_game_id AND match_id = m.id AND state IN ('completed', 'no_contest'))
     OR EXISTS (SELECT 1 FROM app.game later JOIN app.game prior ON prior.id = decision.after_game_id WHERE later.match_id = m.id AND later.game_index > prior.game_index)
     OR NOT EXISTS (SELECT 1 FROM app.command_receipt a WHERE a.scope = 'match_decks_v1' AND a.principal_key = 'admission:' || m.id::text
       AND a.target_ref = m.id::text AND a.state = 'accepted' AND a.response_payload->'data'->>'releaseId' = m.accepted_release_id::text
       AND a.response_payload->'data'->>'qualificationRef' = m.admission_qualification_ref) THEN
    RETURN unavailable;
  END IF;
  SELECT * INTO seat FROM app.match_seat WHERE id = decision.seat_id AND match_id = m.id;
  SELECT * INTO original FROM app.deck_revision WHERE id = seat.source_revision_id AND sealed_at IS NOT NULL;
  SELECT * INTO selected FROM app.deck_revision WHERE id = p_selected_revision AND sealed_at IS NOT NULL;
  SELECT * INTO library FROM app.deck WHERE id = selected.deck_id;
  IF seat.id IS NULL OR original.id IS NULL OR selected.id IS NULL OR seat.deck_accepted_at IS NULL OR seat.deck_snapshot_ref IS NULL
     OR NOT EXISTS (SELECT 1 FROM app.match_clock WHERE match_seat_id = seat.id AND clock_revision = decision.clock_revision)
     OR NOT (
       (seat.controller_kind = 'human' AND library.source_kind = 'account' AND (
         library.account_id = seat.account_id
         OR (m.mode = 'open' AND NOT m.competitive AND m.share_host_decks AND library.account_id = m.host_id)
       ))
       OR (seat.controller_kind = 'ai' AND (
         library.source_kind = 'system_pool'
         OR (library.source_kind = 'account' AND (
           library.account_id = seat.sponsor_id
           OR (m.mode = 'open' AND NOT m.competitive AND m.share_host_decks AND library.account_id = m.host_id)
         ))
       ))
     ) THEN
    RETURN unavailable;
  END IF;

  -- Check both official AND custom card pool multiset invariance.
  IF EXISTS ((SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = selected.id GROUP BY definition_id, printing_id
      EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id)
    UNION ALL (SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id
      EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = selected.id GROUP BY definition_id, printing_id))
    OR EXISTS ((SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = selected.id GROUP BY custom_card_revision_id
      EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id)
    UNION ALL (SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id
      EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = selected.id GROUP BY custom_card_revision_id)) THEN
    RETURN unavailable;
  END IF;

  SELECT * INTO profile FROM app.table_profile_revision WHERE id = m.profile_id;
  SELECT * INTO release FROM app.qualified_release WHERE id = m.accepted_release_id;
  IF profile.id IS NULL OR profile.admission_qualification_ref IS NULL OR release.id IS NULL THEN RETURN unavailable; END IF;

  SELECT count(*) INTO official_count FROM app.deck_entry WHERE revision_id = selected.id;
  SELECT count(*) INTO custom_count FROM app.deck_custom_entry WHERE revision_id = selected.id;
  SELECT entry_count, total_quantity INTO total_count, total_quantity FROM app.deck_revision_card_totals(selected.id);
  IF total_count NOT BETWEEN 1 AND 1000 OR total_quantity > 1000 THEN RETURN unavailable; END IF;
  IF custom_count > 0 AND (NOT m.allow_custom_cards OR m.mode <> 'open' OR m.competitive) THEN RETURN unavailable; END IF;

  IF EXISTS (
    SELECT 1 FROM app.deck_entry e WHERE e.revision_id = selected.id AND (
      (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id = p.definition_revision_id WHERE p.release_id = release.id AND b.definition_id = e.definition_id) <> 1
      OR (e.printing_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM app.printing WHERE id = e.printing_id AND definition_id = e.definition_id))
    )
  ) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native between-game card identity is inconsistent';
  END IF;

  entries := app.native_deck_source(release.id, selected.id);
  IF jsonb_array_length(entries) IS DISTINCT FROM official_count THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native between-game entry projection is incomplete';
  END IF;

  use_v2 := m.allow_custom_cards OR custom_count > 0;
  IF use_v2 THEN
    custom_entries := app.native_custom_deck_source(selected.id);
  END IF;

  terms := jsonb_build_object(
    'version', CASE WHEN use_v2 THEN 'native-between-source-v2' ELSE 'native-between-source-v1' END,
    'decision', jsonb_build_object(
      'id', decision.id, 'matchId', m.id, 'seatId', seat.id, 'afterGameId', decision.after_game_id,
      'basisRevision', decision.basis_revision::text, 'clockRevision', decision.clock_revision::text,
      'deadlineRevision', decision.deadline_revision::text,
      'deadlineAt', to_char(decision.deadline_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
      'state', decision.state
    ),
    'match', jsonb_build_object(
      'id', m.id, 'revision', m.revision::text, 'mode', m.mode, 'formatId', m.format_id, 'profileId', m.profile_id,
      'legalityRevisionId', m.legality_revision_id, 'releaseId', m.accepted_release_id,
      'availabilityRevision', m.availability_revision::text, 'availability', m.availability, 'structure', m.structure,
      'startedAt', to_char(m.started_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
    ) || CASE WHEN use_v2 THEN jsonb_build_object('allowCustomCards', m.allow_custom_cards, 'shareHostDecks', m.share_host_decks) ELSE '{}'::jsonb END,
    'profile', jsonb_build_object(
      'id', profile.id, 'key', profile.key, 'revisionRef', profile.revision_ref, 'rulesRef', profile.rules_ref,
      'planarPoolRef', profile.planar_pool_ref, 'minimumSeats', profile.minimum_seats::text,
      'seatCapacity', profile.seat_capacity::text, 'qualificationRef', profile.admission_qualification_ref
    ),
    'release', jsonb_build_object(
      'id', release.id, 'releaseRef', release.release_ref, 'kernelRef', release.kernel_ref, 'rulesRef', release.rules_ref,
      'programManifestRef', release.program_manifest_ref, 'evidenceRef', release.evidence_ref
    ),
    'originalRevisionId', seat.source_revision_id,
    'originalSnapshotRef', seat.deck_snapshot_ref,
    'selectedRevisionId', selected.id,
    'entries', entries
  ) || CASE WHEN use_v2 THEN jsonb_build_object('customEntries', custom_entries) ELSE '{}'::jsonb END;

  IF octet_length(terms::text) > 2097152 THEN RETURN unavailable; END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('sourceHash', app.auth_json_hash(terms), 'terms', terms));
END $$;

CREATE OR REPLACE FUNCTION app.read_native_admission_source(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  g app.game; m app.match; profile app.table_profile_revision; release app.qualified_release;
  s app.match_seat; d app.between_game_decision; previous app.game; result app.game_result;
  chosen app.deck_revision; original app.deck_revision; entries jsonb; custom_entries jsonb; seats jsonb := '[]';
  selected uuid; count_seats bigint; count_games bigint; maximum numeric;
  total_entries bigint; total_quantity numeric; use_v2 boolean;
  unavailable constant jsonb := '{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
  IF p_game IS NULL OR p_game = '00000000-0000-0000-0000-000000000000'::uuid THEN RETURN unavailable; END IF;
  SELECT * INTO g FROM app.game WHERE id = p_game;
  IF g.id IS NULL OR g.state <> 'reserved' THEN RETURN unavailable; END IF;
  SELECT * INTO m FROM app.match WHERE id = g.match_id;
  IF m.id IS NULL OR m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL
     OR m.admission_qualification_ref IS NULL OR length(btrim(m.admission_qualification_ref)) = 0
     OR m.state <> 'ready' OR m.reserved_game_id IS DISTINCT FROM g.id
     OR NOT EXISTS (SELECT 1 FROM app.command_receipt source WHERE source.principal_key = 'admission:' || m.id::text
       AND source.scope = 'match_decks_v1' AND source.target_ref = m.id::text AND source.state = 'accepted'
       AND source.response_payload->'data'->>'releaseId' = m.accepted_release_id::text
       AND source.response_payload->'data'->>'qualificationRef' = m.admission_qualification_ref)
     OR NOT EXISTS (SELECT 1 FROM app.game game0 JOIN app.command_receipt source
       ON source.scope = 'room_start_v1' AND source.principal_key = 'room-start:' || m.room_id::text
       AND source.target_ref = m.room_id::text AND source.state = 'accepted'
       AND source.response_payload->'data'->>'resourceId' = m.id::text
       AND source.response_payload->'data'->>'gameId' = game0.id::text
       WHERE game0.match_id = m.id AND game0.game_index = 0) THEN
    RETURN unavailable;
  END IF;

  SELECT * INTO profile FROM app.table_profile_revision WHERE id = m.profile_id;
  SELECT * INTO release FROM app.qualified_release WHERE id = m.accepted_release_id;
  SELECT count(*) INTO count_seats FROM app.match_seat WHERE match_id = m.id;
  IF profile.admission_qualification_ref IS NULL OR count_seats NOT BETWEEN 2 AND 6
     OR count_seats NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
     OR EXISTS (SELECT 1 FROM app.match_seat WHERE match_id = m.id AND (deck_accepted_at IS NULL OR source_revision_id IS NULL OR deck_snapshot_ref IS NULL)) THEN
    RETURN unavailable;
  END IF;

  SELECT count(*), max(game_index) INTO count_games, maximum FROM app.game WHERE match_id = m.id;
  IF release.id IS NULL OR m.current_game_id IS NOT NULL OR g.accepted_revision <> 0
     OR EXISTS (SELECT 1 FROM app.match_result WHERE match_id = m.id)
     OR count_games <> maximum + 1 OR g.game_index <> maximum OR (m.structure = 'bo1' AND count_games <> 1)
     OR EXISTS (SELECT 1 FROM app.game other WHERE other.match_id = m.id AND other.id <> g.id AND (other.state <> 'completed' OR NOT EXISTS (SELECT 1 FROM app.game_result WHERE game_id = other.id)))
     OR EXISTS (SELECT 1 FROM app.between_game_decision WHERE match_id = m.id AND state = 'pending')
     OR EXISTS (SELECT 1 FROM app.game_result WHERE game_id = g.id)
     OR EXISTS (SELECT 1 FROM app.game_context WHERE game_id = g.id)
     OR EXISTS (SELECT 1 FROM app.game_checkpoint WHERE game_id = g.id)
     OR EXISTS (SELECT 1 FROM app.game_event WHERE game_id = g.id)
     OR EXISTS (SELECT 1 FROM app.operational_state WHERE game_id = g.id) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'reserved native admission frontier is inconsistent';
  END IF;

  IF g.game_index > 0 THEN
    SELECT * INTO previous FROM app.game WHERE match_id = m.id AND game_index = g.game_index - 1;
    SELECT * INTO result FROM app.game_result WHERE game_id = previous.id;
    IF m.structure <> 'bo3' OR result.id IS NULL OR NOT EXISTS (
         SELECT 1 FROM app.command_receipt source
         WHERE source.principal_key = 'match-lifecycle:' || m.id::text AND source.scope = 'game_outcome_v1'
           AND source.target_ref = result.id::text AND source.request_ref = result.id::text AND source.state = 'accepted'
           AND source.response_payload->'data'->>'phase' = 'between_games')
       OR (SELECT count(*) FROM app.between_game_decision WHERE match_id = m.id AND after_game_id = previous.id AND state = 'answered') <> count_seats
       OR NOT EXISTS (
         SELECT 1 FROM app.between_game_decision decision JOIN app.command_receipt reply
         ON reply.scope = 'between_game_reply_v1' AND reply.principal_key = 'match-seat:' || decision.seat_id::text
         AND reply.target_ref = decision.id::text AND reply.state = 'accepted'
         AND reply.response_payload->'data'->>'reservedGameId' = g.id::text
         WHERE decision.match_id = m.id AND decision.after_game_id = previous.id) THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'reserved native successor selection is inconsistent';
    END IF;
  END IF;

  use_v2 := m.allow_custom_cards OR EXISTS (
    SELECT 1 FROM app.match_seat ms
    WHERE ms.match_id = m.id AND app.deck_revision_has_custom_cards(ms.source_revision_id)
  );

  FOR s IN SELECT * FROM app.match_seat WHERE match_id = m.id ORDER BY seat_index LOOP
    d := NULL;
    IF g.game_index = 0 THEN
      selected := s.source_revision_id;
    ELSE
      SELECT * INTO d FROM app.between_game_decision WHERE match_id = m.id AND after_game_id = previous.id AND seat_id = s.id AND state = 'answered';
      IF d.id IS NULL THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'reserved native seat selection is missing'; END IF;
      PERFORM app.check_between_game_answer(d.id);
      selected := d.selected_deck_revision_id;
    END IF;
    SELECT * INTO chosen FROM app.deck_revision WHERE id = selected;
    SELECT * INTO original FROM app.deck_revision WHERE id = s.source_revision_id;
    IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR original.sealed_at IS NULL
       OR EXISTS ((SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = selected GROUP BY definition_id, printing_id
           EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id)
         UNION ALL (SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id
           EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = selected GROUP BY definition_id, printing_id))
       OR EXISTS ((SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = selected GROUP BY custom_card_revision_id
           EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id)
         UNION ALL (SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id
           EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = selected GROUP BY custom_card_revision_id)) THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'reserved native deck source is inconsistent';
    END IF;
    SELECT entry_count, total_quantity INTO total_entries, total_quantity FROM app.deck_revision_card_totals(selected);
    IF total_entries NOT BETWEEN 1 AND 1000 OR total_quantity > 1000 THEN RETURN unavailable; END IF;
    IF EXISTS (
      SELECT 1 FROM app.deck_entry e WHERE e.revision_id = selected AND (
        (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id = p.definition_revision_id WHERE p.release_id = m.accepted_release_id AND b.definition_id = e.definition_id) <> 1
        OR NOT EXISTS (SELECT 1 FROM app.card_definition WHERE id = e.definition_id)
        OR (e.printing_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM app.printing WHERE id = e.printing_id AND definition_id = e.definition_id))
      )
    ) THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'reserved native card identity is inconsistent';
    END IF;
    entries := app.native_deck_source(m.accepted_release_id, selected);
    IF use_v2 THEN
      custom_entries := app.native_custom_deck_source(selected);
    END IF;
    seats := seats || jsonb_build_array(
      jsonb_build_object(
        'seatId', s.id, 'seatIndex', s.seat_index::text, 'accountId', s.account_id,
        'controllerKind', s.controller_kind, 'sponsorId', s.sponsor_id,
        'originalRevisionId', s.source_revision_id, 'originalSnapshotRef', s.deck_snapshot_ref,
        'selectedRevisionId', selected, 'selectionDecisionId', d.id, 'selectionReplyRef', d.reply_ref,
        'entries', entries
      ) || CASE WHEN use_v2 THEN jsonb_build_object('customEntries', custom_entries) ELSE '{}'::jsonb END
    );
  END LOOP;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object(
    'version', CASE WHEN use_v2 THEN 'native-admission-source-v2' ELSE 'native-admission-source-v1' END,
    'gameId', g.id, 'matchId', m.id, 'roomId', m.room_id, 'gameIndex', g.game_index::text,
    'matchRevision', m.revision::text, 'mode', m.mode, 'formatId', m.format_id,
    'legalityRevisionId', m.legality_revision_id, 'competitive', m.competitive,
    'structure', m.structure, 'admissionQualificationRef', m.admission_qualification_ref,
    'profile', jsonb_build_object(
      'id', profile.id, 'key', profile.key, 'revisionRef', profile.revision_ref, 'rulesRef', profile.rules_ref,
      'planarPoolRef', profile.planar_pool_ref, 'minimumSeats', profile.minimum_seats::text,
      'seatCapacity', profile.seat_capacity::text, 'qualificationRef', profile.admission_qualification_ref
    ),
    'release', jsonb_build_object(
      'id', release.id, 'releaseRef', release.release_ref, 'kernelRef', release.kernel_ref,
      'rulesRef', release.rules_ref, 'programManifestRef', release.program_manifest_ref, 'evidenceRef', release.evidence_ref
    ),
    'seats', seats
  ) || CASE WHEN use_v2 THEN jsonb_build_object('allowCustomCards', m.allow_custom_cards, 'shareHostDecks', m.share_host_decks) ELSE '{}'::jsonb END);
END $$;

CREATE OR REPLACE FUNCTION app.accept_match_decks(
  p_match uuid, p_expected numeric, p_release uuid, p_qualification text, p_inputs jsonb, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt; source_match app.match; profile app.table_profile_revision;
  actors uuid[]; item jsonb; seat app.match_seat; revision app.deck_revision; library app.deck;
  accepted_at timestamptz; fingerprint text; required_count bigint; has_custom boolean;
  tot_entries bigint;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'match deck acceptance requires read committed retry';
  END IF;
  IF p_match IS NULL OR p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
     OR p_release IS NULL OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
     OR jsonb_typeof(p_inputs) IS DISTINCT FROM 'array' OR octet_length(p_inputs::text) > 1048576 THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid admission identity or artifact envelope';
  END IF;
  fingerprint := encode(sha256(convert_to(jsonb_build_object('basis', p_expected::text, 'release', p_release, 'qualification', p_qualification, 'inputs', p_inputs)::text, 'UTF8')), 'hex');
  INSERT INTO app.command_receipt(principal_key, request_ref, scope, target_ref, payload_hash, state, result_ref)
  VALUES ('admission:' || p_match::text, p_request::text, 'match_decks_v1', p_match::text, fingerprint, 'pending', '')
  ON CONFLICT DO NOTHING RETURNING * INTO receipt;
  IF receipt.id IS NULL THEN
    SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key = 'admission:' || p_match::text AND scope = 'match_decks_v1' AND request_ref = p_request::text FOR UPDATE;
    IF receipt.payload_hash <> fingerprint OR receipt.target_ref <> p_match::text THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'admission request terms conflict';
    END IF;
    IF receipt.state = 'pending' THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'admission disposition is pending'; END IF;
    RETURN receipt.response_payload;
  END IF;
  SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM (
    SELECT account_id a FROM app.match_seat WHERE match_id = p_match AND account_id IS NOT NULL
    UNION SELECT sponsor_id FROM app.match_seat WHERE match_id = p_match AND sponsor_id IS NOT NULL
    UNION SELECT host_id FROM app.match WHERE id = p_match AND host_id IS NOT NULL
  ) roster;
  PERFORM id FROM app.account WHERE id = ANY(actors) ORDER BY id FOR NO KEY UPDATE;
  SELECT * INTO source_match FROM app.match WHERE id = p_match FOR NO KEY UPDATE;
  IF source_match.id IS NULL OR source_match.state <> 'admitting' OR source_match.revision <> p_expected OR EXISTS (SELECT 1 FROM app.game WHERE match_id = p_match)
     OR EXISTS (SELECT 1 FROM app.match_result WHERE match_id = p_match) OR EXISTS (SELECT 1 FROM app.match_seat WHERE match_id = p_match AND deck_accepted_at IS NOT NULL) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('match', 'stale, accepted or started'));
  END IF;
  IF EXISTS (SELECT 1 FROM app.match_seat WHERE match_id = p_match AND ((account_id IS NOT NULL AND NOT (account_id = ANY(COALESCE(actors, '{}'::uuid[])))) OR (sponsor_id IS NOT NULL AND NOT (sponsor_id = ANY(COALESCE(actors, '{}'::uuid[])))))) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'admission roster changed before account fencing';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS (SELECT 1 FROM app.account a WHERE a.id = members.actor_id AND a.state = 'active')) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id = source_match.profile_id;
  SELECT count(*) INTO required_count FROM app.match_seat WHERE match_id = p_match;
  IF profile.admission_qualification_ref IS NULL OR required_count NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
     OR jsonb_array_length(p_inputs) <> required_count OR EXISTS (SELECT 1 FROM app.match_seat WHERE match_id = p_match AND (seat_index < 0 OR seat_index >= profile.seat_capacity)) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('profile', 'qualified roster and complete inputs required'));
  END IF;
  PERFORM id FROM app.qualified_release WHERE id = p_release FOR SHARE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('release', 'unavailable')); END IF;
  PERFORM id FROM app.legality_revision WHERE id = source_match.legality_revision_id FOR SHARE;

  FOR item IN SELECT v FROM jsonb_array_elements(p_inputs) v ORDER BY v->>'seatId' LOOP
    IF jsonb_typeof(item) IS DISTINCT FROM 'object'
       OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string' OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
       OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
       OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
       OR EXISTS (SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId', 'revisionId', 'snapshotRef')) THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid accepted seat input';
    END IF;
    SELECT * INTO seat FROM app.match_seat WHERE id = (item->>'seatId')::uuid AND match_id = p_match FOR NO KEY UPDATE;
    IF seat.id IS NULL THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('seat', 'unavailable')); END IF;
    SELECT * INTO revision FROM app.deck_revision WHERE id = (item->>'revisionId')::uuid FOR SHARE;
    SELECT * INTO library FROM app.deck WHERE id = revision.deck_id FOR SHARE;
    has_custom := revision.id IS NOT NULL AND app.deck_revision_has_custom_cards(revision.id);
    SELECT entry_count INTO tot_entries FROM app.deck_revision_card_totals(revision.id);

    IF revision.id IS NULL OR revision.sealed_at IS NULL OR library.id IS NULL
       OR library.mode <> source_match.mode OR library.format_id <> source_match.format_id
       OR tot_entries < 1
       OR (has_custom AND (NOT source_match.allow_custom_cards OR source_match.mode <> 'open' OR source_match.competitive))
       OR (seat.controller_kind = 'human' AND (
            seat.sponsor_id IS NOT NULL
            OR library.source_kind <> 'account'
            OR (library.account_id IS DISTINCT FROM seat.account_id
                AND NOT (source_match.mode = 'open' AND NOT source_match.competitive AND source_match.share_host_decks AND library.account_id = source_match.host_id))
          ))
       OR (seat.controller_kind = 'ai' AND NOT (
            library.source_kind = 'system_pool'
            OR (library.source_kind = 'account' AND (
              library.account_id = seat.sponsor_id
              OR (source_match.mode = 'open' AND NOT source_match.competitive AND source_match.share_host_decks AND library.account_id = source_match.host_id)
            ))
          )) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable or incompatible'));
    END IF;

    IF has_custom AND EXISTS (
      SELECT 1 FROM app.deck_custom_entry ce
      JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
      WHERE ce.revision_id = revision.id AND NOT cr.stage2_playable
    ) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('cards', 'unsupported or not legal in accepted revision'));
    END IF;

    IF EXISTS (
      SELECT 1 FROM app.deck_entry e WHERE e.revision_id = revision.id AND (
        (SELECT count(*) FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id = program.definition_revision_id WHERE program.release_id = p_release AND body.definition_id = e.definition_id) <> 1
        OR NOT EXISTS (SELECT 1 FROM app.card_legality l WHERE l.definition_id = e.definition_id AND l.format_id = source_match.format_id AND l.legality_revision_id = source_match.legality_revision_id AND l.status IN ('legal', 'restricted'))
      )
    ) THEN
      RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('cards', 'unsupported or not legal in accepted revision'));
    END IF;
  END LOOP;

  IF (SELECT count(DISTINCT v->>'seatId') FROM jsonb_array_elements(p_inputs) v) <> required_count THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('seat', 'duplicate input'));
  END IF;

  PERFORM i.id FROM app.inventory_asset i JOIN app.asset_owner o ON o.id = i.owner_id
  WHERE o.account_id = ANY(actors) ORDER BY i.id FOR NO KEY UPDATE OF i;

  IF source_match.mode = 'collection' AND EXISTS (
    SELECT 1 FROM jsonb_array_elements(p_inputs) input JOIN app.match_seat s ON s.id = (input->>'seatId')::uuid
    JOIN app.deck_entry e ON e.revision_id = (input->>'revisionId')::uuid
    WHERE s.controller_kind = 'human' AND NOT EXISTS (
      SELECT 1 FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id = program.definition_revision_id
      WHERE program.release_id = p_release AND body.definition_id = e.definition_id AND program.unlimited_basic_land)
    GROUP BY s.account_id, e.printing_id HAVING e.printing_id IS NULL OR sum(e.quantity) >
      COALESCE((SELECT i.owned_quantity - COALESCE((SELECT sum(line.quantity) FROM app.hold_line line JOIN app.asset_hold h ON h.id = line.hold_id WHERE line.inventory_asset_id = i.id AND h.state = 'held'), 0)
        FROM app.inventory_asset i JOIN app.asset_owner o ON o.id = i.owner_id JOIN app.asset_definition d ON d.id = i.asset_definition_id
        WHERE o.account_id = s.account_id AND o.state = 'active' AND d.kind = 'card' AND d.printing_id = e.printing_id), 0)
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('collection', 'not enough available copies'));
  END IF;

  accepted_at := clock_timestamp();
  FOR item IN SELECT v FROM jsonb_array_elements(p_inputs) v ORDER BY v->>'seatId' LOOP
    UPDATE app.match_seat SET source_revision_id = (item->>'revisionId')::uuid, deck_snapshot_ref = item->>'snapshotRef', deck_accepted_at = accepted_at WHERE id = (item->>'seatId')::uuid;
  END LOOP;
  UPDATE app.match m SET accepted_release_id = p_release, admission_qualification_ref = p_qualification, revision = m.revision + 1 WHERE m.id = p_match;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_match, 'revision', (p_expected + 1)::text, 'releaseId', p_release, 'qualificationRef', p_qualification));
END $$;

CREATE OR REPLACE FUNCTION app.start_room_match(
  p_room uuid, p_expected numeric, p_release uuid, p_legality uuid, p_qualification text, p_inputs jsonb, p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt; lobby app.room; actors uuid[]; fingerprint text; reserved_match uuid; reserved_game uuid;
  item jsonb; slot app.room_slot; seat_id uuid; deck_inputs jsonb := '[]'; accepted jsonb; failure jsonb; rejected boolean := false; accepted_at timestamptz;
  advanced_ai_count integer := 0; quota_policy record; quota_day date; current_matches integer; current_advanced_seats integer;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'room start requires read committed retry'; END IF;
  IF p_room IS NULL OR p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
     OR p_release IS NULL OR p_legality IS NULL OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
     OR jsonb_typeof(p_inputs) IS DISTINCT FROM 'array' OR octet_length(p_inputs::text) > 1048576 THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid room start envelope';
  END IF;
  fingerprint := encode(sha256(convert_to(jsonb_build_object('basis', p_expected::text, 'release', p_release, 'legality', p_legality, 'qualification', p_qualification, 'inputs', p_inputs)::text, 'UTF8')), 'hex');
  INSERT INTO app.command_receipt(principal_key, request_ref, scope, target_ref, payload_hash, state, result_ref)
  VALUES ('room-start:' || p_room::text, p_request::text, 'room_start_v1', p_room::text, fingerprint, 'pending', '') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
  IF receipt.id IS NULL THEN
    SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key = 'room-start:' || p_room::text AND scope = 'room_start_v1' AND request_ref = p_request::text FOR UPDATE;
    IF receipt.payload_hash <> fingerprint OR receipt.target_ref <> p_room::text THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'room start request terms conflict'; END IF;
    IF receipt.state = 'pending' THEN RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'room start disposition pending'; END IF;
    RETURN receipt.response_payload;
  END IF;
  SELECT array_agg(DISTINCT actor ORDER BY actor) INTO actors FROM (
    SELECT account_id actor FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND account_id IS NOT NULL
    UNION SELECT sponsor_id FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND sponsor_id IS NOT NULL
    UNION SELECT host_id FROM app.room WHERE id = p_room
  ) roster;
  PERFORM id FROM app.account WHERE id = ANY(actors) ORDER BY id FOR NO KEY UPDATE;
  PERFORM id FROM app.queue_entry WHERE claimed_room_id = p_room ORDER BY id FOR NO KEY UPDATE;
  SELECT * INTO lobby FROM app.room WHERE id = p_room FOR NO KEY UPDATE;
  IF lobby.id IS NULL OR lobby.state <> 'countdown' OR lobby.revision <> p_expected OR lobby.countdown_at IS NULL OR lobby.countdown_at > clock_timestamp()
     OR EXISTS (SELECT 1 FROM app.match WHERE room_id = p_room) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('room', 'stale, not due or already reserved'));
  END IF;
  IF EXISTS (SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND ((account_id IS NOT NULL AND NOT (account_id = ANY(COALESCE(actors, '{}'::uuid[])))) OR (sponsor_id IS NOT NULL AND NOT (sponsor_id = ANY(COALESCE(actors, '{}'::uuid[])))))) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'room roster changed before account fence';
  END IF;
  IF EXISTS (SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS (SELECT 1 FROM app.account a WHERE a.id = members.actor_id AND a.state = 'active')) THEN
    failure := jsonb_build_object('account', 'unavailable'); rejected := true;
  END IF;
  IF NOT rejected AND EXISTS (SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND controller_kind = 'ai' AND NOT app.ai_slot_ready(id)) THEN
    failure := jsonb_build_object('ai', 'authenticated current-epoch readiness required'); rejected := true;
  END IF;
  IF NOT rejected THEN
    SELECT count(*)::integer INTO advanced_ai_count
    FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND controller_kind = 'ai' AND ai_tier = 'advanced';
    IF advanced_ai_count > 0 THEN
      quota_day := (clock_timestamp() AT TIME ZONE 'UTC')::date;
      SELECT * INTO quota_policy FROM app.ai_daily_quota_for_account(lobby.host_id);
      INSERT INTO app.ai_usage_daily(account_id, usage_day) VALUES (lobby.host_id, quota_day) ON CONFLICT (account_id, usage_day) DO NOTHING;
      SELECT used_matches, used_advanced_seats INTO STRICT current_matches, current_advanced_seats
      FROM app.ai_usage_daily WHERE account_id = lobby.host_id AND usage_day = quota_day FOR UPDATE;
      IF (quota_policy.daily_limit IS NOT NULL AND current_matches >= quota_policy.daily_limit)
         OR current_advanced_seats + advanced_ai_count > quota_policy.hard_cap THEN
        failure := jsonb_build_object('aiQuota', 'daily advanced AI limit reached'); rejected := true;
      END IF;
    END IF;
  END IF;
  IF NOT rejected THEN
    IF jsonb_array_length(p_inputs) <> (SELECT count(*) FROM app.room_slot WHERE retired_at IS NULL AND room_id = p_room AND (account_id IS NOT NULL OR controller_kind = 'ai')) THEN
      failure := jsonb_build_object('inputs', 'complete occupied roster required'); rejected := true;
    END IF;
  END IF;
  IF NOT rejected AND lobby.queue_claim_receipt_id IS NOT NULL THEN PERFORM app.check_queue_claim(p_room); END IF;
  IF NOT rejected THEN
    BEGIN
      INSERT INTO app.match(
        room_id, format_id, profile_id, legality_revision_id, mode, competitive, structure,
        state, revision, allow_custom_cards, share_host_decks, host_id
      ) VALUES (
        p_room, lobby.format_id, lobby.profile_id, p_legality, lobby.mode, lobby.competitive, lobby.structure,
        'admitting', 0, lobby.allow_custom_cards, lobby.share_host_decks, lobby.host_id
      ) RETURNING id INTO reserved_match;

      FOR item IN SELECT * FROM jsonb_array_elements(p_inputs) LOOP
        IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
           OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
           OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
           OR EXISTS (SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId', 'revisionId', 'snapshotRef')) THEN
          RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid room start seat artifact';
        END IF;
        SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id = (item->>'slotId')::uuid AND room_id = p_room AND (account_id IS NOT NULL OR controller_kind = 'ai') FOR NO KEY UPDATE;
        IF slot.id IS NULL OR slot.deck_revision_id IS DISTINCT FROM (item->>'revisionId')::uuid THEN
          failure := jsonb_build_object('deck', 'selected revision changed');
          RAISE EXCEPTION USING ERRCODE = 'SW001', MESSAGE = 'room admission rejected';
        END IF;
        INSERT INTO app.match_seat(match_id, account_id, sponsor_id, seat_index, team_ref, controller_kind, ai_tier)
        VALUES (reserved_match, slot.account_id, slot.sponsor_id, slot.seat_index, slot.team_ref, slot.controller_kind, slot.ai_tier) RETURNING id INTO seat_id;
        deck_inputs := deck_inputs || jsonb_build_array(jsonb_build_object('seatId', seat_id, 'revisionId', item->>'revisionId', 'snapshotRef', item->>'snapshotRef'));
      END LOOP;
      accepted := app.accept_match_decks(reserved_match, 0, p_release, p_qualification, deck_inputs, receipt.id);
      IF accepted->>'status' IS DISTINCT FROM 'success' THEN
        failure := accepted->'data';
        RAISE EXCEPTION USING ERRCODE = 'SW001', MESSAGE = 'room admission rejected';
      END IF;
      accepted_at := clock_timestamp();
      INSERT INTO app.game(match_id, game_index, state, accepted_revision) VALUES (reserved_match, 0, 'reserved', 0) RETURNING id INTO reserved_game;
      INSERT INTO app.match_clock(match_seat_id, remaining_ms, clock_revision, policy_ref, paused, last_accounted_at)
      SELECT seat.id, CASE WHEN lobby.structure = 'bo3' THEN 1800000 ELSE 0 END, 0, CASE WHEN lobby.structure = 'bo3' THEN 'bo3-overall-bank-v1' ELSE 'bo1-bank-inactive-v1' END, true, accepted_at FROM app.match_seat seat WHERE seat.match_id = reserved_match;
      UPDATE app.match m SET state = 'ready', started_at = accepted_at, reserved_game_id = reserved_game WHERE m.id = reserved_match;
      PERFORM app.adopt_room_ai(p_room, reserved_match, receipt.id);
      UPDATE app.ai_runner_binding b SET ai_tier = s.ai_tier FROM app.room_slot s WHERE b.room_slot_id = s.id AND b.desired_state = 'running' AND s.room_id = p_room;
      IF advanced_ai_count > 0 THEN
        UPDATE app.ai_usage_daily SET used_matches = used_matches + 1, used_advanced_seats = used_advanced_seats + advanced_ai_count, updated_at = accepted_at
        WHERE account_id = lobby.host_id AND usage_day = quota_day;
        INSERT INTO app.ai_usage_reservation(match_id, account_id, usage_day, advanced_seats, state, created_at, updated_at)
        VALUES (reserved_match, lobby.host_id, quota_day, advanced_ai_count, 'reserved', accepted_at, accepted_at);
      END IF;
      PERFORM app.ensure_game_conversation(reserved_match);
      INSERT INTO app.durable_handoff(match_id, game_id, source_ref, destination, kind, state) VALUES (reserved_match, reserved_game, reserved_match::text, 'game_host', 'ready_match', 'pending');
    EXCEPTION WHEN SQLSTATE 'SW001' THEN rejected := true;
    END;
  END IF;
  IF rejected THEN
    UPDATE app.room_slot SET ready = false WHERE retired_at IS NULL AND room_id = p_room;
    UPDATE app.room SET state = 'lobby', countdown_at = NULL, revision = revision + 1 WHERE id = p_room;
    RETURN app.finish_account_command(receipt.id, false, failure);
  END IF;
  UPDATE app.queue_entry SET state = 'reserved', reserved_match_id = reserved_match WHERE claimed_room_id = p_room AND state = 'claimed';
  UPDATE app.room SET state = 'started', countdown_at = NULL, revision = revision + 1 WHERE id = p_room;
  UPDATE app.invitation SET state = 'canceled' WHERE room_id = p_room AND state = 'active';
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', reserved_match, 'gameId', reserved_game));
END $$;

CREATE OR REPLACE FUNCTION app.read_native_human_presentation(
  p_subject uuid, p_session uuid, p_game uuid, p_context uuid, p_free jsonb
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  access jsonb; matched app.match; seat app.match_seat; revision app.deck_revision;
  library app.deck; owner_id uuid; appearance_owner_id uuid; lifecycle text; selection jsonb; items jsonb := '[]';
  slot_kind text; slot_key text; free_id uuid;
  unauthorized constant jsonb := '{"status":"fail","data":{"session":"unauthorized"}}'::jsonb;
  unavailable constant jsonb := '{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
  no_presentation constant jsonb := '{"status":"fail","data":{"presentation":"unavailable"}}'::jsonb;
BEGIN
  IF p_subject IS NULL OR p_session IS NULL OR p_game IS NULL OR p_context IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_subject, p_session, p_game, p_context) THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid native presentation identity';
  END IF;
  IF NOT app.valid_appearance_selection(p_free) OR p_free->'playmatId' IS DISTINCT FROM 'null'::jsonb THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid native free presentation';
  END IF;
  FOREACH slot_kind IN ARRAY ARRAY['back', 'sleeve', 'dice'] LOOP
    slot_key := slot_kind || 'Id';
    free_id := (p_free->>slot_key)::uuid;
    IF p_free->>slot_key IS DISTINCT FROM free_id::text OR NOT EXISTS (
      SELECT 1 FROM app.cosmetic c WHERE c.id = free_id AND c.kind = slot_kind AND c.is_free AND c.state = 'active'
    ) THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'native free presentation is not configured';
    END IF;
  END LOOP;
  access := app.read_native_human_access(p_subject, p_session, p_game, p_context);
  IF access = unauthorized OR access = unavailable THEN RETURN access; END IF;
  IF access->>'status' IS DISTINCT FROM 'success' THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'native presentation access is inconsistent';
  END IF;
  SELECT * INTO matched FROM app.match WHERE id = (access->'data'->'source'->>'matchId')::uuid;
  IF matched.id IS NULL THEN RETURN no_presentation; END IF;

  FOR seat IN SELECT * FROM app.match_seat WHERE match_id = matched.id ORDER BY seat_index LOOP
    SELECT * INTO revision FROM app.deck_revision WHERE id = seat.source_revision_id;
    SELECT * INTO library FROM app.deck WHERE id = revision.deck_id;
    IF seat.deck_accepted_at IS NULL OR revision.id IS NULL OR revision.sealed_at IS NULL OR library.id IS NULL THEN
      RETURN no_presentation;
    END IF;
    IF seat.controller_kind = 'human' THEN
      owner_id := seat.account_id;
      IF owner_id IS NULL OR seat.sponsor_id IS NOT NULL OR library.source_kind <> 'account' THEN
        RETURN no_presentation;
      END IF;
      IF library.account_id IS DISTINCT FROM owner_id THEN
        IF NOT (matched.mode = 'open' AND NOT matched.competitive AND matched.share_host_decks AND library.account_id = matched.host_id) THEN
          RETURN no_presentation;
        END IF;
      END IF;
      appearance_owner_id := library.account_id;
    ELSIF seat.controller_kind = 'ai' THEN
      owner_id := seat.sponsor_id;
      IF owner_id IS NULL OR seat.account_id IS NOT NULL THEN RETURN no_presentation; END IF;
      IF library.source_kind = 'account' THEN
        IF library.account_id IS DISTINCT FROM owner_id
           AND NOT (matched.mode = 'open' AND NOT matched.competitive AND matched.share_host_decks AND library.account_id = matched.host_id) THEN
          RETURN no_presentation;
        END IF;
        appearance_owner_id := library.account_id;
      ELSIF library.source_kind = 'system_pool' THEN
        IF library.account_id IS NOT NULL OR library.system_pool_ref IS NULL OR library.source_manifest_ref IS NULL THEN
          RETURN no_presentation;
        END IF;
        appearance_owner_id := owner_id;
      ELSE
        RETURN no_presentation;
      END IF;
    ELSE
      RETURN no_presentation;
    END IF;

    SELECT state INTO lifecycle FROM app.account WHERE id = appearance_owner_id;
    IF lifecycle IS NULL OR lifecycle NOT IN ('active', 'deletion_pending', 'deleted') THEN
      RETURN no_presentation;
    END IF;
    IF lifecycle = 'deleted' THEN
      selection := p_free;
    ELSE
      selection := app.resolve_account_appearance(appearance_owner_id, library.back_id, library.sleeve_id, library.dice_id, library.playmat_id);
    END IF;
    items := items || jsonb_build_array(jsonb_build_object('seatId', seat.id, 'appearance', selection));
  END LOOP;

  IF jsonb_array_length(items) NOT BETWEEN 2 AND 6 THEN RETURN no_presentation; END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object(
    'version', 'native-presentation-v1',
    'access', access->'data',
    'presentation', jsonb_build_object('seats', items)
  ));
END $$;

CREATE OR REPLACE FUNCTION app.apply_between_game_reply(
  p_receipt uuid, p_decision uuid, p_expected numeric, p_revision uuid, p_reply text, p_qualification text, p_expiry uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  d app.between_game_decision; m app.match; seat app.match_seat; chosen app.deck_revision; original app.deck_revision;
  receipt app.command_receipt; expiry app.command_receipt; game_id uuid; prior_index numeric; current_clock numeric;
  input jsonb; accepted_at timestamptz;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'between-game selection requires read committed retry';
  END IF;
  input := jsonb_build_object('decision', p_decision, 'basis', p_expected::text, 'revision', p_revision, 'reply', p_reply, 'qualification', p_qualification);
  SELECT * INTO d FROM app.between_game_decision WHERE id = p_decision;
  SELECT * INTO receipt FROM app.command_receipt WHERE id = p_receipt;
  IF receipt.state IS DISTINCT FROM 'pending' OR receipt.scope IS DISTINCT FROM 'between_game_reply_v1' OR receipt.principal_key IS DISTINCT FROM 'match-seat:' || d.seat_id::text OR receipt.target_ref IS DISTINCT FROM d.id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(input) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'locked reply core requires its preclaimed exact identity';
  END IF;
  SELECT * INTO STRICT m FROM app.match WHERE id = d.match_id FOR NO KEY UPDATE;
  SELECT * INTO STRICT d FROM app.between_game_decision WHERE id = d.id FOR NO KEY UPDATE;
  SELECT * INTO STRICT seat FROM app.match_seat WHERE id = d.seat_id FOR SHARE;
  SELECT clock_revision INTO current_clock FROM app.match_clock WHERE match_seat_id = seat.id FOR SHARE;
  IF m.availability <> 'available' OR m.revision <> p_expected OR m.state <> 'between_games' OR d.state <> 'pending' OR d.after_game_id IS NULL OR current_clock IS DISTINCT FROM d.clock_revision
     OR EXISTS (SELECT 1 FROM app.game g JOIN app.game prior ON prior.id = d.after_game_id WHERE g.match_id = m.id AND g.game_index > prior.game_index) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('basis', 'stale or unavailable between-game decision'));
  END IF;
  PERFORM app.check_started_match_frontier(m.id);
  PERFORM app.check_between_deadline(d.id);
  SELECT * INTO chosen FROM app.deck_revision WHERE id = p_revision FOR SHARE;
  SELECT * INTO original FROM app.deck_revision WHERE id = seat.source_revision_id FOR SHARE;
  IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR NOT EXISTS (
    SELECT 1 FROM app.deck deck WHERE deck.id = chosen.deck_id AND (
      (seat.controller_kind = 'human' AND deck.source_kind = 'account' AND (
        deck.account_id = seat.account_id
        OR (m.mode = 'open' AND NOT m.competitive AND m.share_host_decks AND deck.account_id = m.host_id)
      ))
      OR (seat.controller_kind = 'ai' AND (
        deck.source_kind = 'system_pool'
        OR (deck.source_kind = 'account' AND (
          deck.account_id = seat.sponsor_id
          OR (m.mode = 'open' AND NOT m.competitive AND m.share_host_decks AND deck.account_id = m.host_id)
        ))
      ))
    )
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'unavailable selected revision'));
  END IF;

  IF EXISTS ((SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = chosen.id GROUP BY definition_id, printing_id
      EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id)
    UNION ALL (SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = original.id GROUP BY definition_id, printing_id
      EXCEPT SELECT definition_id, printing_id, sum(quantity) FROM app.deck_entry WHERE revision_id = chosen.id GROUP BY definition_id, printing_id))
    OR EXISTS ((SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = chosen.id GROUP BY custom_card_revision_id
      EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id)
    UNION ALL (SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = original.id GROUP BY custom_card_revision_id
      EXCEPT SELECT custom_card_revision_id, sum(quantity) FROM app.deck_custom_entry WHERE revision_id = chosen.id GROUP BY custom_card_revision_id)) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'selection changes the immutable original card pool'));
  END IF;

  accepted_at := clock_timestamp();
  IF d.deadline_policy_ref IS NOT NULL AND d.deadline_at <= accepted_at THEN
    IF p_expiry IS NULL THEN RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deadline', 'qualified cutoff has elapsed')); END IF;
    SELECT * INTO expiry FROM app.command_receipt WHERE id = p_expiry;
    IF expiry.state IS DISTINCT FROM 'pending' OR expiry.scope IS DISTINCT FROM 'between_deadline_expiry_v1' OR expiry.principal_key IS DISTINCT FROM 'between-expiry:' || d.id::text OR expiry.target_ref IS DISTINCT FROM d.id::text OR expiry.normalized_source->>'replyReceiptId' IS DISTINCT FROM receipt.id::text OR expiry.normalized_source->'replyInput' IS DISTINCT FROM input OR expiry.normalized_source->'snapshot' IS DISTINCT FROM app.between_deadline_snapshot(d.id) OR expiry.normalized_source->>'observedAt' IS NULL OR app.financial_instant(expiry.normalized_source->>'observedAt') < d.deadline_at OR app.financial_instant(expiry.normalized_source->>'observedAt') > accepted_at THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'expired selection requires its exact qualified expiry producer';
    END IF;
  ELSIF p_expiry IS NOT NULL THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'qualified deadline is not due';
  END IF;
  UPDATE app.command_receipt SET normalized_source = jsonb_build_object('version', 'between-game-reply-v1', 'input', input, 'acceptedAt', to_char(accepted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'), 'deadlineSnapshot', CASE WHEN d.deadline_policy_ref IS NOT NULL THEN app.between_deadline_snapshot(d.id) END, 'expiryReceiptId', p_expiry) WHERE id = receipt.id;
  UPDATE app.between_game_decision SET state = 'answered', selected_deck_revision_id = p_revision, reply_ref = p_reply WHERE id = d.id;
  IF NOT EXISTS (SELECT 1 FROM app.between_game_decision WHERE match_id = m.id AND after_game_id = d.after_game_id AND state = 'pending') THEN
    SELECT game_index INTO STRICT prior_index FROM app.game WHERE id = d.after_game_id;
    INSERT INTO app.game(match_id, game_index, state, accepted_revision) VALUES (m.id, prior_index + 1, 'reserved', 0) RETURNING id INTO game_id;
    UPDATE app.match SET state = 'ready', reserved_game_id = game_id, revision = revision + 1 WHERE id = m.id;
    INSERT INTO app.durable_handoff(match_id, game_id, source_ref, destination, kind, state) VALUES (m.id, game_id, game_id::text, 'game_host', 'reserved_game', 'pending');
  ELSE
    UPDATE app.match SET revision = revision + 1 WHERE id = m.id;
  END IF;
  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', d.id, 'revision', (m.revision + 1)::text, 'reservedGameId', COALESCE(game_id::text, ''), 'qualificationRef', p_qualification));
END $$;

-- Update constructed/commander room policy & countdown to count custom cards and
-- enforce Open Play custom/lending rules cleanly.
CREATE OR REPLACE FUNCTION app.native_room_policy_before_commander(p_terms jsonb, p_prepared jsonb, p_kind text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  format_id_ uuid := (p_terms->'room'->>'formatId')::uuid;
  legality_id_ uuid := (p_prepared->>'legalityRevisionId')::uuid;
  selected uuid[];
  current_key text;
  proof text := p_prepared->>'qualificationRef';
  statuses jsonb;
  pool_rows jsonb;
  item jsonb;
  proof_row jsonb;
  has_custom_selected boolean;
BEGIN
  SELECT f.key INTO current_key FROM app.format f WHERE f.id = format_id_ FOR SHARE;
  IF current_key IS NULL OR current_key NOT IN ('standard', 'pioneer', 'modern', 'legacy', 'vintage', 'draft', 'cube')
     OR split_part(proof, '/', 2) IS DISTINCT FROM current_key
     OR split_part(proof, '/', 1) IS DISTINCT FROM (CASE WHEN p_kind = 'ready' THEN 'native-constructed-ready-v1' ELSE 'native-constructed-ffa-candidate-v1' END) THEN
    RETURN NULL;
  END IF;

  IF p_kind = 'ready' THEN
    selected := ARRAY[(p_prepared->>'selectedRevisionId')::uuid];
  ELSE
    SELECT array_agg(DISTINCT (s->>'selectedRevisionId')::uuid) INTO selected FROM jsonb_array_elements(p_terms->'slots') s;
  END IF;
  IF selected IS NULL OR array_position(selected, NULL) IS NOT NULL THEN RETURN NULL; END IF;

  has_custom_selected := EXISTS (
    SELECT 1 FROM unnest(selected) r(revision) WHERE app.deck_revision_has_custom_cards(r.revision)
  );
  IF has_custom_selected THEN
    IF current_key IN ('draft', 'cube')
       OR COALESCE((p_terms->'room'->>'allowCustomCards')::boolean, false) IS NOT TRUE
       OR p_terms->'room'->>'mode' IS DISTINCT FROM 'open'
       OR COALESCE((p_terms->'room'->>'competitive')::boolean, true) IS TRUE
       OR EXISTS (
         SELECT 1 FROM app.deck_custom_entry ce
         JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
         WHERE ce.revision_id = ANY(selected) AND NOT cr.stage2_playable
       ) THEN
      RETURN NULL;
    END IF;
  END IF;

  IF current_key IN ('draft', 'cube') THEN
    pool_rows := '[]'::jsonb;
    FOR item IN SELECT s FROM jsonb_array_elements(p_terms->'slots') s
      WHERE p_kind = 'countdown' OR (s->>'selectedRevisionId')::uuid = ANY(selected)
    LOOP
      IF item->>'controllerKind' IS DISTINCT FROM 'human' THEN RETURN NULL; END IF;
      proof_row := app.limited_match_seat_pool_proof((p_terms->'room'->>'id')::uuid,
        (item->>'slotId')::uuid, (item->>'accountId')::uuid, format_id_,
        (p_prepared->>'releaseId')::uuid, (item->>'selectedRevisionId')::uuid);
      IF proof_row IS NULL THEN RETURN NULL; END IF;
      pool_rows := pool_rows || jsonb_build_array(proof_row);
    END LOOP;
    IF jsonb_array_length(pool_rows) = 0 THEN RETURN NULL; END IF;
  END IF;

  PERFORM id FROM app.legality_revision WHERE id = legality_id_ FOR SHARE;
  IF NOT FOUND THEN RETURN NULL; END IF;
  PERFORM l.id FROM app.card_legality l WHERE l.format_id = format_id_ AND l.legality_revision_id = legality_id_
    AND l.definition_id IN (SELECT e.definition_id FROM app.deck_entry e WHERE e.revision_id = ANY(selected))
    ORDER BY l.id FOR SHARE;
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'revisionId', q.revision_id, 'definitionId', q.definition_id, 'quantity', q.copies::text, 'status', l.status
  ) ORDER BY q.revision_id, q.definition_id), '[]'::jsonb)
  INTO statuses
  FROM (SELECT e.revision_id, e.definition_id, sum(e.quantity) copies FROM app.deck_entry e
        WHERE e.revision_id = ANY(selected) GROUP BY e.revision_id, e.definition_id) q
  LEFT JOIN app.card_legality l ON l.definition_id = q.definition_id AND l.format_id = format_id_ AND l.legality_revision_id = legality_id_;

  IF (jsonb_array_length(statuses) = 0 AND NOT has_custom_selected)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(statuses) s WHERE COALESCE(s->>'status', '') NOT IN ('legal', 'restricted')
       OR (s->>'status' = 'restricted' AND (s->>'quantity')::numeric > 1)) THEN
    RETURN NULL;
  END IF;

  RETURN jsonb_build_object('formatId', format_id_, 'formatKey', current_key, 'legalityRevisionId', legality_id_, 'cards', statuses)
    || CASE WHEN pool_rows IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('seatPools', pool_rows) END;
END $$;

CREATE OR REPLACE FUNCTION app.native_constructed_room_policy(p_terms jsonb, p_prepared jsonb, p_kind text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  format_id_ uuid := (p_terms->'room'->>'formatId')::uuid;
  legality_id_ uuid := (p_prepared->>'legalityRevisionId')::uuid;
  selected uuid[];
  current_key text;
  proof text := p_prepared->>'qualificationRef';
  statuses jsonb;
  has_custom_selected boolean;
BEGIN
  IF EXISTS (
    SELECT 1 FROM app.room lobby JOIN app.private_pilot_profile private_format
      ON private_format.format_id = lobby.format_id AND private_format.hide_format
    WHERE lobby.id = (p_terms->'room'->>'id')::uuid AND NOT EXISTS (
      SELECT 1 FROM app.private_pilot_profile profile
      WHERE profile.profile_id = lobby.profile_id AND profile.format_id = lobby.format_id
    )
  ) THEN RETURN NULL; END IF;
  IF EXISTS (
    SELECT 1 FROM app.room lobby JOIN app.private_pilot_profile profile ON profile.profile_id = lobby.profile_id
    WHERE lobby.id = (p_terms->'room'->>'id')::uuid AND (
      profile.format_id <> lobby.format_id OR profile.release_id IS DISTINCT FROM (p_prepared->>'releaseId')::uuid
      OR lobby.visibility <> 'private' OR lobby.mode <> 'open' OR lobby.structure <> 'bo1' OR lobby.competitive
      OR EXISTS (
        SELECT 1 FROM (
          SELECT slot->>'controllerKind' AS controller, (slot->>'accountId')::uuid AS account
          FROM jsonb_array_elements(p_terms->'slots') slot ORDER BY slot->>'accountId'
        ) member
        WHERE member.controller IS DISTINCT FROM 'human'
          OR (member.account IS NOT NULL AND NOT app.lock_private_pilot_grant(profile.release_id, member.account, profile.commander))
      )
    )
  ) THEN RETURN NULL; END IF;

  SELECT f.key INTO current_key FROM app.format f WHERE f.id = format_id_ FOR SHARE;
  IF current_key IS DISTINCT FROM 'commander' THEN
    RETURN app.native_room_policy_before_commander(p_terms, p_prepared, p_kind);
  END IF;
  IF split_part(proof, '/', 2) IS DISTINCT FROM current_key
     OR split_part(proof, '/', 1) IS DISTINCT FROM (CASE WHEN p_kind = 'ready' THEN 'native-constructed-ready-v1' ELSE 'native-constructed-ffa-candidate-v1' END) THEN
    RETURN NULL;
  END IF;

  IF p_kind = 'ready' THEN
    selected := ARRAY[(p_prepared->>'selectedRevisionId')::uuid];
  ELSE
    SELECT array_agg(DISTINCT (s->>'selectedRevisionId')::uuid) INTO selected FROM jsonb_array_elements(p_terms->'slots') s;
  END IF;
  IF selected IS NULL OR array_position(selected, NULL) IS NOT NULL THEN RETURN NULL; END IF;

  has_custom_selected := EXISTS (
    SELECT 1 FROM unnest(selected) r(revision) WHERE app.deck_revision_has_custom_cards(r.revision)
  );
  IF has_custom_selected AND (
    COALESCE((p_terms->'room'->>'allowCustomCards')::boolean, false) IS NOT TRUE
    OR EXISTS (
      SELECT 1 FROM app.deck_custom_entry ce
      JOIN app.custom_card_revision cr ON cr.id = ce.custom_card_revision_id
      WHERE ce.revision_id = ANY(selected) AND NOT cr.stage2_playable
    )
  ) THEN
    RETURN NULL;
  END IF;

  IF current_key = 'commander' THEN
    IF split_part(proof, '/', 3) IS DISTINCT FROM ('sha256:' || encode(sha256(convert_to(p_prepared->>'sourceHash', 'UTF8')), 'hex')) THEN
      RETURN NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM app.room room WHERE room.id = (p_terms->'room'->>'id')::uuid
        AND room.visibility = 'private' AND room.mode = 'open' AND room.structure = 'bo1' AND NOT room.competitive)
       OR jsonb_array_length(p_terms->'slots') > 3
       OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_terms->'slots') s
         WHERE NOT EXISTS (SELECT 1 FROM app.commander_pilot_member member
           WHERE member.release_id = (p_prepared->>'releaseId')::uuid AND member.account_id = (s->>'accountId')::uuid))
       OR EXISTS (SELECT 1 FROM jsonb_array_elements(p_terms->'slots') s
         WHERE s->>'controllerKind' IS DISTINCT FROM 'human' OR s->>'teamRef' IS NOT NULL)
       OR EXISTS (
         SELECT 1 FROM unnest(selected) r(revision)
         CROSS JOIN LATERAL (
           SELECT
             COALESCE((SELECT sum(e.quantity) FROM app.deck_entry e WHERE e.revision_id = r.revision), 0)
               + COALESCE((SELECT sum(ce.quantity) FROM app.deck_custom_entry ce WHERE ce.revision_id = r.revision), 0) AS cards,
             (SELECT count(*) FROM app.deck_entry e WHERE e.revision_id = r.revision AND e.section = 'commander' AND e.quantity = 1)
               + (SELECT count(*) FROM app.deck_custom_entry ce WHERE ce.revision_id = r.revision AND ce.section = 'commander' AND ce.quantity = 1) AS commanders,
             (SELECT count(*) FROM app.deck_entry e WHERE e.revision_id = r.revision AND e.section <> 'main' AND (e.section <> 'commander' OR e.quantity <> 1))
               + (SELECT count(*) FROM app.deck_custom_entry ce WHERE ce.revision_id = r.revision AND ce.section <> 'main' AND (ce.section <> 'commander' OR ce.quantity <> 1)) AS invalid
         ) sizes
         WHERE sizes.cards <> 100 OR sizes.commanders NOT BETWEEN 1 AND 2 OR sizes.invalid <> 0
       ) THEN
      RETURN NULL;
    END IF;
  END IF;

  PERFORM id FROM app.legality_revision WHERE id = legality_id_ AND (current_key <> 'commander' OR effective_at <= statement_timestamp()) FOR SHARE;
  IF NOT FOUND THEN RETURN NULL; END IF;
  PERFORM l.id FROM app.card_legality l WHERE l.format_id = format_id_ AND l.legality_revision_id = legality_id_
    AND l.definition_id IN (SELECT e.definition_id FROM app.deck_entry e WHERE e.revision_id = ANY(selected))
    ORDER BY l.id FOR SHARE;
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'revisionId', q.revision_id, 'definitionId', q.definition_id, 'quantity', q.copies::text, 'status', l.status
  ) ORDER BY q.revision_id, q.definition_id), '[]'::jsonb)
  INTO statuses
  FROM (SELECT e.revision_id, e.definition_id, sum(e.quantity) copies FROM app.deck_entry e
        WHERE e.revision_id = ANY(selected) GROUP BY e.revision_id, e.definition_id) q
  LEFT JOIN app.card_legality l ON l.definition_id = q.definition_id AND l.format_id = format_id_ AND l.legality_revision_id = legality_id_;
  IF (jsonb_array_length(statuses) = 0 AND NOT has_custom_selected)
     OR EXISTS (SELECT 1 FROM jsonb_array_elements(statuses) s WHERE COALESCE(s->>'status', '') NOT IN ('legal', 'restricted')
       OR (s->>'status' = 'restricted' AND (current_key = 'commander' OR (s->>'quantity')::numeric > 1))) THEN
    RETURN NULL;
  END IF;
  RETURN jsonb_build_object('formatId', format_id_, 'formatKey', current_key, 'legalityRevisionId', legality_id_, 'cards', statuses);
END $$;

CREATE OR REPLACE FUNCTION app.start_validated_countdown(p_room uuid, p_expected numeric, p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  original app.command_receipt;
  prepared jsonb;
  slots jsonb;
  inputs jsonb;
BEGIN
  IF p_room IS NULL OR p_room = '00000000-0000-0000-0000-000000000000'::uuid OR
     p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid OR
     p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid validated countdown request';
  END IF;
  SELECT * INTO original FROM app.command_receipt r
   WHERE ((r.scope = 'qualified_countdown_intent_v1' AND r.normalized_source->'sourceTerms'->'room'->>'state' = 'lobby'
           AND r.normalized_source ? 'constructedPolicy') OR
          (r.scope = 'qualified_ready_intent_v1' AND r.normalized_source ? 'roomStartPolicy'))
     AND r.target_ref = p_room::text AND r.state = 'accepted'
     AND r.response_payload->'data'->>'revision' = p_expected::text
     AND r.response_payload->'data'->>'resourceId' = p_room::text
     AND r.normalized_source->'sourceTerms'->'room'->>'id' = p_room::text
     AND r.normalized_source->'sourceTerms'->'room'->>'revision' = (p_expected - 1)::text;
  IF NOT FOUND THEN
    IF EXISTS (SELECT 1 FROM app.command_receipt r WHERE r.scope = 'room_start_v1'
       AND r.principal_key = 'room-start:' || p_room::text AND r.request_ref = p_request::text) THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'room start request terms conflict';
    END IF;
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'qualified countdown source unavailable';
  END IF;
  prepared := COALESCE(original.normalized_source->'roomStartPrepared', original.normalized_source->'prepared');
  slots := original.normalized_source->'sourceTerms'->'slots';
  SELECT jsonb_agg(jsonb_build_object(
    'slotId', s->'slotId',
    'revisionId', s->'selectedRevisionId',
    'snapshotRef', 'native-constructed-deck-v1/' || original.id::text || '/' || (s->>'slotId') || '/sha256:' ||
      app.auth_json_hash(
        jsonb_build_object('release', prepared->'releaseId', 'revision', s->'selectedRevisionId', 'entries', s->'entries')
        || CASE WHEN s ? 'customEntries' AND jsonb_typeof(s->'customEntries') = 'array' AND jsonb_array_length(s->'customEntries') > 0
                THEN jsonb_build_object('customEntries', s->'customEntries')
                ELSE '{}'::jsonb END
      )
  ) ORDER BY (s->>'seatIndex')::numeric)
  INTO inputs
  FROM jsonb_array_elements(slots) s;

  RETURN app.start_room_match(p_room, p_expected, (prepared->>'releaseId')::uuid,
    (prepared->>'legalityRevisionId')::uuid, prepared->>'qualificationRef', inputs, p_request);
END $$;

-- ---------------------------------------------------------------------------
-- 7. GDPR Account Minimization v3 (accepting both historical v2 and new v3)
-- ---------------------------------------------------------------------------

CREATE FUNCTION app.account_minimization_actions_v3() RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path = '' AS $$
  SELECT '["profile","recipient_notifications","friendships","deck_labels_and_appearance","cosmetic_access","checkout_urls","patreon_entitlements","controller_authority","private_username_reservation","resource_cleanup_intents","custom_cards"]'::jsonb
$$;

CREATE OR REPLACE FUNCTION app.valid_account_minimization(
  p_account uuid, p_retirement uuid, p_source jsonb
) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  a app.account; r app.command_receipt; expected jsonb; appearance jsonb;
  memberships uuid[]; jobs uuid[]; job app.command_receipt; grant_row app.controller_grant; terms jsonb;
  src_version text;
  expected_actions jsonb;
BEGIN
  SELECT * INTO a FROM app.account WHERE id = p_account;
  SELECT * INTO r FROM app.command_receipt WHERE id = p_retirement;
  IF a.id IS NULL OR a.state <> 'deleted' OR r.account_id IS DISTINCT FROM a.id OR r.scope IS DISTINCT FROM 'account_retirement_v1'
     OR r.operation_id IS DISTINCT FROM a.retirement_operation_id OR jsonb_typeof(p_source) IS DISTINCT FROM 'object' THEN
    RETURN false;
  END IF;
  src_version := p_source->>'version';
  IF src_version = 'account-minimization-v2' THEN
    expected_actions := app.account_minimization_actions();
  ELSIF src_version = 'account-minimization-v3' THEN
    expected_actions := app.account_minimization_actions_v3();
  ELSE
    RETURN false;
  END IF;

  appearance := p_source->'freeAppearance';
  IF jsonb_typeof(appearance) IS DISTINCT FROM 'object'
     OR (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(appearance) key) IS DISTINCT FROM ARRAY['backId', 'diceId', 'sleeveId']::text[]
     OR EXISTS (SELECT 1 FROM jsonb_each(appearance) value WHERE jsonb_typeof(value.value) NOT IN ('string', 'null') OR (jsonb_typeof(value.value) = 'string' AND value.value#>>'{}' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')) THEN
    RETURN false;
  END IF;

  SELECT COALESCE(array_agg(source_event_id ORDER BY source_event_id), '{}'::uuid[]) INTO memberships FROM app.patreon_membership WHERE account_id = p_account;
  SELECT COALESCE(array_agg(id ORDER BY id), '{}'::uuid[]) INTO jobs FROM app.command_receipt WHERE principal_key = 'account-resources:' || p_account::text AND scope = 'account_resource_cleanup_v1';

  expected := jsonb_build_object(
    'version', src_version,
    'accountId', a.id,
    'intentId', app.accepted_deletion_intent(a.id),
    'retirementOperationId', a.retirement_operation_id,
    'usernameHash', app.auth_json_hash(to_jsonb(a.username)),
    'actions', expected_actions,
    'freeAppearance', appearance,
    'patreonSourceIds', to_jsonb(memberships),
    'resourceWorkIds', to_jsonb(jobs)
  );
  IF p_source IS DISTINCT FROM expected OR app.accepted_deletion_intent(a.id) IS NULL THEN RETURN false; END IF;

  IF NOT EXISTS (SELECT 1 FROM app.asset_owner WHERE account_id = a.id AND state = 'retired')
     OR EXISTS (SELECT 1 FROM app.inventory_asset inventory JOIN app.asset_owner owner ON owner.id = inventory.owner_id WHERE owner.account_id = a.id AND inventory.owned_quantity <> 0)
     OR EXISTS (SELECT 1 FROM app.asset_hold h JOIN app.asset_owner owner ON owner.id = h.owner_id WHERE owner.account_id = a.id AND h.state = 'held')
     OR EXISTS (SELECT 1 FROM app.profile WHERE account_id = a.id)
     OR EXISTS (SELECT 1 FROM app.notification WHERE recipient_id = a.id)
     OR EXISTS (SELECT 1 FROM app.friendship WHERE a.id IN (account_low_id, account_high_id))
     OR EXISTS (SELECT 1 FROM app.deck d WHERE d.account_id = a.id AND (d.name <> 'Deleted deck' OR d.back_id::text IS DISTINCT FROM appearance->>'backId' OR d.sleeve_id::text IS DISTINCT FROM appearance->>'sleeveId' OR d.dice_id::text IS DISTINCT FROM appearance->>'diceId' OR d.playmat_id IS NOT NULL))
     OR EXISTS (SELECT 1 FROM app.cosmetic_grant WHERE account_id = a.id AND state = 'active')
     OR EXISTS (SELECT 1 FROM app.purchase WHERE account_id = a.id AND checkout_url IS NOT NULL)
     OR EXISTS (SELECT 1 FROM app.patreon_membership m WHERE m.account_id = a.id AND NOT app.valid_patreon_retirement_source(m)) THEN
    RETURN false;
  END IF;

  IF src_version = 'account-minimization-v3' THEN
    IF EXISTS (SELECT 1 FROM app.deck d WHERE d.account_id = a.id AND d.face_custom_card_revision_id IS NOT NULL)
       OR EXISTS (SELECT 1 FROM app.custom_card c WHERE c.account_id = a.id AND c.archived_at IS NULL)
       OR EXISTS (
         SELECT 1 FROM app.custom_card_revision cr
         JOIN app.custom_card c ON c.id = cr.custom_card_id
         WHERE c.account_id = a.id AND (cr.flavor_texts_json <> '[]'::jsonb OR cr.collector_note IS NOT NULL)
       )
       OR EXISTS (
         SELECT 1 FROM app.custom_card_revision_art cra
         JOIN app.custom_card_revision cr ON cr.id = cra.revision_id
         JOIN app.custom_card c ON c.id = cr.custom_card_id
         WHERE c.account_id = a.id AND (cra.art_bytes IS NOT NULL OR cra.artist IS NOT NULL)
       ) THEN
      RETURN false;
    END IF;
  END IF;

  FOR grant_row IN SELECT g.* FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id = g.binding_id JOIN app.room_slot slot ON slot.id = b.room_slot_id JOIN app.room room ON room.id = slot.room_id WHERE a.id IN (slot.sponsor_id, room.host_id) LOOP
    terms := app.account_resource_terms(a.id, grant_row.id, r.id);
    IF terms IS NULL OR NOT EXISTS (SELECT 1 FROM app.command_receipt work WHERE work.id = ANY(jobs) AND work.principal_key = 'account-resources:' || a.id::text AND work.scope = 'account_resource_cleanup_v1' AND work.request_ref = grant_row.id::text AND work.target_ref = grant_row.id::text AND work.normalized_source = terms AND work.payload_hash = app.auth_json_hash(terms) AND work.state = 'accepted' AND work.account_id IS NULL AND work.operation_id IS NULL AND work.controller_grant_id IS NULL AND work.response_payload = jsonb_build_object('status', 'success', 'data', jsonb_build_object('resourceId', grant_row.id, 'kind', 'controller_secret', 'disposition', 'cleanup pending'))) THEN
      RETURN false;
    END IF;
  END LOOP;
  FOR job IN SELECT * FROM app.command_receipt WHERE id = ANY(jobs) LOOP
    IF job.state <> 'accepted' OR NOT EXISTS (SELECT 1 FROM app.controller_grant g WHERE g.id::text = job.target_ref AND job.normalized_source = app.account_resource_terms(a.id, g.id, r.id)) THEN
      RETURN false;
    END IF;
  END LOOP;
  IF EXISTS (SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id = b.room_slot_id JOIN app.room room ON room.id = slot.room_id WHERE a.id IN (slot.sponsor_id, room.host_id) AND (b.bootstrap_receipt_id IS NULL OR b.desired_state <> 'stopped' OR NOT app.ai_stop_confirmed(b.id, b.stop_receipt_id) OR EXISTS (SELECT 1 FROM app.read_ai_stop_work(b.id)))) THEN
    RETURN false;
  END IF;
  RETURN true;
END $$;

CREATE OR REPLACE FUNCTION app.retire_account(p_account uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  subject uuid; player app.account; owner app.asset_owner; receipt app.command_receipt;
  retirement app.command_receipt; operation uuid; auth_retirement uuid; item record;
  free_back uuid; free_sleeve uuid; free_dice uuid; requested boolean; terms jsonb; minimization jsonb;
  resources uuid[]; patreon_sources uuid[];
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'account retirement requires read committed retry';
  END IF;
  IF p_account IS NULL OR p_account = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'account retirement identity required';
  END IF;
  SELECT auth_subject INTO subject FROM app.account WHERE id = p_account;
  IF subject IS NOT NULL THEN
    PERFORM id FROM auth.users WHERE id = subject FOR KEY SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'retirement Auth parent unavailable'; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('signup:' || subject::text, 0));
  END IF;
  SELECT * INTO player FROM app.account WHERE id = p_account FOR UPDATE;
  IF NOT FOUND THEN RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable')); END IF;
  IF player.state = 'deleted' THEN
    retirement := app.read_account_retirement_proof(player.id, player.retirement_operation_id);
    IF retirement.id IS NULL THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'canonical retirement proof unavailable'; END IF;
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', player.id, 'state', 'retired', 'retirementOperationId', player.retirement_operation_id));
  END IF;
  IF player.state <> 'deletion_pending' THEN RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable')); END IF;
  IF subject IS NULL OR player.auth_subject IS DISTINCT FROM subject THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'retirement subject changed; retry';
  END IF;
  PERFORM pg_advisory_xact_lock(hashtextextended('auth_authority_v1:' || app.auth_uuid_hash(subject), 0));
  SELECT EXISTS (SELECT 1 FROM app.command_receipt r WHERE r.account_id = p_account AND r.principal_key = 'human:' || p_account::text
    AND r.scope = 'account_delete' AND r.target_ref = p_account::text AND r.state = 'accepted'
    AND substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"') IS NOT NULL
    AND r.result_ref = jsonb_build_object('version', 1, 'statusKeyHash', substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"'))::text
    AND r.payload_hash = app.auth_json_hash(jsonb_build_object('statusKeyHash', substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"')))
    AND r.response_payload = jsonb_build_object('status', 'success', 'data', jsonb_build_object('resourceId', p_account))) INTO requested;
  SELECT * INTO owner FROM app.asset_owner WHERE account_id = p_account FOR UPDATE;
  IF NOT FOUND OR owner.state <> 'active' THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'retirement owner unavailable'; END IF;
  IF requested THEN PERFORM app.cleanup_pre_game_lobbies(player.id, 100); END IF;
  IF NOT requested OR app.account_retirement_blocked(player.id, owner.id) THEN
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', player.id, 'state', 'waiting'));
  END IF;
  IF EXISTS (SELECT 1 FROM app.deck WHERE account_id = p_account) THEN
    free_back := app.free_cosmetic(p_account, 'back');
    free_sleeve := app.free_cosmetic(p_account, 'sleeve');
    free_dice := app.free_cosmetic(p_account, 'dice');
    IF free_back IS NULL OR free_sleeve IS NULL OR free_dice IS NULL THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'retirement free appearance unavailable';
    END IF;
  END IF;
  PERFORM id FROM app.inventory_asset WHERE owner_id = owner.id ORDER BY id FOR NO KEY UPDATE;
  terms := jsonb_build_object('version', 1, 'scope', 'account_retirement_v1', 'accountId', p_account);
  receipt := app.begin_account_command(p_account, p_account, 'account_retirement_v1', p_account::text, terms);
  IF receipt.state <> 'pending' THEN RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'retirement receipt conflicts with lifecycle'; END IF;
  INSERT INTO app.ledger_operation(kind, source_ref, source_scope, actor_account_id, created_at, state)
  VALUES ('retirement', p_account::text, 'account_retirement', p_account, clock_timestamp(), 'pending') RETURNING id INTO operation;
  FOR item IN SELECT id, owned_quantity FROM app.inventory_asset WHERE owner_id = owner.id AND owned_quantity > 0 ORDER BY id LOOP
    INSERT INTO app.ledger_posting(operation_id, inventory_asset_id, direction, quantity, source_kind, source_ref)
    VALUES (operation, item.id, 'debit', item.owned_quantity, 'retirement', p_account::text);
  END LOOP;
  IF EXISTS (SELECT 1 FROM app.inventory_asset WHERE owner_id = owner.id AND owned_quantity <> 0)
     OR EXISTS (SELECT 1 FROM app.asset_hold WHERE owner_id = owner.id AND state = 'held') THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'retirement assets remain';
  END IF;
  UPDATE app.ledger_operation SET state = 'committed' WHERE id = operation;
  UPDATE app.command_receipt SET operation_id = operation WHERE id = receipt.id;
  UPDATE app.account SET state = 'deleted', retirement_operation_id = operation, deletion_finalized_at = clock_timestamp() WHERE id = p_account;
  UPDATE app.asset_owner SET state = 'retired' WHERE id = owner.id;
  UPDATE app.deck
  SET name = 'Deleted deck',
      back_id = free_back,
      sleeve_id = free_sleeve,
      dice_id = free_dice,
      playmat_id = NULL,
      face_custom_card_revision_id = NULL
  WHERE account_id = p_account;

  -- Scrub user-authored custom card PII/art while preserving rules_sha256 & replay integrity.
  PERFORM set_config('spellwake.retiring_account', 'on', true);
  UPDATE app.custom_card
  SET archived_at = COALESCE(archived_at, clock_timestamp()), updated_at = clock_timestamp()
  WHERE account_id = p_account;
  UPDATE app.custom_card_revision cr
  SET flavor_texts_json = '[]'::jsonb, collector_note = NULL
  FROM app.custom_card c
  WHERE cr.custom_card_id = c.id AND c.account_id = p_account;
  UPDATE app.custom_card_revision_art cra
  SET art_bytes = NULL, artist = NULL
  FROM app.custom_card_revision cr
  JOIN app.custom_card c ON c.id = cr.custom_card_id
  WHERE cra.revision_id = cr.id AND c.account_id = p_account;

  DELETE FROM app.profile WHERE account_id = p_account;
  DELETE FROM app.notification WHERE recipient_id = p_account;
  DELETE FROM app.friendship WHERE p_account IN (account_low_id, account_high_id);
  UPDATE app.cosmetic_grant SET state = 'revoked' WHERE account_id = p_account AND state = 'active';
  UPDATE app.purchase SET checkout_url = NULL WHERE account_id = p_account AND checkout_url IS NOT NULL;
  patreon_sources := app.retire_patreon_memberships(p_account, receipt.id);
  resources := app.record_account_resource_work(p_account, receipt.id);

  minimization := jsonb_build_object(
    'version', 'account-minimization-v3',
    'accountId', p_account,
    'intentId', app.accepted_deletion_intent(p_account),
    'retirementOperationId', operation,
    'usernameHash', app.auth_json_hash(to_jsonb(player.username)),
    'actions', app.account_minimization_actions_v3(),
    'freeAppearance', jsonb_build_object('backId', free_back, 'sleeveId', free_sleeve, 'diceId', free_dice),
    'patreonSourceIds', to_jsonb(patreon_sources),
    'resourceWorkIds', to_jsonb(resources)
  );
  UPDATE app.command_receipt SET normalized_source = minimization WHERE id = receipt.id;
  auth_retirement := app.record_registered_auth_retirement(subject, p_account, operation);
  UPDATE app.account SET auth_cleanup_subject = subject, auth_subject = NULL WHERE id = p_account;
  UPDATE app.command_receipt
  SET operation_id = operation,
      result_ref = jsonb_build_object(
        'version', 3,
        'policy', 'account_minimization_v3',
        'minimizationHash', app.auth_json_hash(minimization),
        'retirementOperationId', operation,
        'authRetirementId', auth_retirement
      )::text
  WHERE id = receipt.id;
  PERFORM app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_account));
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', p_account, 'state', 'retired', 'retirementOperationId', operation));
END $$;

CREATE OR REPLACE FUNCTION app.read_account_retirement_proof(p_account uuid, p_operation uuid) RETURNS app.command_receipt
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE finalization app.command_receipt; retirement app.command_receipt; terms jsonb; expected_v2 jsonb; expected_v3 jsonb;
BEGIN
  PERFORM app.check_registered_auth_retirement(p_account, p_operation);
  SELECT * INTO finalization FROM app.command_receipt
  WHERE principal_key = 'human:' || p_account::text
    AND scope = 'account_retirement_v1'
    AND request_ref = p_account::text;
  terms := jsonb_build_object('version', 1, 'scope', 'account_retirement_v1', 'accountId', p_account);
  IF finalization.id IS NULL OR finalization.state <> 'accepted' OR finalization.account_id IS DISTINCT FROM p_account
     OR finalization.target_ref IS DISTINCT FROM p_account::text OR finalization.operation_id IS DISTINCT FROM p_operation
     OR finalization.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)
     OR finalization.response_payload IS DISTINCT FROM jsonb_build_object('status', 'success', 'data', jsonb_build_object('resourceId', p_account)) THEN
    RETURN NULL;
  END IF;
  SELECT * INTO retirement FROM app.command_receipt WHERE id = (finalization.result_ref::jsonb->>'authRetirementId')::uuid;
  IF retirement.id IS NULL OR retirement.scope <> 'auth_retirement_v1' OR retirement.state <> 'accepted'
     OR retirement.target_ref IS DISTINCT FROM p_account::text THEN
    RETURN NULL;
  END IF;
  PERFORM app.check_auth_receipt_shape(retirement);
  IF retirement.result_ref::jsonb IS DISTINCT FROM jsonb_build_object(
    'version', 1,
    'proofHash', app.auth_json_hash(jsonb_build_object('version', 1, 'source', 'application_retirement', 'accountId', p_account::text, 'retirementOperationId', p_operation::text))
  ) THEN
    RETURN NULL;
  END IF;

  expected_v2 := jsonb_build_object(
    'version', 2, 'policy', 'account_minimization_v2',
    'minimizationHash', app.auth_json_hash(finalization.normalized_source),
    'retirementOperationId', p_operation, 'authRetirementId', retirement.id
  );
  expected_v3 := jsonb_build_object(
    'version', 3, 'policy', 'account_minimization_v3',
    'minimizationHash', app.auth_json_hash(finalization.normalized_source),
    'retirementOperationId', p_operation, 'authRetirementId', retirement.id
  );
  IF finalization.result_ref::jsonb IS DISTINCT FROM expected_v2
     AND finalization.result_ref::jsonb IS DISTINCT FROM expected_v3 THEN
    RETURN NULL;
  END IF;
  IF NOT app.valid_account_minimization(p_account, finalization.id, finalization.normalized_source) THEN
    RETURN NULL;
  END IF;
  RETURN retirement;
END $$;

CREATE FUNCTION app.create_open_play_quick_test_room_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_custom_card_revision_id uuid,
  p_ai_plays_custom_card boolean DEFAULT false
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
  rev app.custom_card_revision;
  defaults app.profile;
  fmt_id uuid;
  profile_id uuid;
  basic_def_id uuid;
  deck_id uuid;
  deck_rev_id uuid;
  room_id uuid;
  ai_slot_id uuid;
  deck_name text;
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_custom_card_revision_id IS NULL OR p_custom_card_revision_id = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;

  receipt := app.begin_account_command(
    player.id,
    p_request,
    'open_play_quick_test_v1',
    p_custom_card_revision_id::text,
    jsonb_build_object(
      'customCardRevisionId', p_custom_card_revision_id,
      'aiPlaysCustomCard', COALESCE(p_ai_plays_custom_card, false)
    )
  );
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable'));
  END IF;
  IF app.active_supporter_tier(player.id) IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('supporter', 'supporter_required'));
  END IF;

  SELECT r.* INTO rev
  FROM app.custom_card_revision r
  JOIN app.custom_card c ON c.id = r.custom_card_id
  WHERE r.id = p_custom_card_revision_id AND c.account_id = player.id;
  IF rev.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'unavailable'));
  END IF;
  IF NOT rev.stage2_playable THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('card', 'not_playable'));
  END IF;

  SELECT f.id INTO fmt_id
  FROM app.format f
  WHERE f.key IN ('legacy', 'modern', 'pioneer', 'standard', 'vintage')
  ORDER BY array_position(ARRAY['legacy', 'modern', 'pioneer', 'standard', 'vintage'], f.key)
  LIMIT 1;
  SELECT p.id INTO profile_id
  FROM app.table_profile_revision p
  WHERE p.admission_qualification_ref IS NOT NULL
    AND p.planar_pool_ref IS NULL
    AND p.minimum_seats <= 2
    AND p.seat_capacity >= 2
  ORDER BY p.seat_capacity ASC, p.id ASC
  LIMIT 1;
  IF fmt_id IS NULL OR profile_id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('format', 'unavailable'));
  END IF;

  SELECT * INTO STRICT defaults FROM app.profile WHERE account_id = player.id FOR SHARE;
  deck_name := left('Quick Test: ' || rev.name, 120);

  INSERT INTO app.deck(
    account_id, source_kind, mode, format_id, name,
    back_id, sleeve_id, dice_id, playmat_id, face_custom_card_revision_id
  ) VALUES (
    player.id, 'account', 'open', fmt_id, deck_name,
    defaults.default_back_id, defaults.default_sleeve_id, defaults.default_dice_id, defaults.default_playmat_id,
    rev.id
  ) RETURNING id INTO deck_id;

  INSERT INTO app.deck_revision(deck_id, revision_number, created_at)
  VALUES (deck_id, 1, statement_timestamp())
  RETURNING id INTO deck_rev_id;

  INSERT INTO app.deck_custom_entry(revision_id, custom_card_revision_id, section, quantity)
  VALUES (deck_rev_id, rev.id, 'main', 4);

  -- Pad to a legal 60-card Open Play deck using an unlimited basic land definition if present.
  SELECT cd.id INTO basic_def_id
  FROM app.card_definition cd
  WHERE cd.name IN ('Island', 'Mountain', 'Plains', 'Swamp', 'Forest')
  ORDER BY array_position(ARRAY['Island', 'Mountain', 'Plains', 'Swamp', 'Forest'], cd.name)
  LIMIT 1;
  IF basic_def_id IS NOT NULL THEN
    INSERT INTO app.deck_entry(revision_id, definition_id, printing_id, section, quantity)
    VALUES (deck_rev_id, basic_def_id, NULL, 'main', 56);
  END IF;

  UPDATE app.deck_revision SET sealed_at = statement_timestamp() WHERE id = deck_rev_id;
  UPDATE app.deck SET current_revision_id = deck_rev_id WHERE id = deck_id;

  INSERT INTO app.room(
    host_id, format_id, profile_id, mode, competitive, visibility, state, revision,
    structure, allow_custom_cards, share_host_decks
  ) VALUES (
    player.id, fmt_id, profile_id, 'open', false, 'private', 'lobby', 0,
    'bo1', true, true
  ) RETURNING id INTO room_id;

  INSERT INTO app.room_slot(room_id, account_id, seat_index, controller_kind, deck_revision_id, ready)
  VALUES (room_id, player.id, 0, 'human', deck_rev_id, false);

  INSERT INTO app.room_slot(room_id, sponsor_id, seat_index, controller_kind, ai_tier, deck_revision_id, ready)
  VALUES (
    room_id,
    player.id,
    1,
    'ai',
    'normal',
    CASE WHEN COALESCE(p_ai_plays_custom_card, false) THEN deck_rev_id ELSE NULL END,
    false
  ) RETURNING id INTO ai_slot_id;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object(
      'roomId', room_id,
      'deckId', deck_id,
      'deckRevisionId', deck_rev_id,
      'aiSlotId', ai_slot_id
    )
  );
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

-- ---------------------------------------------------------------------------
-- 8. Least-Privilege Function Grants
-- ---------------------------------------------------------------------------

REVOKE ALL ON FUNCTION
  app.deck_revision_has_custom_cards(uuid),
  app.deck_revision_card_totals(uuid),
  app.room_seat_can_use_deck(uuid, uuid, uuid, text),
  app.custom_card_revision_json(app.custom_card_revision),
  app.create_custom_card_command(uuid, uuid, uuid, jsonb),
  app.save_custom_card_revision_command(uuid, uuid, uuid, uuid, uuid, jsonb),
  app.archive_custom_card_command(uuid, uuid, uuid, uuid, boolean),
  app.delete_custom_card_command(uuid, uuid, uuid, uuid),
  app.read_my_custom_cards(uuid, integer, boolean, text),
  app.read_custom_card_art_for_owner(uuid, uuid, smallint),
  app.read_lent_deck_face_art(uuid, uuid, uuid, uuid),
  app.record_revealed_custom_cards(uuid, uuid, smallint, uuid[]),
  app.read_revealed_match_custom_card(uuid, uuid, uuid, uuid),
  app.read_revealed_match_custom_card_art(uuid, uuid, uuid, uuid, smallint),
  app.read_host_shared_decks(uuid),
  app.set_ai_slot_deck(uuid, uuid, numeric, uuid, uuid),
  app.set_ai_slot_deck_command(uuid, uuid, uuid, uuid, numeric, uuid),
  app.read_set_ai_slot_deck_command(uuid, uuid, uuid, uuid, numeric, uuid),
  app.create_open_play_quick_test_room_command(uuid, uuid, uuid, uuid, boolean),
  app.native_custom_deck_source(uuid),
  app.account_minimization_actions_v3()
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

GRANT EXECUTE ON FUNCTION
  app.read_my_custom_cards(uuid, integer, boolean, text),
  app.read_host_shared_decks(uuid)
TO authenticated;

GRANT EXECUTE ON FUNCTION
  app.create_custom_card_command(uuid, uuid, uuid, jsonb),
  app.save_custom_card_revision_command(uuid, uuid, uuid, uuid, uuid, jsonb),
  app.archive_custom_card_command(uuid, uuid, uuid, uuid, boolean),
  app.delete_custom_card_command(uuid, uuid, uuid, uuid),
  app.read_custom_card_art_for_owner(uuid, uuid, smallint),
  app.read_lent_deck_face_art(uuid, uuid, uuid, uuid),
  app.record_revealed_custom_cards(uuid, uuid, smallint, uuid[]),
  app.read_revealed_match_custom_card(uuid, uuid, uuid, uuid),
  app.read_revealed_match_custom_card_art(uuid, uuid, uuid, uuid, smallint),
  app.set_ai_slot_deck_command(uuid, uuid, uuid, uuid, numeric, uuid),
  app.read_set_ai_slot_deck_command(uuid, uuid, uuid, uuid, numeric, uuid),
  app.create_open_play_quick_test_room_command(uuid, uuid, uuid, uuid, boolean)
TO service_role;

COMMIT;
