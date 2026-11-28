BEGIN;

-- Register and backfill together so a concurrent account cannot miss the free choice.
SET LOCAL lock_timeout = '5s';
LOCK TABLE app.account IN SHARE ROW EXCLUSIVE MODE;

-- Sort after existing starter backs in app.free_cosmetic. Registration still
-- uses the configured Spellwake back and grants every active free choice.
INSERT INTO app.cosmetic (id, kind, asset_ref, state, is_free, display_name)
VALUES (
  'ffffffff-ffff-4fff-bfff-ffffffffffff',
  'back',
  'scryfall:card-back-standard',
  'active',
  true,
  'Magic'
);

INSERT INTO app.cosmetic_grant (account_id, cosmetic_id, source_kind, state)
SELECT id, 'ffffffff-ffff-4fff-bfff-ffffffffffff', 'free', 'active'
FROM app.account
WHERE state = 'active'
ORDER BY id;

COMMIT;
