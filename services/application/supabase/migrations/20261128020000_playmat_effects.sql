BEGIN;

-- Register all 8 approved Playmat Effects (Clean free default + 7 paid effects
-- priced $0.49 to $1.49) and backfill free Clean grants to active accounts.
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
        'sleeve_effect'::text,
        'playmat_effect'::text
      ]
    )
  );

INSERT INTO app.cosmetic (
  id, kind, asset_ref, state, is_free, display_name, name, description, price_cents, rarity
)
VALUES
  (
    'e0000004-0001-4000-8000-000000000001',
    'playmat_effect',
    'effect:playmat:clean',
    'active',
    true,
    'Clean',
    'Clean',
    'Fresh out of the tube with zero wear, stains, or creases.',
    0,
    'common'
  ),
  (
    'e0000004-0001-4000-8000-000000000002',
    'playmat_effect',
    'effect:playmat:dusty',
    'active',
    false,
    'Dusty & Chalky',
    'Dusty & Chalky',
    'Fine settled tabletop dust in outer corners with soft swept card-slide trails.',
    49,
    'common'
  ),
  (
    'e0000004-0001-4000-8000-000000000003',
    'playmat_effect',
    'effect:playmat:oil_stained',
    'active',
    false,
    'Oil Stained',
    'Oil Stained',
    'Translucent snack and hand oil blots soaking darker into the playmat weave.',
    99,
    'common'
  ),
  (
    'e0000004-0001-4000-8000-000000000004',
    'playmat_effect',
    'effect:playmat:frayed',
    'active',
    false,
    'Frayed & Fuzzy',
    'Frayed & Fuzzy',
    'Unraveled edge threads, clustered fabric pilling fuzz, and worn forearm abrasion.',
    99,
    'uncommon'
  ),
  (
    'e0000004-0001-4000-8000-000000000005',
    'playmat_effect',
    'effect:playmat:creased',
    'active',
    false,
    'Rolled & Creased',
    'Rolled & Creased',
    'Matte neoprene tube-roll waves, soft backpack quarter-fold memory, and lifted side curls.',
    99,
    'uncommon'
  ),
  (
    'e0000004-0001-4000-8000-000000000006',
    'playmat_effect',
    'effect:playmat:drink_spill',
    'active',
    false,
    'Coffee Ring & Spill',
    'Coffee Ring & Spill',
    'Dried mug ring with dark capillary rim, secondary ring, and cloth-soaked spill stain.',
    149,
    'rare'
  ),
  (
    'e0000004-0001-4000-8000-000000000007',
    'playmat_effect',
    'effect:playmat:lgs_grime',
    'active',
    false,
    'LGS Veteran Grime',
    'LGS Veteran Grime',
    'Darkened wrist-rest patina along the bottom edge, handled corners, and bottom-left deck outline.',
    149,
    'rare'
  ),
  (
    'e0000004-0001-4000-8000-000000000008',
    'playmat_effect',
    'effect:playmat:scorched',
    'active',
    false,
    'Ember Scorched',
    'Ember Scorched',
    'Singed corner with toasted-amber heat halo, carbonized soot core, and stray ember burns.',
    149,
    'mythic'
  )
ON CONFLICT (id) DO NOTHING;

-- Backfill free Clean playmat effect grants to active accounts.
INSERT INTO app.cosmetic_grant (account_id, cosmetic_id, source_kind, state)
SELECT a.id, 'e0000004-0001-4000-8000-000000000001'::uuid, 'free', 'active'
FROM app.account a
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
       'back', 'sleeve', 'dice', 'playmat', 'pet',
       'card_effect', 'sleeve_effect', 'playmat_effect'
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
