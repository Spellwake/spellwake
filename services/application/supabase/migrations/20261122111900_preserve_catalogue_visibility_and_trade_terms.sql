BEGIN;

-- Use the accepted catalogue projection for authenticated rich reads.
CREATE OR REPLACE FUNCTION app.card_printing_rich_json(p_printing_id uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  WITH latest_legality AS (
    SELECT id FROM app.legality_revision ORDER BY effective_at DESC, imported_at DESC LIMIT 1
  ),
  latest_rev AS (
    SELECT DISTINCT ON (r.definition_id)
      r.id, r.definition_id, r.rules_text_ref
    FROM app.card_definition_revision r
    WHERE app.catalogue_source_visible(r.catalogue_receipt_id)
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
    'imageUrl', app.printing_art_url(p.image_ref,p.external_printing_ref),
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
    AND app.catalogue_source_visible(p.catalogue_receipt_id)
    AND app.catalogue_source_visible(d.catalogue_receipt_id)
    AND app.catalogue_source_visible(s.catalogue_receipt_id)
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

  SELECT COALESCE(jsonb_agg(entry ORDER BY p.id) FILTER (WHERE entry IS NOT NULL), '[]'::jsonb)
  INTO printings_out
  FROM app.printing p
  CROSS JOIN LATERAL app.card_printing_rich_json(p.id) AS card(entry)
  WHERE p.id = ANY(p_ids);

  SELECT COALESCE(jsonb_agg(
    first_printing.entry ORDER BY first_printing.definition_id
  ), '[]'::jsonb)
  INTO definitions_out
  FROM (
    SELECT DISTINCT ON (p.definition_id) p.definition_id, entry
    FROM app.printing p
    CROSS JOIN LATERAL app.card_printing_rich_json(p.id) AS card(entry)
    WHERE p.definition_id = ANY(d_ids) AND entry IS NOT NULL
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
    SELECT p.id, entry
    FROM app.printing p
    JOIN app.card_definition d ON d.id = p.definition_id
    CROSS JOIN LATERAL app.card_printing_rich_json(p.id) AS card(entry)
    WHERE entry IS NOT NULL AND (p_after IS NULL OR p.id > p_after)
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

-- Apply the live listing state to both rich search queries.
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
  WHERE (CASE WHEN COALESCE(p_owned, false) THEN l.seller_id = actor ELSE l.state = 'listed' END)
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
        'imageUrl', app.printing_art_url(p.image_ref,p.external_printing_ref)
      ) AS view_json
    FROM app.market_listing l
    JOIN app.asset_definition ad ON ad.id = l.asset_definition_id
    LEFT JOIN app.printing p ON p.id = ad.printing_id
    LEFT JOIN app.card_definition cd ON cd.id = p.definition_id
    LEFT JOIN app.booster_recipe br ON br.id = ad.recipe_id
    LEFT JOIN app.booster_product bp ON bp.id = br.product_id
    WHERE (CASE WHEN COALESCE(p_owned, false) THEN l.seller_id = actor ELSE l.state = 'listed' END)
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

-- Roll back all offer writes when any requested asset is unavailable.
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
  owner_id uuid;
  coin_id uuid;
  item jsonb;
  a_id uuid;
  qty numeric;
  proposed_bundle jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_coins IS NULL OR p_coins < 0 OR p_coins <> trunc(p_coins)
     OR jsonb_typeof(COALESCE(p_items, '[]'::jsonb)) <> 'array'
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

  SELECT id INTO coin_id FROM app.asset_definition WHERE kind = 'coin' AND currency_key = 'coin' LIMIT 1;
  SELECT id INTO owner_id FROM app.asset_owner WHERE account_id = actor AND state = 'active';
  IF owner_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  DELETE FROM app.trade_session_item line
  WHERE line.session_id = p_session AND line.owner_id = actor;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_items, '[]'::jsonb)) LOOP
    a_id := (item->>'assetId')::uuid;
    qty := (item->>'quantity')::numeric;
    IF a_id IS NULL OR qty IS NULL OR qty <= 0 OR qty <> trunc(qty) OR a_id = coin_id THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid trade item';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM app.asset_definition WHERE id = a_id) THEN
      RAISE EXCEPTION USING ERRCODE = 'SW001', MESSAGE = 'trade asset unavailable';
    END IF;
    INSERT INTO app.trade_session_item(session_id, owner_id, asset_definition_id, quantity)
    VALUES (p_session, actor, a_id, qty);
  END LOOP;

  proposed_bundle := app.trade_session_bundle(p_session, actor, p_coins);
  IF jsonb_array_length(proposed_bundle) > 0 AND NOT app.bundle_available(owner_id, proposed_bundle) THEN
    RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'insufficient available trade assets';
  END IF;

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
EXCEPTION
  WHEN SQLSTATE 'SW001' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('assets', 'unavailable'));
  WHEN raise_exception THEN
    IF SQLERRM = 'insufficient available trade assets' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('assets', 'not enough available offered assets'));
    END IF;
    RAISE;
  WHEN unique_violation OR invalid_parameter_value OR invalid_text_representation THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('offer', 'invalid'));
END $$;

COMMIT;
