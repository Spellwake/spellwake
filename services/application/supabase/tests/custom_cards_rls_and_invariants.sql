-- Stage 2: Dedicated RLS & Authorization / Invariant SQL verification suite for Custom Cards.
-- Execute inside a rolled-back transaction against a migrated Spellwake Postgres database:
--   psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f services/application/supabase/tests/custom_cards_rls_and_invariants.sql
BEGIN;

DO $$
DECLARE
  rls_count integer;
  overload_count integer;
BEGIN
  -- 1. Verify RLS + FORCE RLS enabled on all 5 new custom-card tables.
  SELECT count(*) INTO rls_count
  FROM pg_class c
  JOIN pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = 'app'
    AND c.relname IN (
      'custom_card',
      'custom_card_revision',
      'custom_card_revision_art',
      'deck_custom_entry',
      'match_revealed_custom_card'
    )
    AND c.relrowsecurity = true
    AND c.relforcerowsecurity = true;
  IF rls_count <> 5 THEN
    RAISE EXCEPTION 'Expected FORCE RLS on all 5 custom card tables, got %', rls_count;
  END IF;

  -- 2. Verify zero PostgREST PGRST203 duplicate function overloads.
  SELECT count(*) INTO overload_count
  FROM (
    SELECT proname, count(*) AS cnt
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'app'
      AND proname IN ('save_deck_command', 'save_deck_contents', 'deck_face_card_json', 'update_room_options')
    GROUP BY proname
    HAVING count(*) > 1
  ) dups;
  IF overload_count > 0 THEN
    RAISE EXCEPTION 'Duplicate SQL overloads detected (PGRST203 hazard)';
  END IF;

  -- 3. Verify anon has ZERO table privileges on custom card tables.
  IF has_table_privilege('anon', 'app.custom_card', 'SELECT')
     OR has_table_privilege('anon', 'app.custom_card_revision', 'SELECT')
     OR has_table_privilege('anon', 'app.custom_card_revision_art', 'SELECT')
     OR has_table_privilege('anon', 'app.deck_custom_entry', 'SELECT')
     OR has_table_privilege('anon', 'app.match_revealed_custom_card', 'SELECT') THEN
    RAISE EXCEPTION 'anon must not have SELECT on any custom card table';
  END IF;

  -- 4. Verify authenticated CANNOT directly SELECT binary art or match reveal tables,
  --    and CANNOT directly INSERT/UPDATE/DELETE any custom card table.
  IF has_table_privilege('authenticated', 'app.custom_card_revision_art', 'SELECT')
     OR has_table_privilege('authenticated', 'app.match_revealed_custom_card', 'SELECT') THEN
    RAISE EXCEPTION 'authenticated must not have direct SELECT on custom_card_revision_art or match_revealed_custom_card';
  END IF;

  IF has_table_privilege('authenticated', 'app.custom_card', 'INSERT')
     OR has_table_privilege('authenticated', 'app.custom_card', 'UPDATE')
     OR has_table_privilege('authenticated', 'app.custom_card', 'DELETE')
     OR has_table_privilege('authenticated', 'app.custom_card_revision', 'INSERT')
     OR has_table_privilege('authenticated', 'app.deck_custom_entry', 'INSERT') THEN
    RAISE EXCEPTION 'authenticated must not have direct DML on custom card tables';
  END IF;

  -- 5. Verify CHECK constraints enforce Open Play isolation on app.room and app.match,
  --    and face-card mutual exclusivity on app.deck.
  IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'deck_face_card_mutual_exclusivity')
     OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'room_allow_custom_cards_open_only')
     OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'room_share_host_decks_open_only')
     OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'match_allow_custom_cards_open_only')
     OR NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'match_share_host_decks_open_only') THEN
    RAISE EXCEPTION 'Missing expected CHECK constraints for Open Play isolation or face-card mutual exclusivity';
  END IF;

  -- 6. Verify valid_deck_settings_change enforces faceCardId vs faceCustomCardRevisionId mutual exclusivity.
  IF app.valid_deck_settings_change(jsonb_build_object(
       'name', 'Test',
       'mode', 'open',
       'formatId', '11111111-1111-1111-1111-111111111111',
       'appearance', jsonb_build_object(
         'backId', '11111111-1111-1111-1111-111111111111',
         'sleeveId', '22222222-2222-2222-2222-222222222222',
         'diceId', '33333333-3333-3333-3333-333333333333',
         'playmatId', NULL
       ),
       'faceCardId', '44444444-4444-4444-4444-444444444444',
       'faceCustomCardRevisionId', '55555555-5555-5555-5555-555555555555'
     )) THEN
    RAISE EXCEPTION 'valid_deck_settings_change must reject setting both faceCardId and faceCustomCardRevisionId';
  END IF;

  IF NOT app.valid_deck_settings_change(jsonb_build_object(
       'name', 'Test',
       'mode', 'open',
       'formatId', '11111111-1111-1111-1111-111111111111',
       'appearance', jsonb_build_object(
         'backId', '11111111-1111-1111-1111-111111111111',
         'sleeveId', '22222222-2222-2222-2222-222222222222',
         'diceId', '33333333-3333-3333-3333-333333333333',
         'playmatId', NULL
       ),
       'faceCustomCardRevisionId', '55555555-5555-5555-5555-555555555555'
     )) THEN
    RAISE EXCEPTION 'valid_deck_settings_change must accept valid faceCustomCardRevisionId';
  END IF;

  -- 7. Verify GDPR minimization actions v3 includes custom_cards while preserving v2 function.
  IF NOT (app.account_minimization_actions_v3() ? 'custom_cards') THEN
    RAISE EXCEPTION 'account_minimization_actions_v3 must include custom_cards';
  END IF;
  IF (app.account_minimization_actions() ? 'custom_cards') THEN
    RAISE EXCEPTION 'historical account_minimization_actions (v2) must remain unchanged for v2 proofs';
  END IF;
END $$;

ROLLBACK;
