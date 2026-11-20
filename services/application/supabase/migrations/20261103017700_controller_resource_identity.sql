BEGIN;
-- A private canonical secret-store generation belongs to one immutable grant.
-- Retained revoked history reserves it forever, including after proven removal.
ALTER TABLE app.controller_grant ADD CONSTRAINT controller_secret_reference_bytes CHECK(octet_length(grant_secret_ref) BETWEEN 1 AND 2048);
ALTER TABLE app.controller_grant ADD CONSTRAINT controller_secret_generation_unique UNIQUE(grant_secret_ref);
CREATE OR REPLACE FUNCTION app.issue_ai_grant(p_binding uuid,p_epoch numeric,p_secret_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room; grant_id uuid;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_secret_ref IS NULL OR length(btrim(p_secret_ref))=0 OR octet_length(p_secret_ref) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid grant issue envelope';END IF;
 receipt:=app.begin_ai_runtime_command('ai_grant_issue_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'secretRef',p_secret_ref));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id AND (state='active' OR app.ai_sponsor_continuing_match(p_binding,slot.sponsor_id)) FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sponsor','unavailable'));END IF;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF slot.id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.room_slot current_slot WHERE current_slot.id=slot.id AND current_slot.room_id=slot.room_id AND current_slot.sponsor_id IS NOT DISTINCT FROM slot.sponsor_id AND current_slot.retired_at IS NOT DISTINCT FROM slot.retired_at) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='runtime slot source changed before account/room fence';END IF;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL OR binding.binding_epoch<>p_epoch OR binding.desired_state<>'running' OR binding.observed_state NOT IN ('running','ready') OR binding.provider_runner_ref IS NULL OR slot.retired_at IS NOT NULL OR lobby.state='closed'
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=slot.room_id AND state IN ('completed','no_contest'))
 OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=binding.id AND binding_epoch=p_epoch AND state='active') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('binding','stale or unavailable'));END IF;
 INSERT INTO app.controller_grant(binding_id,binding_epoch,match_seat_id,grant_secret_ref,state,issue_receipt_id) VALUES(binding.id,p_epoch,binding.match_seat_id,p_secret_ref,'active',receipt.id) RETURNING id INTO grant_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',grant_id,'delivery','pending'));
END $$;
COMMIT;
