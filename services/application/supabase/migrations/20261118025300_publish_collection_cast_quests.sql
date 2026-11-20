BEGIN;
-- The native producer counts completed casts by their actual colors. The
-- existing game consumer limits these facts to human accounts in Collection.
INSERT INTO app.quest_definition_revision
 (id, definition_key, revision, objective_ref, target, reward_coins,
  fact_kind, fact_schema_revision, objective_qualification_ref, reroll_enabled)
VALUES
 ('844a8cc2-293c-513b-963b-c923301dc5a9', 'cast-red-or-green-20', 'v1', 'native:cast-red-or-green', 20, 500,
  'native:cast-red-or-green', 'native-cast-colors-v1', 'collection-cast-v1/cast-red-or-green-20', true),
 ('de55131b-451c-506f-80df-f9a559ed373a', 'cast-black-or-green-20', 'v1', 'native:cast-black-or-green', 20, 500,
  'native:cast-black-or-green', 'native-cast-colors-v1', 'collection-cast-v1/cast-black-or-green-20', true),
 ('ee640edd-0c16-5f88-b8a5-c0a99190231c', 'cast-red-or-green-30', 'v1', 'native:cast-red-or-green', 30, 750,
  'native:cast-red-or-green', 'native-cast-colors-v1', 'collection-cast-v1/cast-red-or-green-30', true);

-- The private worker delivers a frozen definition selection and advances one
-- bounded activity cursor. Account ownership, dates, retries and payout remain
-- in the existing transactions. Neither function can be invoked by players.
GRANT EXECUTE ON FUNCTION app.generate_daily_quest(uuid,uuid,text,uuid),
 app.consume_quest_game_activity(uuid,integer) TO service_role;
COMMIT;
