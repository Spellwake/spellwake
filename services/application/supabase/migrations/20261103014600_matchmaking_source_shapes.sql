BEGIN;
ALTER TABLE app.table_profile_revision ADD COLUMN matchmaking_config jsonb;
ALTER TABLE app.queue_entry ADD COLUMN intent_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.queue_entry ADD COLUMN alternative_index bigint CHECK(alternative_index IS NULL OR alternative_index>=0);
ALTER TABLE app.queue_entry ADD COLUMN format_id uuid REFERENCES app.format(id) ON DELETE RESTRICT;
ALTER TABLE app.queue_entry ADD COLUMN preferences jsonb;
ALTER TABLE app.queue_entry ADD COLUMN claimed_room_id uuid REFERENCES app.room(id) ON DELETE RESTRICT;
ALTER TABLE app.queue_entry ADD COLUMN claim_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX queue_intent_alternative ON app.queue_entry(intent_receipt_id,alternative_index);
CREATE UNIQUE INDEX queue_intent_deck ON app.queue_entry(intent_receipt_id,deck_revision_id);
ALTER TABLE app.queue_entry ADD CONSTRAINT typed_queue_intent_tuple CHECK(
 (intent_receipt_id IS NULL AND alternative_index IS NULL AND format_id IS NULL AND preferences IS NULL)
 OR (intent_receipt_id IS NOT NULL AND alternative_index IS NOT NULL AND format_id IS NOT NULL AND preferences IS NOT NULL AND jsonb_typeof(preferences)='object'));
ALTER TABLE app.queue_entry DROP CONSTRAINT queue_state_source;
ALTER TABLE app.queue_entry ADD CONSTRAINT queue_state_source CHECK(
 (state='queued' AND reserved_match_id IS NULL AND claimed_room_id IS NULL AND claim_receipt_id IS NULL)
 OR (state='claimed' AND reserved_match_id IS NULL AND claimed_room_id IS NOT NULL AND claim_receipt_id IS NOT NULL)
 OR (state='reserved' AND reserved_match_id IS NOT NULL AND claimed_room_id IS NOT NULL AND claim_receipt_id IS NOT NULL)
 OR (state='canceled' AND reserved_match_id IS NULL AND ((claimed_room_id IS NULL AND claim_receipt_id IS NULL) OR (claimed_room_id IS NOT NULL AND claim_receipt_id IS NOT NULL))));
CREATE OR REPLACE FUNCTION app.guard_queue_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
 IF NEW.state<>'queued' OR NEW.reserved_match_id IS NOT NULL OR NEW.claimed_room_id IS NOT NULL OR NEW.claim_receipt_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue entry requires original unclaimed source'; END IF;RETURN NEW;
 END IF;
 IF TG_OP='DELETE' OR (NEW.id,NEW.account_id,NEW.deck_revision_id,NEW.mode,NEW.competitive,NEW.preferences_ref,NEW.intent_receipt_id,NEW.alternative_index,NEW.format_id,NEW.preferences) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.deck_revision_id,OLD.mode,OLD.competitive,OLD.preferences_ref,OLD.intent_receipt_id,OLD.alternative_index,OLD.format_id,OLD.preferences)
 OR (OLD.state IN ('reserved','canceled') AND NEW IS DISTINCT FROM OLD)
 OR (OLD.state='queued' AND NEW.state NOT IN ('queued','claimed','canceled')) OR (OLD.state='claimed' AND NEW.state NOT IN ('claimed','reserved','canceled'))
 OR (OLD.claimed_room_id IS NOT NULL AND (NEW.claimed_room_id,NEW.claim_receipt_id) IS DISTINCT FROM (OLD.claimed_room_id,OLD.claim_receipt_id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue intent, claim and terminal reservation are immutable'; END IF;
 RETURN NEW;
END $$;
-- Influence tokens: none means not applicable; distance:N is a positive seat distance.
CREATE FUNCTION app.guard_matchmaking_profile() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE configuration jsonb; count_value integer; item jsonb;
BEGIN
 IF NEW.matchmaking_config IS NULL THEN RETURN NEW; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='profile qualification requires read committed retry'; END IF;
 IF NEW.admission_qualification_ref IS NULL OR jsonb_typeof(NEW.matchmaking_config) IS DISTINCT FROM 'object'
 OR NEW.matchmaking_config->>'version' IS DISTINCT FROM 'matchmaking-profile-v1'
 OR jsonb_typeof(NEW.matchmaking_config->'influence') IS DISTINCT FROM 'string' OR NEW.matchmaking_config->>'influence' !~ '^(none|unlimited|distance:[1-9][0-9]?)$'
 OR jsonb_typeof(NEW.matchmaking_config->'planechase') IS DISTINCT FROM 'boolean'
 OR jsonb_typeof(NEW.matchmaking_config->'qualificationRef') IS DISTINCT FROM 'string' OR NEW.matchmaking_config->>'qualificationRef' !~ '^sha256:[0-9a-f]{64}$'
 OR jsonb_typeof(NEW.matchmaking_config->'configurations') IS DISTINCT FROM 'array'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(NEW.matchmaking_config) k WHERE k NOT IN ('version','influence','planechase','qualificationRef','configurations')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified typed matchmaking profile required'; END IF;
 IF NEW.matchmaking_config->'planechase'='true'::jsonb AND (NEW.planar_pool_ref IS NULL OR length(btrim(NEW.planar_pool_ref)) NOT BETWEEN 1 AND 2048) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Planechase configuration requires qualified shared planar pool';END IF;
 IF jsonb_array_length(NEW.matchmaking_config->'configurations') NOT BETWEEN 1 AND 512 OR (SELECT count(DISTINCT x->>'key') FROM jsonb_array_elements(NEW.matchmaking_config->'configurations') x)<>jsonb_array_length(NEW.matchmaking_config->'configurations') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='profile needs bounded distinct concrete configurations'; END IF;
 FOR configuration IN SELECT * FROM jsonb_array_elements(NEW.matchmaking_config->'configurations') LOOP
 IF jsonb_typeof(configuration) IS DISTINCT FROM 'object'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['key','formatId','seatCount','structure']) k WHERE jsonb_typeof(configuration->k) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(configuration) k WHERE k NOT IN ('key','formatId','seatCount','structure','teams'))
 OR length(btrim(configuration->>'key')) NOT BETWEEN 1 AND 128 OR configuration->>'formatId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR configuration->>'seatCount' !~ '^[2-6]$' OR configuration->>'structure' NOT IN ('bo1','bo3') OR jsonb_typeof(configuration->'teams') IS DISTINCT FROM 'array' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid concrete matchmaking configuration'; END IF;
 count_value:=(configuration->>'seatCount')::integer;
 IF count_value NOT BETWEEN NEW.minimum_seats AND NEW.seat_capacity OR (configuration->>'structure'='bo3' AND count_value<>2) OR jsonb_array_length(configuration->'teams')<>count_value THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='matchmaking count/structure/team layout is not supported'; END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(configuration->'teams') LOOP
 IF jsonb_typeof(item) NOT IN ('string','null') OR (jsonb_typeof(item)='string' AND length(btrim(item#>>'{}')) NOT BETWEEN 1 AND 128) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid concrete team reference'; END IF;
 END LOOP;
 PERFORM id FROM app.format WHERE id=(configuration->>'formatId')::uuid FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='qualified configuration format unavailable'; END IF;
 END LOOP;
 RETURN NEW;
END $$;
CREATE TRIGGER matchmaking_profile_source BEFORE INSERT OR UPDATE ON app.table_profile_revision FOR EACH ROW EXECUTE FUNCTION app.guard_matchmaking_profile();
CREATE FUNCTION app.guard_profile_format_reference() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP='UPDATE' AND NEW.id=OLD.id THEN RETURN NEW; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='format identity change requires read committed retry'; END IF;
 IF EXISTS(SELECT 1 FROM app.table_profile_revision p CROSS JOIN LATERAL jsonb_array_elements(p.matchmaking_config->'configurations') config WHERE config->>'formatId'=OLD.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified matchmaking profile retains this format identity'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;RETURN NEW;
END $$;
CREATE TRIGGER profile_format_reference BEFORE DELETE OR UPDATE ON app.format FOR EACH ROW EXECUTE FUNCTION app.guard_profile_format_reference();
REVOKE ALL ON FUNCTION app.guard_matchmaking_profile(),app.guard_profile_format_reference() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
