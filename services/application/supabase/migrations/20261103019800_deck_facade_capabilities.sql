BEGIN;
-- Verified-session facades own human deck mutations. These internal actor-based
-- implementations remain callable by their definer, without extra service RPCs.
REVOKE EXECUTE ON FUNCTION app.create_deck(uuid,text,text,uuid,uuid),app.save_deck_contents(uuid,uuid,uuid,jsonb,uuid),app.change_deck_appearance(uuid,uuid,uuid,uuid,uuid,uuid,uuid) FROM service_role;
COMMIT;
