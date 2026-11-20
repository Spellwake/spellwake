BEGIN;
-- No deployed consumer uses this private draft writer. Typed intents replace it.
DROP FUNCTION app.enqueue_matchmaking(uuid,uuid,boolean,text,uuid);
COMMIT;
