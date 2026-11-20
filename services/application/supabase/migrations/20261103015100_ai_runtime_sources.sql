BEGIN;
ALTER TABLE app.ai_runner_binding ADD COLUMN bootstrap_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD COLUMN epoch_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD COLUMN provider_observation_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD COLUMN stop_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD COLUMN stop_observation_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.ai_runner_binding ADD CONSTRAINT ai_runtime_source_tuple CHECK((bootstrap_receipt_id IS NULL)=(epoch_receipt_id IS NULL));
CREATE UNIQUE INDEX runner_bootstrap_source ON app.ai_runner_binding(bootstrap_receipt_id) WHERE bootstrap_receipt_id IS NOT NULL;
ALTER TABLE app.controller_grant ADD COLUMN issue_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
ALTER TABLE app.controller_grant ADD COLUMN delivery_state text NOT NULL DEFAULT 'pending' CHECK(delivery_state IN ('pending','unknown','delivered'));
ALTER TABLE app.controller_grant ADD COLUMN delivery_revision numeric(78,0) NOT NULL DEFAULT 0 CHECK(delivery_revision>=0);
ALTER TABLE app.controller_grant ADD COLUMN delivery_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX controller_epoch_authority ON app.controller_grant(binding_id,binding_epoch) WHERE state='active';

-- Private normalized runtime intake. External process/browser truth is verified by its later adapter.
CREATE FUNCTION app.begin_ai_runtime_command(p_scope text,p_target uuid,p_request uuid,p_terms jsonb) RETURNS app.command_receipt
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI runtime source requires read committed retry';END IF;
 IF p_scope IS NULL OR p_scope NOT IN ('ai_provider_ack_v1','ai_grant_issue_v1','ai_delivery_ack_v1','ai_stop_ack_v1','ai_adopt_v1') OR p_target IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_terms IS NULL OR octet_length(p_terms::text)>262144 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI runtime source envelope';END IF;
 fingerprint:=encode(sha256(convert_to(p_terms::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('ai-runtime',p_scope,p_target::text,p_request::text,fingerprint,'pending','',p_terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NOT NULL THEN RETURN receipt;END IF;
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ai-runtime' AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF receipt.target_ref<>p_target::text OR receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI runtime source request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI runtime source disposition pending';END IF;
 RETURN receipt;
END $$;

CREATE FUNCTION app.bootstrap_ai_slot(p_actor uuid,p_slot uuid,p_expected numeric,p_revision uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; slot app.room_slot; lobby app.room; actors uuid[]; binding_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI bootstrap requires read committed retry';END IF;
 IF p_slot IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI bootstrap envelope';END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'ai_bootstrap_v1',p_slot::text,jsonb_build_object('basis',p_expected::text,'revision',p_revision));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id;
 actors:=ARRAY[lobby.host_id,slot.sponsor_id,p_actor];
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot FOR NO KEY UPDATE;
 IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind<>'ai' OR slot.sponsor_id IS NULL OR lobby.state NOT IN ('lobby','countdown') OR lobby.revision<>p_expected
 OR p_actor NOT IN (slot.sponsor_id,lobby.host_id) OR EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state<>'active')
 OR EXISTS(SELECT 1 FROM app.ai_runner_binding WHERE room_slot_id=p_slot AND desired_state='running') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('slot','stale or unavailable'));END IF;
 IF p_revision IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.mode=lobby.mode AND d.format_id=lobby.format_id AND (d.source_kind='system_pool' OR (d.source_kind='account' AND d.account_id=slot.sponsor_id)) FOR SHARE OF r,d) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable'));END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','ai-bootstrap-v1','slotId',slot.id,'sponsorId',slot.sponsor_id,'roomId',lobby.id,'configurationId',lobby.configuration_id,'epoch','1','startKey',receipt.id::text) WHERE id=receipt.id;
 INSERT INTO app.ai_runner_binding(room_slot_id,binding_epoch,desired_state,observed_state,bootstrap_receipt_id,epoch_receipt_id)
 VALUES(slot.id,1,'running','pending',receipt.id,receipt.id) RETURNING id INTO binding_id;
 UPDATE app.room_slot SET deck_revision_id=p_revision,ready=false WHERE id=slot.id;
 UPDATE app.room_slot SET ready=false WHERE room_id=lobby.id AND retired_at IS NULL AND controller_kind='human';
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=lobby.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding_id,'epoch','1','startKey',receipt.id));
END $$;

CREATE FUNCTION app.observe_ai_provider(p_binding uuid,p_epoch numeric,p_start uuid,p_outcome text,p_provider_ref text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_start IS NULL OR p_outcome IS NULL OR p_outcome NOT IN ('running','unavailable','unknown') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048
 OR (p_outcome='running' AND (p_provider_ref IS NULL OR length(btrim(p_provider_ref)) NOT BETWEEN 1 AND 2048)) OR (p_outcome<>'running' AND p_provider_ref IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid provider observation';END IF;
 receipt:=app.begin_ai_runtime_command('ai_provider_ack_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'startId',p_start,'outcome',p_outcome,'providerRef',p_provider_ref,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF binding.id IS NULL OR binding.binding_epoch<>p_epoch OR binding.epoch_receipt_id<>p_start OR binding.desired_state<>'running' OR slot.retired_at IS NOT NULL OR lobby.state='closed' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('source','stale'));END IF;
 IF binding.provider_runner_ref IS NOT NULL THEN
 IF p_outcome='running' AND binding.provider_runner_ref=p_provider_ref THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition','already observed'));END IF;
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('provider','conflicting observation'));END IF;
 UPDATE app.ai_runner_binding SET provider_runner_ref=p_provider_ref,observed_state=p_outcome,provider_observation_receipt_id=receipt.id WHERE id=binding.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'disposition',p_outcome));
END $$;

-- First provider acknowledgment supplies the real process identity for an already-persisted epoch.
CREATE OR REPLACE FUNCTION app.guard_controller_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller history cannot be erased';END IF;
 IF TG_TABLE_NAME='controller_grant' THEN
 IF TG_OP='INSERT' THEN IF NEW.state<>'active' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller grant requires original active authority';END IF;
 ELSIF (NEW.id,NEW.binding_id,NEW.binding_epoch,NEW.match_seat_id,NEW.grant_secret_ref,NEW.issue_receipt_id) IS DISTINCT FROM (OLD.id,OLD.binding_id,OLD.binding_epoch,OLD.match_seat_id,OLD.grant_secret_ref,OLD.issue_receipt_id)
 OR (NEW.state IS DISTINCT FROM OLD.state AND NOT(OLD.state='active' AND NEW.state='revoked')) OR NEW.delivery_revision<OLD.delivery_revision OR (OLD.delivery_state='delivered' AND NEW.delivery_state<>'delivered') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller grant history cannot be rewritten';END IF;
 ELSE
 IF (NEW.id,NEW.room_slot_id,NEW.bootstrap_receipt_id) IS DISTINCT FROM (OLD.id,OLD.room_slot_id,OLD.bootstrap_receipt_id) OR NEW.binding_epoch<OLD.binding_epoch OR (OLD.match_seat_id IS NOT NULL AND NEW.match_seat_id IS DISTINCT FROM OLD.match_seat_id)
 OR ((NEW.match_seat_id,NEW.provider_runner_ref) IS DISTINCT FROM (OLD.match_seat_id,OLD.provider_runner_ref) AND NEW.binding_epoch<=OLD.binding_epoch
 AND NOT(NEW.match_seat_id IS NOT DISTINCT FROM OLD.match_seat_id AND OLD.provider_runner_ref IS NULL AND NEW.provider_runner_ref IS NOT NULL AND OLD.bootstrap_receipt_id IS NOT NULL AND NEW.provider_observation_receipt_id IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='controller binding identity and epoch cannot be rewritten';END IF;
 END IF;RETURN NEW;
END $$;

CREATE FUNCTION app.issue_ai_grant(p_binding uuid,p_epoch numeric,p_secret_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room; grant_id uuid;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_secret_ref IS NULL OR length(btrim(p_secret_ref)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid grant issue envelope';END IF;
 receipt:=app.begin_ai_runtime_command('ai_grant_issue_v1',p_binding,p_request,jsonb_build_object('bindingId',p_binding,'epoch',p_epoch::text,'secretRef',p_secret_ref));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id WHERE b.id=p_binding;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sponsor','unavailable'));END IF;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding FOR NO KEY UPDATE;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL OR binding.binding_epoch<>p_epoch OR binding.desired_state<>'running' OR binding.observed_state NOT IN ('running','ready') OR binding.provider_runner_ref IS NULL OR slot.retired_at IS NOT NULL OR lobby.state='closed'
 OR EXISTS(SELECT 1 FROM app.controller_grant WHERE binding_id=binding.id AND binding_epoch=p_epoch AND state='active') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('binding','stale or unavailable'));END IF;
 INSERT INTO app.controller_grant(binding_id,binding_epoch,match_seat_id,grant_secret_ref,state,issue_receipt_id) VALUES(binding.id,p_epoch,binding.match_seat_id,p_secret_ref,'active',receipt.id) RETURNING id INTO grant_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',grant_id,'delivery','pending'));
END $$;

CREATE FUNCTION app.observe_ai_grant_delivery(p_grant uuid,p_expected numeric,p_outcome text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; grant_row app.controller_grant; binding app.ai_runner_binding; slot app.room_slot;
BEGIN
 IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_outcome IS NULL OR p_outcome NOT IN ('unknown','delivered') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid grant delivery observation';END IF;
 receipt:=app.begin_ai_runtime_command('ai_delivery_ack_v1',p_grant,p_request,jsonb_build_object('grantId',p_grant,'basis',p_expected::text,'outcome',p_outcome,'proofRef',p_proof));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT s.* INTO slot FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id WHERE g.id=p_grant;
 PERFORM id FROM app.account WHERE id=slot.sponsor_id FOR NO KEY UPDATE;
 PERFORM id FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT b.* INTO binding FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id WHERE g.id=p_grant FOR NO KEY UPDATE OF b;
 SELECT * INTO grant_row FROM app.controller_grant WHERE id=p_grant FOR NO KEY UPDATE;
 IF grant_row.id IS NULL OR grant_row.issue_receipt_id IS NULL OR grant_row.state<>'active' OR binding.desired_state<>'running' OR grant_row.binding_epoch<>binding.binding_epoch OR grant_row.delivery_revision<>p_expected OR grant_row.delivery_state='delivered' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('grant','stale or unavailable'));END IF;
 UPDATE app.controller_grant SET delivery_state=p_outcome,delivery_revision=delivery_revision+1,delivery_receipt_id=receipt.id WHERE id=p_grant;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_grant,'delivery',p_outcome));
END $$;
REVOKE ALL ON FUNCTION app.begin_ai_runtime_command(text,uuid,uuid,jsonb),app.bootstrap_ai_slot(uuid,uuid,numeric,uuid,uuid),app.observe_ai_provider(uuid,numeric,uuid,text,text,text,uuid),app.issue_ai_grant(uuid,numeric,text,uuid),app.observe_ai_grant_delivery(uuid,numeric,text,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
