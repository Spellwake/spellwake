BEGIN;
-- Reuse the full row/work-set proof at every mutable minimization boundary.
-- KEY SHARE freezes final retirement (which takes UPDATE), while remaining
-- compatible with normal UUID-sorted NO KEY UPDATE domain fences.
CREATE FUNCTION app.guard_retired_minimization() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actors uuid[]:='{}'::uuid[]; bindings uuid[]:='{}'::uuid[]; rooms uuid[]:='{}'::uuid[]; a app.account; additional uuid[]; source_principal text;
BEGIN
 -- Removing these rows only preserves the required absent state and must not
 -- acquire a departing player's peer account after the finalizer's own lock.
 IF TG_OP='DELETE' AND TG_TABLE_NAME IN('profile','notification','friendship') THEN RETURN NULL;END IF;
 IF TG_TABLE_NAME='account' THEN
 IF TG_OP<>'DELETE' THEN actors:=array_append(actors,NEW.id);END IF;IF TG_OP<>'INSERT' THEN actors:=array_append(actors,OLD.id);END IF;
 ELSIF TG_TABLE_NAME IN('profile','deck','purchase','cosmetic_grant','patreon_membership','rank','queue_entry','command_receipt') THEN
 IF TG_OP<>'DELETE' THEN actors:=array_append(actors,NEW.account_id);END IF;IF TG_OP<>'INSERT' THEN actors:=array_append(actors,OLD.account_id);END IF;
 IF TG_TABLE_NAME='command_receipt' THEN
 IF TG_OP<>'DELETE' AND NEW.scope IN('account_resource_cleanup_v1','account_resource_ack_v1') THEN source_principal:=NEW.principal_key;
 ELSIF TG_OP<>'INSERT' AND OLD.scope IN('account_resource_cleanup_v1','account_resource_ack_v1') THEN source_principal:=OLD.principal_key;END IF;
 IF source_principal IS NOT NULL THEN
 IF source_principal !~ '^account-resources:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='resource source principal is malformed';END IF;
 actors:=array_append(actors,substr(source_principal,length('account-resources:')+1)::uuid);
 END IF;END IF;
 ELSIF TG_TABLE_NAME='notification' THEN
 IF TG_OP<>'DELETE' THEN actors:=array_append(actors,NEW.recipient_id);END IF;IF TG_OP<>'INSERT' THEN actors:=array_append(actors,OLD.recipient_id);END IF;
 ELSIF TG_TABLE_NAME='friendship' THEN
 IF TG_OP<>'DELETE' THEN actors:=actors||ARRAY[NEW.account_low_id,NEW.account_high_id];END IF;IF TG_OP<>'INSERT' THEN actors:=actors||ARRAY[OLD.account_low_id,OLD.account_high_id];END IF;
 ELSIF TG_TABLE_NAME='controller_grant' THEN
 IF TG_OP<>'DELETE' THEN bindings:=array_append(bindings,NEW.binding_id);END IF;IF TG_OP<>'INSERT' THEN bindings:=array_append(bindings,OLD.binding_id);END IF;
 ELSIF TG_TABLE_NAME='ai_runner_binding' THEN
 IF TG_OP<>'DELETE' THEN bindings:=array_append(bindings,NEW.id);END IF;IF TG_OP<>'INSERT' THEN bindings:=array_append(bindings,OLD.id);END IF;
 ELSIF TG_TABLE_NAME='room' THEN
 IF TG_OP<>'DELETE' THEN actors:=array_append(actors,NEW.host_id);END IF;IF TG_OP<>'INSERT' THEN actors:=array_append(actors,OLD.host_id);END IF;
 ELSIF TG_TABLE_NAME IN('room_slot','match_seat') THEN
 IF TG_OP<>'DELETE' THEN actors:=actors||ARRAY[NEW.account_id,NEW.sponsor_id];END IF;IF TG_OP<>'INSERT' THEN actors:=actors||ARRAY[OLD.account_id,OLD.sponsor_id];END IF;
 IF TG_TABLE_NAME='room_slot' THEN
 IF TG_OP<>'DELETE' THEN rooms:=array_append(rooms,NEW.room_id);END IF;IF TG_OP<>'INSERT' THEN rooms:=array_append(rooms,OLD.room_id);END IF;
 ELSE
 SELECT coalesce(array_agg(room_id),'{}'::uuid[]) INTO rooms FROM app.match WHERE id IN(CASE WHEN TG_OP<>'DELETE' THEN NEW.match_id END,CASE WHEN TG_OP<>'INSERT' THEN OLD.match_id END);
 END IF;
 END IF;
 SELECT coalesce(array_agg(DISTINCT account_id ORDER BY account_id),'{}'::uuid[]) INTO additional FROM(
 SELECT slot.sponsor_id account_id FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE b.id=ANY(bindings)
 UNION SELECT room.host_id FROM app.room room WHERE room.id=ANY(rooms) OR EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.room_slot slot ON slot.id=b.room_slot_id WHERE b.id=ANY(bindings) AND slot.room_id=room.id)) ids;
 actors:=actors||additional;
 FOR a IN SELECT * FROM app.account WHERE id=ANY(actors) ORDER BY id FOR KEY SHARE LOOP
 -- Unqualified historical lifecycle fixtures stay incomplete under the strict
 -- reader. This guard preserves a produced v2 proof, not a bare deleted flag.
 IF a.state='deleted' AND EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.account_id=a.id AND source.principal_key='human:'||a.id::text AND source.scope='account_retirement_v1' AND source.request_ref=a.id::text AND source.normalized_source->>'version'='account-minimization-v2') THEN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='minimized source changes require read committed retry';END IF;
 IF (app.read_account_retirement_proof(a.id,a.retirement_operation_id)).id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retired personal rows and credential work set must retain exact minimization proof';END IF;END IF;
 END LOOP;RETURN NULL;
END $$;
DO $$ DECLARE table_name text; BEGIN
 FOREACH table_name IN ARRAY ARRAY['account','profile','deck','purchase','cosmetic_grant','patreon_membership','rank','queue_entry','command_receipt','notification','friendship','controller_grant','ai_runner_binding','room','room_slot','match_seat'] LOOP
 EXECUTE format('CREATE CONSTRAINT TRIGGER retired_minimization AFTER INSERT OR UPDATE OR DELETE ON app.%I DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_retired_minimization()',table_name);
 END LOOP;
END $$;
REVOKE ALL ON FUNCTION app.guard_retired_minimization() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
