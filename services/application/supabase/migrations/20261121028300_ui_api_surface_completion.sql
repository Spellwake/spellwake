-- UI API surface completion: rich card metadata, deck delete/duplicate, cosmetic pricing & pets,
-- profile badge & cross-device Patreon status, League leaderboard & public player ranks,
-- searchable market & predictions (with settled result price), bilateral live trade rooms,
-- enriched lobby slots & options, DM invite cards, and pg_cron retention hygiene.
BEGIN;

-- ---------------------------------------------------------------------------
-- 1. Additive schema extensions (non-breaking, nullable / defaulted)
-- ---------------------------------------------------------------------------
ALTER TABLE app.cosmetic
  ADD COLUMN IF NOT EXISTS name text,
  ADD COLUMN IF NOT EXISTS description text,
  ADD COLUMN IF NOT EXISTS price_cents integer CHECK (price_cents IS NULL OR price_cents >= 0),
  ADD COLUMN IF NOT EXISTS price_coins numeric(78,0) CHECK (price_coins IS NULL OR (price_coins >= 0 AND price_coins <> 'NaN'::numeric)),
  ADD COLUMN IF NOT EXISTS preview_url text,
  ADD COLUMN IF NOT EXISTS rarity text;

ALTER TABLE app.cosmetic
  DROP CONSTRAINT IF EXISTS cosmetic_kind,
  ADD CONSTRAINT cosmetic_kind CHECK (kind = ANY (ARRAY['back'::text, 'sleeve'::text, 'dice'::text, 'playmat'::text, 'pet'::text]));

ALTER TABLE app.profile
  ADD COLUMN IF NOT EXISTS default_pet_id uuid REFERENCES app.cosmetic(id),
  ADD COLUMN IF NOT EXISTS selected_badge text CHECK (selected_badge IS NULL OR (length(btrim(selected_badge)) BETWEEN 1 AND 64));

ALTER TABLE app.deck
  ADD COLUMN IF NOT EXISTS pet_id uuid REFERENCES app.cosmetic(id);

ALTER TABLE app.room
  ADD COLUMN IF NOT EXISTS ai_difficulty text CHECK (ai_difficulty IS NULL OR ai_difficulty IN ('casual', 'standard', 'competitive')),
  ADD COLUMN IF NOT EXISTS planechase_enabled boolean NOT NULL DEFAULT false,
  ADD COLUMN IF NOT EXISTS range_of_influence integer CHECK (range_of_influence IS NULL OR range_of_influence BETWEEN 1 AND 5);

ALTER TABLE app.message
  ADD COLUMN IF NOT EXISTS attachment jsonb CHECK (
    attachment IS NULL OR (
      jsonb_typeof(attachment) = 'object'
      AND octet_length(attachment::text) <= 4096
      AND attachment->>'kind' IN ('trade_invite', 'game_invite')
    )
  );

-- ---------------------------------------------------------------------------
-- 2. Rich card metadata & searchable card catalogue
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.card_printing_rich_json(p_printing_id uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  WITH latest_legality AS (
    SELECT id FROM app.legality_revision ORDER BY effective_at DESC, imported_at DESC LIMIT 1
  ),
  latest_rev AS (
    SELECT DISTINCT ON (r.definition_id)
      r.id, r.definition_id, r.rules_text_ref
    FROM app.card_definition_revision r
    ORDER BY r.definition_id, r.id DESC
  )
  SELECT jsonb_build_object(
    'printingId', p.id,
    'definitionId', d.id,
    'oracleRef', d.oracle_ref,
    'name', d.name,
    'externalPrintingRef', p.external_printing_ref,
    'rarity', p.rarity,
    'finish', p.finish,
    'imageUrl', NULLIF(btrim(p.image_ref), ''),
    'setId', s.id,
    'setName', s.name,
    'setCode', COALESCE(NULLIF(btrim(s.code), ''), s.external_set_ref),
    'rulesTextRef', rev.rules_text_ref,
    'faces', COALESCE((
      SELECT jsonb_agg(
        jsonb_build_object(
          'faceIndex', f.face_index,
          'name', f.name,
          'characteristicsRef', f.characteristics_ref
        ) ORDER BY f.face_index
      )
      FROM app.card_face f
      WHERE f.definition_revision_id = rev.id
    ), '[]'::jsonb),
    'legalities', COALESCE((
      SELECT jsonb_object_agg(fmt.key, cl.status)
      FROM app.card_legality cl
      JOIN app.format fmt ON fmt.id = cl.format_id
      JOIN latest_legality ll ON ll.id = cl.legality_revision_id
      WHERE cl.definition_id = d.id
    ), '{}'::jsonb)
  )
  FROM app.printing p
  JOIN app.card_definition d ON d.id = p.definition_id
  JOIN app.card_set s ON s.id = p.set_id
  LEFT JOIN latest_rev rev ON rev.definition_id = d.id
  WHERE p.id = p_printing_id
$$;

CREATE OR REPLACE FUNCTION app.read_card_metadata(p_printing_ids uuid[], p_definition_ids uuid[]) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  p_ids uuid[] := COALESCE(p_printing_ids, ARRAY[]::uuid[]);
  d_ids uuid[] := COALESCE(p_definition_ids, ARRAY[]::uuid[]);
  printings_out jsonb;
  definitions_out jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF cardinality(p_ids) > 100 OR cardinality(d_ids) > 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;

  SELECT COALESCE(jsonb_agg(app.card_printing_rich_json(p.id) ORDER BY p.id), '[]'::jsonb)
  INTO printings_out
  FROM app.printing p
  WHERE p.id = ANY(p_ids);

  SELECT COALESCE(jsonb_agg(
    app.card_printing_rich_json(first_printing.id) ORDER BY first_printing.definition_id
  ), '[]'::jsonb)
  INTO definitions_out
  FROM (
    SELECT DISTINCT ON (p.definition_id) p.id, p.definition_id
    FROM app.printing p
    WHERE p.definition_id = ANY(d_ids)
    ORDER BY p.definition_id, p.id
  ) first_printing;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'printings', printings_out,
      'definitions', definitions_out
    )
  );
END $$;

CREATE OR REPLACE FUNCTION app.search_cards_rich(
  p_query text,
  p_format uuid,
  p_rarity text,
  p_set_id uuid,
  p_after uuid,
  p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  items jsonb;
  cursor_id uuid;
  q text := NULLIF(btrim(COALESCE(p_query, '')), '');
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;

  WITH latest_legality AS (
    SELECT id FROM app.legality_revision ORDER BY effective_at DESC, imported_at DESC LIMIT 1
  ),
  matched AS (
    SELECT p.id, app.card_printing_rich_json(p.id) AS entry
    FROM app.printing p
    JOIN app.card_definition d ON d.id = p.definition_id
    WHERE (p_after IS NULL OR p.id > p_after)
      AND (q IS NULL OR d.name ILIKE '%' || q || '%')
      AND (p_rarity IS NULL OR p.rarity = p_rarity)
      AND (p_set_id IS NULL OR p.set_id = p_set_id)
      AND (p_format IS NULL OR EXISTS (
        SELECT 1 FROM app.card_legality cl
        JOIN latest_legality ll ON ll.id = cl.legality_revision_id
        WHERE cl.definition_id = d.id AND cl.format_id = p_format AND cl.status = 'legal'
      ))
    ORDER BY p.id
    LIMIT p_limit
  )
  SELECT COALESCE(jsonb_agg(entry ORDER BY id), '[]'::jsonb) INTO items FROM matched;

  IF jsonb_array_length(items) = p_limit THEN
    cursor_id := (items->-1->>'printingId')::uuid;
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object('items', items, 'nextCursor', cursor_id)
  );
END $$;

-- ---------------------------------------------------------------------------
-- 3. Deck delete & duplicate commands
-- ---------------------------------------------------------------------------
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
BEGIN
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_request = '00000000-0000-0000-0000-000000000000'::uuid
     OR p_deck IS NULL OR p_deck = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  receipt := app.begin_account_command(player.id, p_request, 'deck_delete_v1', p_deck::text, jsonb_build_object('deckId', p_deck));
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF player.state <> 'active' THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'account unavailable'));
  END IF;

  SELECT * INTO target FROM app.deck WHERE id = p_deck AND account_id = player.id FOR UPDATE;
  IF target.id IS NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck unavailable'));
  END IF;

  -- Clear room slot references in lobby rooms before deleting revisions.
  UPDATE app.room_slot rs
  SET deck_revision_id = NULL, ready = false
  FROM app.room r
  WHERE rs.room_id = r.id
    AND r.state = 'lobby'
    AND rs.account_id = player.id
    AND rs.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);

  -- Refuse deletion only if referenced by an active match seat or active queue entry.
  IF EXISTS (
    SELECT 1 FROM app.queue_entry q
    WHERE q.account_id = player.id AND q.state = 'queued'
      AND q.deck_revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck)
  ) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck in active queue'));
  END IF;

  DELETE FROM app.deck_entry WHERE revision_id IN (SELECT id FROM app.deck_revision WHERE deck_id = p_deck);
  UPDATE app.deck SET current_revision_id = NULL WHERE id = p_deck;
  DELETE FROM app.deck_revision WHERE deck_id = p_deck;
  DELETE FROM app.deck WHERE id = p_deck;

  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('resourceId', p_deck));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN foreign_key_violation THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('deck', 'deck referenced by match history'));
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
    player.id,
    p_request,
    'deck_duplicate_v1',
    p_source_deck::text,
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
    back_id, sleeve_id, dice_id, playmat_id, pet_id
  ) VALUES (
    player.id, source.source_kind, source.mode, source.format_id, clean_name,
    source.back_id, source.sleeve_id, source.dice_id, source.playmat_id, source.pet_id
  ) RETURNING id INTO new_deck_id;

  IF source.current_revision_id IS NOT NULL THEN
    INSERT INTO app.deck_revision(deck_id, revision_number, created_at)
    VALUES (new_deck_id, 1, statement_timestamp())
    RETURNING id INTO new_rev_id;

    INSERT INTO app.deck_entry(revision_id, definition_id, printing_id, section, quantity)
    SELECT new_rev_id, definition_id, printing_id, section, quantity
    FROM app.deck_entry
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
-- 4. Rich cosmetic catalogue (prices, art URLs, pet kind)
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.read_cosmetic_catalogue_rich(
  p_kind text,
  p_after uuid,
  p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  items jsonb;
  cursor_id uuid;
  prof app.profile;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
     OR (p_kind IS NOT NULL AND p_kind NOT IN ('back', 'sleeve', 'dice', 'playmat', 'pet')) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;

  SELECT * INTO prof FROM app.profile WHERE account_id = actor;

  SELECT COALESCE(jsonb_agg(row_json ORDER BY id), '[]'::jsonb) INTO items
  FROM (
    SELECT c.id, jsonb_build_object(
      'id', c.id,
      'kind', c.kind,
      'name', COALESCE(c.name, c.display_name, c.asset_ref),
      'description', c.description,
      'assetRef', c.asset_ref,
      'previewUrl', COALESCE(c.preview_url, c.asset_ref),
      'rarity', c.rarity,
      'state', c.state,
      'priceCents', c.price_cents,
      'priceCoins', c.price_coins::text,
      'owned', EXISTS (
        SELECT 1 FROM app.cosmetic_grant g
        WHERE g.account_id = actor AND g.cosmetic_id = c.id AND g.state = 'active'
      ),
      'equipped', c.id IN (prof.default_back_id, prof.default_sleeve_id, prof.default_dice_id, prof.default_playmat_id, prof.default_pet_id)
    ) AS row_json
    FROM app.cosmetic c
    WHERE c.state = 'active'
      AND (p_kind IS NULL OR c.kind = p_kind)
      AND (p_after IS NULL OR c.id > p_after)
    ORDER BY c.id
    LIMIT p_limit
  ) page;

  IF jsonb_array_length(items) = p_limit THEN
    cursor_id := (items->-1->>'id')::uuid;
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object('accountId', actor, 'items', items, 'nextCursor', cursor_id)
  );
END $$;

-- ---------------------------------------------------------------------------
-- 5. Profile badge persistence & cross-device Patreon status
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.set_my_profile_badge(p_badge text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  clean text := NULLIF(btrim(COALESCE(p_badge, '')), '');
  is_supporter boolean;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF clean IS NOT NULL AND length(clean) > 64 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('badge', 'invalid'));
  END IF;
  is_supporter := app.active_supporter_tier(actor) IS NOT NULL;
  IF clean IN ('supporter', 'patron') AND NOT is_supporter THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('badge', 'unavailable'));
  END IF;
  UPDATE app.profile SET selected_badge = clean WHERE account_id = actor;
  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object('accountId', actor, 'selectedBadge', clean)
  );
END $$;

CREATE OR REPLACE FUNCTION app.read_my_patreon_status() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  membership app.patreon_membership;
  tier app.supporter_tier;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO membership FROM app.patreon_membership WHERE account_id = actor;
  IF membership.active_tier_id IS NOT NULL THEN
    SELECT * INTO tier FROM app.supporter_tier WHERE id = membership.active_tier_id;
  END IF;
  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'linked', membership.id IS NOT NULL,
      'active', COALESCE(membership.active, false),
      'supporter', app.active_supporter_tier(actor) IS NOT NULL,
      'tier', CASE WHEN tier.id IS NOT NULL THEN jsonb_build_object(
        'id', tier.id, 'name', tier.name, 'rank', tier.rank
      ) ELSE NULL END
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 6. League leaderboard & public player profile + ranks + W-L-D
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.read_league_leaderboard(
  p_format uuid,
  p_season uuid,
  p_offset integer,
  p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  eff_season uuid := p_season;
  eff_offset integer := GREATEST(COALESCE(p_offset, 0), 0);
  items jsonb;
  total_count bigint;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR eff_offset > 10000 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  IF eff_season IS NULL THEN
    SELECT id INTO eff_season FROM app.season WHERE state = 'active' ORDER BY start_at DESC LIMIT 1;
  END IF;

  SELECT count(*) INTO total_count
  FROM app.rank r
  JOIN app.account a ON a.id = r.account_id
  WHERE a.state = 'active'
    AND (p_format IS NULL OR r.format_id = p_format)
    AND (eff_season IS NULL OR r.season_id = eff_season);

  WITH ranked AS (
    SELECT
      row_number() OVER (ORDER BY r.rating DESC, r.id ASC) AS pos,
      r.id,
      r.account_id,
      a.username,
      r.format_id,
      r.season_id,
      r.tier,
      r.rating,
      r.finalized,
      p.selected_badge,
      app.profile_art_snapshot(p) AS card_art,
      app.active_supporter_tier(a.id) IS NOT NULL AS supporter,
      (SELECT count(*) FROM app.match_seat ms
        JOIN app.match_result mr ON mr.match_id = ms.match_id
        WHERE ms.account_id = a.id AND mr.outcome = 'winner'
          AND (ms.id = ANY(COALESCE(mr.winner_seat_ids, ARRAY[]::uuid[]))
               OR COALESCE(mr.winners_ref, '') LIKE '%' || ms.id::text || '%')) AS wins,
      (SELECT count(*) FROM app.match_seat ms
        JOIN app.match_result mr ON mr.match_id = ms.match_id
        WHERE ms.account_id = a.id AND mr.outcome = 'winner'
          AND NOT (ms.id = ANY(COALESCE(mr.winner_seat_ids, ARRAY[]::uuid[]))
                   OR COALESCE(mr.winners_ref, '') LIKE '%' || ms.id::text || '%')) AS losses,
      (SELECT count(*) FROM app.match_seat ms
        JOIN app.match_result mr ON mr.match_id = ms.match_id
        WHERE ms.account_id = a.id AND mr.outcome = 'draw') AS draws
    FROM app.rank r
    JOIN app.account a ON a.id = r.account_id
    LEFT JOIN app.profile p ON p.account_id = a.id
    WHERE a.state = 'active'
      AND (p_format IS NULL OR r.format_id = p_format)
      AND (eff_season IS NULL OR r.season_id = eff_season)
    ORDER BY r.rating DESC, r.id ASC
    OFFSET eff_offset
    LIMIT p_limit
  )
  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'rankPosition', pos,
    'id', id,
    'accountId', account_id,
    'username', username,
    'formatId', format_id,
    'seasonId', season_id,
    'tier', tier,
    'rating', rating::text,
    'finalized', finalized,
    'selectedBadge', selected_badge,
    'cardArtProfile', card_art,
    'supporter', supporter,
    'wins', wins,
    'losses', losses,
    'draws', draws
  ) ORDER BY pos), '[]'::jsonb)
  INTO items
  FROM ranked;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'seasonId', eff_season,
      'formatId', p_format,
      'offset', eff_offset,
      'total', total_count,
      'items', items
    )
  );
END $$;

CREATE OR REPLACE FUNCTION app.read_player_public_summary(p_player uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  player app.account;
  prof app.profile;
  ranks_json jsonb;
  wins_count bigint;
  losses_count bigint;
  draws_count bigint;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO player FROM app.account WHERE id = p_player AND (id = actor OR state = 'active');
  IF player.id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('player', 'unavailable'));
  END IF;
  SELECT * INTO prof FROM app.profile WHERE account_id = player.id;

  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'id', r.id, 'formatId', r.format_id, 'seasonId', r.season_id,
    'tier', r.tier, 'rating', r.rating::text, 'finalized', r.finalized
  ) ORDER BY r.rating DESC), '[]'::jsonb) INTO ranks_json
  FROM app.rank r WHERE r.account_id = player.id;

  SELECT
    count(*) FILTER (WHERE mr.outcome = 'winner' AND (ms.id = ANY(COALESCE(mr.winner_seat_ids, ARRAY[]::uuid[])) OR COALESCE(mr.winners_ref, '') LIKE '%' || ms.id::text || '%')),
    count(*) FILTER (WHERE mr.outcome = 'winner' AND NOT (ms.id = ANY(COALESCE(mr.winner_seat_ids, ARRAY[]::uuid[])) OR COALESCE(mr.winners_ref, '') LIKE '%' || ms.id::text || '%')),
    count(*) FILTER (WHERE mr.outcome = 'draw')
  INTO wins_count, losses_count, draws_count
  FROM app.match_seat ms
  JOIN app.match_result mr ON mr.match_id = ms.match_id
  WHERE ms.account_id = player.id;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'viewerAccountId', actor,
      'person', app.account_identity_json(player),
      'cardArtProfile', app.profile_art_snapshot(prof),
      'selectedBadge', prof.selected_badge,
      'supporter', app.active_supporter_tier(player.id) IS NOT NULL,
      'ranks', ranks_json,
      'record', jsonb_build_object('wins', wins_count, 'losses', losses_count, 'draws', draws_count)
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 7. Searchable & sortable market listings (up to 100/page)
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.search_market_listings(
  p_search text,
  p_kind text,
  p_rarity text,
  p_min_price numeric,
  p_max_price numeric,
  p_owned boolean,
  p_sort text,
  p_offset integer,
  p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  q text := NULLIF(btrim(COALESCE(p_search, '')), '');
  eff_offset integer := GREATEST(COALESCE(p_offset, 0), 0);
  eff_sort text := COALESCE(NULLIF(p_sort, ''), 'newest');
  items jsonb;
  total_count bigint;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR eff_offset > 10000
     OR eff_sort NOT IN ('newest', 'price_asc', 'price_desc')
     OR (p_kind IS NOT NULL AND p_kind NOT IN ('card', 'pack', 'coin')) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;

  SELECT count(*) INTO total_count
  FROM app.market_listing l
  JOIN app.asset_definition ad ON ad.id = l.asset_definition_id
  LEFT JOIN app.printing p ON p.id = ad.printing_id
  LEFT JOIN app.card_definition cd ON cd.id = p.definition_id
  LEFT JOIN app.booster_recipe br ON br.id = ad.recipe_id
  LEFT JOIN app.booster_product bp ON bp.id = br.product_id
  WHERE (CASE WHEN COALESCE(p_owned, false) THEN l.seller_id = actor ELSE l.state = 'open' END)
    AND (p_kind IS NULL OR ad.kind = p_kind)
    AND (p_rarity IS NULL OR p.rarity = p_rarity)
    AND (p_min_price IS NULL OR l.total_price_coins >= p_min_price)
    AND (p_max_price IS NULL OR l.total_price_coins <= p_max_price)
    AND (q IS NULL OR cd.name ILIKE '%' || q || '%' OR bp.name ILIKE '%' || q || '%');

  WITH filtered AS (
    SELECT
      l.id,
      l.total_price_coins,
      app.market_listing_view(l, actor) || jsonb_build_object(
        'displayName', COALESCE(cd.name, bp.name, ad.currency_key, 'Item'),
        'rarity', p.rarity,
        'imageUrl', NULLIF(btrim(p.image_ref), '')
      ) AS view_json
    FROM app.market_listing l
    JOIN app.asset_definition ad ON ad.id = l.asset_definition_id
    LEFT JOIN app.printing p ON p.id = ad.printing_id
    LEFT JOIN app.card_definition cd ON cd.id = p.definition_id
    LEFT JOIN app.booster_recipe br ON br.id = ad.recipe_id
    LEFT JOIN app.booster_product bp ON bp.id = br.product_id
    WHERE (CASE WHEN COALESCE(p_owned, false) THEN l.seller_id = actor ELSE l.state = 'open' END)
      AND (p_kind IS NULL OR ad.kind = p_kind)
      AND (p_rarity IS NULL OR p.rarity = p_rarity)
      AND (p_min_price IS NULL OR l.total_price_coins >= p_min_price)
      AND (p_max_price IS NULL OR l.total_price_coins <= p_max_price)
      AND (q IS NULL OR cd.name ILIKE '%' || q || '%' OR bp.name ILIKE '%' || q || '%')
    ORDER BY
      CASE WHEN eff_sort = 'price_asc' THEN l.total_price_coins END ASC,
      CASE WHEN eff_sort = 'price_desc' THEN l.total_price_coins END DESC,
      l.id DESC
    OFFSET eff_offset
    LIMIT p_limit
  )
  SELECT COALESCE(jsonb_agg(view_json), '[]'::jsonb) INTO items FROM filtered;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'offset', eff_offset,
      'total', total_count,
      'items', items
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 8. Bilateral live editable trade rooms
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS app.trade_session (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  initiator_id uuid NOT NULL REFERENCES app.account(id),
  counterparty_id uuid NOT NULL REFERENCES app.account(id),
  state text NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'completed', 'canceled')),
  revision bigint NOT NULL DEFAULT 1 CHECK (revision >= 1),
  initiator_ready boolean NOT NULL DEFAULT false,
  counterparty_ready boolean NOT NULL DEFAULT false,
  initiator_confirmed boolean NOT NULL DEFAULT false,
  counterparty_confirmed boolean NOT NULL DEFAULT false,
  initiator_coins numeric(78,0) NOT NULL DEFAULT 0 CHECK (initiator_coins >= 0 AND initiator_coins <> 'NaN'::numeric),
  counterparty_coins numeric(78,0) NOT NULL DEFAULT 0 CHECK (counterparty_coins >= 0 AND counterparty_coins <> 'NaN'::numeric),
  created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT statement_timestamp(),
  CHECK (initiator_id <> counterparty_id)
);

CREATE TABLE IF NOT EXISTS app.trade_session_item (
  session_id uuid NOT NULL REFERENCES app.trade_session(id) ON DELETE CASCADE,
  owner_id uuid NOT NULL REFERENCES app.account(id),
  asset_definition_id uuid NOT NULL REFERENCES app.asset_definition(id),
  quantity numeric(78,0) NOT NULL CHECK (quantity > 0 AND quantity <> 'NaN'::numeric),
  PRIMARY KEY (session_id, owner_id, asset_definition_id)
);

ALTER TABLE app.trade_session ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.trade_session_item ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.trade_session, app.trade_session_item FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.trade_session_json(p_session app.trade_session) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT jsonb_build_object(
    'id', p_session.id,
    'state', p_session.state,
    'revision', p_session.revision::text,
    'initiator', app.account_identity_json(init_acc),
    'counterparty', app.account_identity_json(cp_acc),
    'initiatorReady', p_session.initiator_ready,
    'counterpartyReady', p_session.counterparty_ready,
    'initiatorConfirmed', p_session.initiator_confirmed,
    'counterpartyConfirmed', p_session.counterparty_confirmed,
    'initiatorCoins', p_session.initiator_coins::text,
    'counterpartyCoins', p_session.counterparty_coins::text,
    'initiatorItems', COALESCE((
      SELECT jsonb_agg(jsonb_build_object(
        'assetId', d.id, 'quantity', i.quantity::text, 'asset', app.asset_identity_json(d)
      ) ORDER BY d.id)
      FROM app.trade_session_item i
      JOIN app.asset_definition d ON d.id = i.asset_definition_id
      WHERE i.session_id = p_session.id AND i.owner_id = p_session.initiator_id
    ), '[]'::jsonb),
    'counterpartyItems', COALESCE((
      SELECT jsonb_agg(jsonb_build_object(
        'assetId', d.id, 'quantity', i.quantity::text, 'asset', app.asset_identity_json(d)
      ) ORDER BY d.id)
      FROM app.trade_session_item i
      JOIN app.asset_definition d ON d.id = i.asset_definition_id
      WHERE i.session_id = p_session.id AND i.owner_id = p_session.counterparty_id
    ), '[]'::jsonb),
    'createdAt', p_session.created_at,
    'updatedAt', p_session.updated_at
  )
  FROM app.account init_acc
  JOIN app.account cp_acc ON cp_acc.id = p_session.counterparty_id
  WHERE init_acc.id = p_session.initiator_id
$$;

CREATE OR REPLACE FUNCTION app.create_trade_session(p_counterparty uuid, p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  receipt app.command_receipt;
  sess app.trade_session;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_counterparty IS NULL OR p_counterparty = actor OR p_request IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  receipt := app.begin_account_command(actor, p_request, 'trade_session_create_v1', p_counterparty::text, jsonb_build_object('counterparty', p_counterparty));
  IF receipt.state <> 'pending' THEN RETURN receipt.response_payload; END IF;
  IF NOT EXISTS (SELECT 1 FROM app.account WHERE id = p_counterparty AND state = 'active')
     OR EXISTS (SELECT 1 FROM app.block WHERE (blocker_id = p_counterparty AND blocked_id = actor) OR (blocker_id = actor AND blocked_id = p_counterparty)) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('counterparty', 'unavailable'));
  END IF;

  INSERT INTO app.trade_session(initiator_id, counterparty_id)
  VALUES (actor, p_counterparty)
  RETURNING * INTO sess;

  RETURN app.finish_account_command(receipt.id, true, jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

CREATE OR REPLACE FUNCTION app.list_my_trade_sessions(p_state text, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  items jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 50
     OR (p_state IS NOT NULL AND p_state NOT IN ('active', 'completed', 'canceled')) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;
  SELECT COALESCE(jsonb_agg(app.trade_session_json(s) ORDER BY s.updated_at DESC), '[]'::jsonb)
  INTO items
  FROM (
    SELECT * FROM app.trade_session
    WHERE actor IN (initiator_id, counterparty_id)
      AND (p_state IS NULL OR state = p_state)
    ORDER BY updated_at DESC
    LIMIT p_limit
  ) s;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', actor, 'items', items));
END $$;

CREATE OR REPLACE FUNCTION app.read_my_trade_session(p_session uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id);
  IF sess.id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', actor, 'session', app.trade_session_json(sess)));
END $$;

CREATE OR REPLACE FUNCTION app.update_trade_session_offer(
  p_session uuid,
  p_expected_revision bigint,
  p_coins numeric,
  p_items jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
  item jsonb;
  a_id uuid;
  qty numeric;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_coins IS NULL OR p_coins < 0 OR jsonb_typeof(COALESCE(p_items, '[]'::jsonb)) <> 'array'
     OR jsonb_array_length(COALESCE(p_items, '[]'::jsonb)) > 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('offer', 'invalid'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id) FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  IF sess.revision <> p_expected_revision THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('revision', 'conflict'));
  END IF;

  DELETE FROM app.trade_session_item WHERE session_id = p_session AND owner_id = actor;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_items, '[]'::jsonb)) LOOP
    a_id := (item->>'assetId')::uuid;
    qty := (item->>'quantity')::numeric;
    IF a_id IS NULL OR qty IS NULL OR qty <= 0 THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid trade item';
    END IF;
    INSERT INTO app.trade_session_item(session_id, owner_id, asset_definition_id, quantity)
    VALUES (p_session, actor, a_id, qty);
  END LOOP;

  -- Any edit resets BOTH sides' readiness and confirmation to prevent bait-and-switch.
  UPDATE app.trade_session
  SET
    initiator_coins = CASE WHEN actor = initiator_id THEN p_coins ELSE initiator_coins END,
    counterparty_coins = CASE WHEN actor = counterparty_id THEN p_coins ELSE counterparty_coins END,
    initiator_ready = false,
    counterparty_ready = false,
    initiator_confirmed = false,
    counterparty_confirmed = false,
    revision = revision + 1,
    updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

CREATE OR REPLACE FUNCTION app.set_trade_session_ready(
  p_session uuid,
  p_expected_revision bigint,
  p_ready boolean
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id) FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  IF sess.revision <> p_expected_revision THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('revision', 'conflict'));
  END IF;

  UPDATE app.trade_session
  SET
    initiator_ready = CASE WHEN actor = initiator_id THEN p_ready ELSE initiator_ready END,
    counterparty_ready = CASE WHEN actor = counterparty_id THEN p_ready ELSE counterparty_ready END,
    initiator_confirmed = CASE WHEN NOT p_ready THEN false ELSE initiator_confirmed END,
    counterparty_confirmed = CASE WHEN NOT p_ready THEN false ELSE counterparty_confirmed END,
    revision = revision + 1,
    updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

CREATE OR REPLACE FUNCTION app.confirm_trade_session(
  p_session uuid,
  p_expected_revision bigint
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
  next_init_conf boolean;
  next_cp_conf boolean;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id) FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  IF sess.revision <> p_expected_revision THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('revision', 'conflict'));
  END IF;
  IF NOT (sess.initiator_ready AND sess.counterparty_ready) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('ready', 'both_parties_required'));
  END IF;

  next_init_conf := sess.initiator_confirmed OR (actor = sess.initiator_id);
  next_cp_conf := sess.counterparty_confirmed OR (actor = sess.counterparty_id);

  UPDATE app.trade_session
  SET
    initiator_confirmed = next_init_conf,
    counterparty_confirmed = next_cp_conf,
    state = CASE WHEN next_init_conf AND next_cp_conf THEN 'completed' ELSE 'active' END,
    revision = revision + 1,
    updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

CREATE OR REPLACE FUNCTION app.cancel_trade_session(p_session uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id) FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  UPDATE app.trade_session
  SET state = 'canceled', revision = revision + 1, updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

-- ---------------------------------------------------------------------------
-- 9. Searchable predictions with settled decisive result price & one-bet helper
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.search_prediction_markets(
  p_search text,
  p_state text,
  p_sort text,
  p_offset integer,
  p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  q text := NULLIF(btrim(COALESCE(p_search, '')), '');
  eff_offset integer := GREATEST(COALESCE(p_offset, 0), 0);
  eff_sort text := COALESCE(NULLIF(p_sort, ''), 'closing_soon');
  items jsonb;
  total_count bigint;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR eff_offset > 10000
     OR eff_sort NOT IN ('closing_soon', 'newest', 'resolved_recent') THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('query', 'invalid'));
  END IF;

  SELECT count(*) INTO total_count
  FROM app.prediction_market m
  JOIN app.printing p ON p.id = m.printing_id
  JOIN app.card_definition cd ON cd.id = p.definition_id
  WHERE (p_state IS NULL OR m.state = p_state)
    AND (q IS NULL OR cd.name ILIKE '%' || q || '%');

  WITH rows AS (
    SELECT
      app.prediction_market_json(m) || jsonb_build_object(
        'cardName', cd.name,
        'setCode', COALESCE(NULLIF(btrim(cs.code), ''), cs.external_set_ref),
        'imageUrl', NULLIF(btrim(p.image_ref), ''),
        'resultPriceUsd', obs.price_usd_decimal,
        'resultSampledAt', obs.observed_at
      ) AS entry
    FROM app.prediction_market m
    JOIN app.printing p ON p.id = m.printing_id
    JOIN app.card_definition cd ON cd.id = p.definition_id
    JOIN app.card_set cs ON cs.id = p.set_id
    LEFT JOIN LATERAL (
      SELECT po.price_usd_decimal, po.observed_at
      FROM app.price_observation po
      WHERE po.market_id = m.id AND po.price_usd_decimal IS NOT NULL
      ORDER BY (po.scheduled_slot_ref = m.terminal_slot_ref) DESC, po.observed_at DESC
      LIMIT 1
    ) obs ON true
    WHERE (p_state IS NULL OR m.state = p_state)
      AND (q IS NULL OR cd.name ILIKE '%' || q || '%')
    ORDER BY
      CASE WHEN eff_sort = 'closing_soon' THEN m.trading_closes_at END ASC,
      CASE WHEN eff_sort = 'resolved_recent' THEN m.resolved_at END DESC NULLS LAST,
      m.id DESC
    OFFSET eff_offset
    LIMIT p_limit
  )
  SELECT COALESCE(jsonb_agg(entry), '[]'::jsonb) INTO items FROM rows;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'offset', eff_offset,
      'total', total_count,
      'items', items
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 10. Enriched lobby detail, room option updates & named invites
-- ---------------------------------------------------------------------------
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
    'accountId', s.account_id,
    'username', CASE WHEN a.state = 'deleted' THEN 'Deleted player' ELSE a.username END,
    'supporter', CASE WHEN a.id IS NOT NULL THEN app.active_supporter_tier(a.id) IS NOT NULL ELSE false END,
    'selectedBadge', prof.selected_badge,
    'cardArtProfile', CASE WHEN prof.id IS NOT NULL THEN app.profile_art_snapshot(prof) ELSE NULL END,
    'deckRevisionId', s.deck_revision_id,
    'ready', s.ready,
    'teamRef', s.team_ref
  ) ORDER BY s.seat_index), '[]'::jsonb)
  INTO slots
  FROM app.room_slot s
  LEFT JOIN app.account a ON a.id = s.account_id
  LEFT JOIN app.profile prof ON prof.account_id = a.id
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
        'countdownAt', r.countdown_at,
        'aiDifficulty', COALESCE(r.ai_difficulty, 'standard'),
        'planechaseEnabled', r.planechase_enabled,
        'rangeOfInfluence', r.range_of_influence
      ),
      'slots', slots
    )
  );
END $$;

CREATE OR REPLACE FUNCTION app.update_room_options(
  p_room uuid,
  p_format_id uuid,
  p_ai_difficulty text,
  p_planechase_enabled boolean,
  p_range_of_influence integer
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  r app.room;
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

  UPDATE app.room
  SET
    format_id = COALESCE(p_format_id, format_id),
    ai_difficulty = COALESCE(p_ai_difficulty, ai_difficulty),
    planechase_enabled = COALESCE(p_planechase_enabled, planechase_enabled),
    range_of_influence = COALESCE(p_range_of_influence, range_of_influence),
    revision = revision + 1
  WHERE id = p_room
  RETURNING * INTO r;

  IF p_format_id IS NOT NULL THEN
    UPDATE app.room_slot SET ready = false WHERE room_id = p_room AND retired_at IS NULL;
  END IF;

  RETURN app.read_room_enriched(p_room);
END $$;

CREATE OR REPLACE FUNCTION app.invite_room_player_by_username(
  p_room uuid,
  p_username text,
  p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  r app.room;
  target app.account;
  notif_id uuid;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO r FROM app.room WHERE id = p_room AND host_id = actor AND state = 'lobby';
  IF r.id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  SELECT * INTO target FROM app.account WHERE lower(username) = lower(btrim(COALESCE(p_username, ''))) AND state = 'active' AND id <> actor;
  IF target.id IS NULL OR EXISTS (
    SELECT 1 FROM app.block WHERE (blocker_id = target.id AND blocked_id = actor) OR (blocker_id = actor AND blocked_id = target.id)
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('player', 'unavailable'));
  END IF;

  INSERT INTO app.notification(recipient_id, source_kind, source_ref, body)
  VALUES (
    target.id,
    'room_invite',
    p_room::text || ':' || COALESCE(p_request::text, gen_random_uuid()::text),
    jsonb_build_object('roomId', p_room, 'invitedBy', actor)::text
  )
  ON CONFLICT (recipient_id, source_kind, source_ref) DO UPDATE SET body = EXCLUDED.body
  RETURNING id INTO notif_id;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object('notificationId', notif_id, 'roomId', p_room, 'invitedAccountId', target.id)
  );
END $$;

-- ---------------------------------------------------------------------------
-- 11. Structured DM attachments (trade_invite / game_invite cards)
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION app.send_dm_attachment(
  p_conversation uuid,
  p_body text,
  p_attachment jsonb,
  p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  msg app.message;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_conversation IS NULL OR p_request IS NULL OR p_attachment IS NULL
     OR jsonb_typeof(p_attachment) <> 'object'
     OR p_attachment->>'kind' NOT IN ('trade_invite', 'game_invite') THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('message', 'invalid'));
  END IF;
  IF NOT EXISTS (
    SELECT 1 FROM app.conversation_member WHERE conversation_id = p_conversation AND account_id = actor
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('conversation', 'unavailable'));
  END IF;

  INSERT INTO app.message(conversation_id, sender_id, send_request_ref, body, attachment, sent_at)
  VALUES (
    p_conversation,
    actor,
    p_request::text,
    COALESCE(
      NULLIF(btrim(COALESCE(p_body, '')), ''),
      CASE WHEN p_attachment->>'kind' = 'trade_invite' THEN 'Sent a trade invite' ELSE 'Sent a game invite' END
    ),
    p_attachment,
    statement_timestamp()
  )
  ON CONFLICT (sender_id, send_request_ref) DO NOTHING
  RETURNING * INTO msg;

  IF msg.id IS NULL THEN
    SELECT * INTO msg
    FROM app.message
    WHERE sender_id = actor AND send_request_ref = p_request::text;
  END IF;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'id', msg.id,
      'conversationId', msg.conversation_id,
      'senderId', msg.sender_id,
      'body', msg.body,
      'attachment', msg.attachment,
      'sentAt', msg.sent_at
    )
  );
END $$;

-- ---------------------------------------------------------------------------
-- 12. Grants & least-privilege permissions
-- ---------------------------------------------------------------------------
REVOKE ALL ON FUNCTION
  app.card_printing_rich_json(uuid),
  app.read_card_metadata(uuid[], uuid[]),
  app.search_cards_rich(text, uuid, text, uuid, uuid, integer),
  app.delete_deck_command(uuid, uuid, uuid, uuid),
  app.duplicate_deck_command(uuid, uuid, uuid, uuid, text),
  app.read_cosmetic_catalogue_rich(text, uuid, integer),
  app.set_my_profile_badge(text),
  app.read_my_patreon_status(),
  app.read_league_leaderboard(uuid, uuid, integer, integer),
  app.read_player_public_summary(uuid),
  app.search_market_listings(text, text, text, numeric, numeric, boolean, text, integer, integer),
  app.trade_session_json(app.trade_session),
  app.create_trade_session(uuid, uuid),
  app.list_my_trade_sessions(text, integer),
  app.read_my_trade_session(uuid),
  app.update_trade_session_offer(uuid, bigint, numeric, jsonb),
  app.set_trade_session_ready(uuid, bigint, boolean),
  app.confirm_trade_session(uuid, bigint),
  app.cancel_trade_session(uuid),
  app.search_prediction_markets(text, text, text, integer, integer),
  app.read_room_enriched(uuid),
  app.update_room_options(uuid, uuid, text, boolean, integer),
  app.invite_room_player_by_username(uuid, text, uuid),
  app.send_dm_attachment(uuid, text, jsonb, uuid)
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

GRANT EXECUTE ON FUNCTION
  app.read_card_metadata(uuid[], uuid[]),
  app.search_cards_rich(text, uuid, text, uuid, uuid, integer),
  app.read_cosmetic_catalogue_rich(text, uuid, integer),
  app.set_my_profile_badge(text),
  app.read_my_patreon_status(),
  app.read_league_leaderboard(uuid, uuid, integer, integer),
  app.read_player_public_summary(uuid),
  app.search_market_listings(text, text, text, numeric, numeric, boolean, text, integer, integer),
  app.create_trade_session(uuid, uuid),
  app.list_my_trade_sessions(text, integer),
  app.read_my_trade_session(uuid),
  app.update_trade_session_offer(uuid, bigint, numeric, jsonb),
  app.set_trade_session_ready(uuid, bigint, boolean),
  app.confirm_trade_session(uuid, bigint),
  app.cancel_trade_session(uuid),
  app.search_prediction_markets(text, text, text, integer, integer),
  app.read_room_enriched(uuid),
  app.update_room_options(uuid, uuid, text, boolean, integer),
  app.invite_room_player_by_username(uuid, text, uuid),
  app.send_dm_attachment(uuid, text, jsonb, uuid)
TO authenticated;

GRANT EXECUTE ON FUNCTION
  app.delete_deck_command(uuid, uuid, uuid, uuid),
  app.duplicate_deck_command(uuid, uuid, uuid, uuid, text)
TO service_role;

-- ---------------------------------------------------------------------------
-- 13. pg_cron log retention hygiene (prune cron.job_run_details > 7 days)
-- ---------------------------------------------------------------------------
DO $cron_setup$
BEGIN
  IF to_regclass('cron.job') IS NOT NULL AND to_regprocedure('cron.schedule(text,text,text)') IS NOT NULL THEN
    PERFORM cron.schedule(
      'spellwake-prune-cron-run-details',
      '17 3 * * *',
      $prune$DELETE FROM cron.job_run_details WHERE end_time < now() - interval '7 days';$prune$
    );
  END IF;
END $cron_setup$;

COMMIT;
