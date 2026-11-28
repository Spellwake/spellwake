BEGIN;

-- Register card and sleeve condition effects and backfill free tiers atomically.
SET LOCAL lock_timeout = '5s';
LOCK TABLE app.account IN SHARE ROW EXCLUSIVE MODE;

ALTER TABLE app.cosmetic
  DROP CONSTRAINT IF EXISTS cosmetic_kind,
  ADD CONSTRAINT cosmetic_kind CHECK (
    kind = ANY (
      ARRAY[
        'back'::text,
        'sleeve'::text,
        'dice'::text,
        'playmat'::text,
        'pet'::text,
        'card_effect'::text,
        'sleeve_effect'::text
      ]
    )
  );

-- Card condition effects: Near Mint (default) and Lightly Played are free to all players.
-- Moderately Played, Heavily Played, Damaged, and Unplayable are sold at $2.99 (299 cents).
INSERT INTO app.cosmetic (
  id, kind, asset_ref, state, is_free, display_name, name, description, price_cents, rarity
)
VALUES
  (
    'e0000001-0001-4000-8000-000000000001',
    'card_effect',
    'effect:card:nm',
    'active',
    true,
    'Near Mint',
    'Near Mint',
    'Pack-fresh cards with zero wear or surface distortion.',
    0,
    'common'
  ),
  (
    'e0000001-0001-4000-8000-000000000002',
    'card_effect',
    'effect:card:lp',
    'active',
    true,
    'Lightly Played',
    'Lightly Played',
    'Subtle edge whitening and light shuffle gloss from gentle table play.',
    0,
    'common'
  ),
  (
    'e0000001-0001-4000-8000-000000000003',
    'card_effect',
    'effect:card:mp',
    'active',
    false,
    'Moderately Played',
    'Moderately Played',
    'Noticeable border whitening, matte surface rubs, and light shuffle scuffs.',
    299,
    'uncommon'
  ),
  (
    'e0000001-0001-4000-8000-000000000004',
    'card_effect',
    'effect:card:hp',
    'active',
    false,
    'Heavily Played',
    'Heavily Played',
    'Heavy border chipping, dull matte wear, and gentle corner bending.',
    299,
    'rare'
  ),
  (
    'e0000001-0001-4000-8000-000000000005',
    'card_effect',
    'effect:card:dmg',
    'active',
    false,
    'Damaged',
    'Damaged',
    'Visible crease lines, worn cardstock edges, and pronounced corner bends.',
    299,
    'rare'
  ),
  (
    'e0000001-0001-4000-8000-000000000006',
    'card_effect',
    'effect:card:unplayable',
    'active',
    false,
    'Unplayable',
    'Unplayable',
    'Deep fold cracks, frayed borders, and warped corners from decades of kitchen-table games.',
    299,
    'mythic'
  ),
  -- Sleeve condition effects: Near Mint (default) and Lightly Played are free to all players.
  -- Moderately Played, Heavily Played, Damaged, and Unplayable are sold at $2.99 (299 cents).
  (
    'e0000002-0001-4000-8000-000000000001',
    'sleeve_effect',
    'effect:sleeve:nm',
    'active',
    true,
    'Near Mint',
    'Near Mint',
    'Fresh out of the box with flat edges, crisp corners, and clear front film.',
    0,
    'common'
  ),
  (
    'e0000002-0001-4000-8000-000000000002',
    'sleeve_effect',
    'effect:sleeve:lp',
    'active',
    true,
    'Lightly Played',
    'Lightly Played',
    'A subtle wave along the top opening with a slight inward bend on one corner tip.',
    0,
    'common'
  ),
  (
    'e0000002-0001-4000-8000-000000000003',
    'sleeve_effect',
    'effect:sleeve:mp',
    'active',
    false,
    'Moderately Played',
    'Moderately Played',
    'Distinct bent corner tips, a wavy top lip, and light matte haze from shuffling.',
    299,
    'uncommon'
  ),
  (
    'e0000002-0001-4000-8000-000000000004',
    'sleeve_effect',
    'effect:sleeve:hp',
    'active',
    false,
    'Heavily Played',
    'Heavily Played',
    'Sharp folded corner tips, speckled shuffle grime, cloudy front film, and puffed deck edges.',
    299,
    'rare'
  ),
  (
    'e0000002-0001-4000-8000-000000000005',
    'sleeve_effect',
    'effect:sleeve:dmg',
    'active',
    false,
    'Damaged',
    'Damaged',
    'Heavy grime, bent corners across the sleeve, and a splitting side weld seam.',
    299,
    'rare'
  ),
  (
    'e0000002-0001-4000-8000-000000000006',
    'sleeve_effect',
    'effect:sleeve:unplayable',
    'active',
    false,
    'Unplayable',
    'Unplayable',
    'Split side seam and a torn, furled corner exposing the card inside the sleeve.',
    299,
    'mythic'
  )
ON CONFLICT (id) DO NOTHING;

-- Backfill active free grants for Near Mint and Lightly Played card & sleeve effects.
INSERT INTO app.cosmetic_grant (account_id, cosmetic_id, source_kind, state)
SELECT a.id, c.id, 'free', 'active'
FROM app.account a
CROSS JOIN (
  VALUES
    ('e0000001-0001-4000-8000-000000000001'::uuid),
    ('e0000001-0001-4000-8000-000000000002'::uuid),
    ('e0000002-0001-4000-8000-000000000001'::uuid),
    ('e0000002-0001-4000-8000-000000000002'::uuid)
) AS c(id)
WHERE a.state = 'active'
ON CONFLICT DO NOTHING;

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
     OR (p_kind IS NOT NULL AND p_kind NOT IN (
       'back', 'sleeve', 'dice', 'playmat', 'pet', 'card_effect', 'sleeve_effect'
     )) THEN
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
      'owned', c.is_free OR EXISTS (
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

COMMIT;
