BEGIN;
-- Deletion stops new admission, not authority needed to finish an already
-- committed match. This never authorizes a new pregame bootstrap or sponsor.
CREATE FUNCTION app.ai_sponsor_continuing_match(p_binding uuid,p_sponsor uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.ai_runner_binding b JOIN app.match_seat seat ON seat.id=b.match_seat_id JOIN app.match m ON m.id=seat.match_id JOIN app.account a ON a.id=seat.sponsor_id
 WHERE b.id=p_binding AND b.bootstrap_receipt_id IS NOT NULL AND seat.sponsor_id=p_sponsor AND a.state='deletion_pending' AND m.accepted_release_id IS NOT NULL AND m.state IN ('ready','active','between_games') AND m.started_at IS NOT NULL AND m.started_at<=a.deletion_requested_at)
$$;
REVOKE ALL ON FUNCTION app.ai_sponsor_continuing_match(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.issue_ai_grant(p_binding uuid,p_epoch numeric,p_secret_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room; grant_id uuid;
BEGIN
 IF p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_secret_ref IS NULL OR length(btrim(p_secret_ref)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid grant issue envelope';END IF;
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

CREATE OR REPLACE FUNCTION app.commit_ai_ready_confirmation(p_grant uuid,p_epoch numeric,p_revision uuid,p_qualification text,p_authentication text,p_configuration uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; binding app.ai_runner_binding; slot app.room_slot; lobby app.room;
 original_sponsor uuid; source_binding uuid; source_slot uuid; fingerprint text; changed boolean; accepted jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI ready confirmation requires read committed retry'; END IF;
 IF p_configuration IS NULL OR p_grant IS NULL OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_revision IS NULL
 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 OR p_authentication IS NULL OR length(btrim(p_authentication)) NOT BETWEEN 1 AND 2048
 OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid authenticated AI ready envelope'; END IF;
 SELECT b.id,b.room_slot_id,s.sponsor_id INTO source_binding,source_slot,original_sponsor FROM app.controller_grant g JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot s ON s.id=b.room_slot_id WHERE g.id=p_grant;
 IF source_binding IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='AI controller grant unavailable'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('epoch',p_epoch::text,'revision',p_revision,'qualification',p_qualification,'authentication',p_authentication,'configuration',p_configuration)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(controller_grant_id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES(p_grant,'controller:'||p_grant::text,'ai_ready_v1',source_binding::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='controller:'||p_grant::text AND scope='ai_ready_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.target_ref<>source_binding::text OR receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ready source terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI ready disposition pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 PERFORM id FROM app.account WHERE id=original_sponsor AND (state='active' OR app.ai_sponsor_continuing_match(source_binding,original_sponsor)) FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('sponsor','unavailable')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=source_slot;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=source_slot FOR NO KEY UPDATE;
 IF slot.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('slot','retired or unavailable')); END IF;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=source_binding FOR NO KEY UPDATE;
 IF slot.sponsor_id IS DISTINCT FROM original_sponsor OR binding.room_slot_id<>slot.id THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI sponsor or slot changed before fence'; END IF;
 IF lobby.configuration_id IS DISTINCT FROM p_configuration THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','stale')); END IF;
 IF binding.binding_epoch<>p_epoch OR binding.desired_state<>'running' OR binding.observed_state NOT IN ('running','ready') OR binding.provider_runner_ref IS NULL
  OR (lobby.state NOT IN ('lobby','countdown') AND NOT(lobby.state='started' AND EXISTS(SELECT 1 FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id WHERE seat.id=binding.match_seat_id AND m.room_id=lobby.id AND m.state IN ('ready','active','between_games'))))
  OR NOT EXISTS(SELECT 1 FROM app.controller_grant WHERE id=p_grant AND binding_id=binding.id AND binding_epoch=p_epoch AND match_seat_id IS NOT DISTINCT FROM binding.match_seat_id AND state='active' AND (issue_receipt_id IS NULL OR delivery_state='delivered') FOR SHARE) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('controller','stale or unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.mode=lobby.mode AND d.format_id=lobby.format_id
   AND (d.source_kind='system_pool' OR d.account_id=original_sponsor) FOR SHARE OF r,d) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable')); END IF;
 IF lobby.state='started' AND slot.deck_revision_id IS DISTINCT FROM p_revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','accepted selection is immutable')); END IF;
 changed:=slot.deck_revision_id IS DISTINCT FROM p_revision;
 UPDATE app.ai_runner_binding SET observed_state='ready',ready_configuration_id=p_configuration,ready_epoch=p_epoch,ready_deck_revision_id=p_revision,ready_qualification_ref=p_qualification,ready_authentication_ref=p_authentication WHERE id=binding.id;
 UPDATE app.room_slot SET deck_revision_id=p_revision,ready=true WHERE id=slot.id;
 IF lobby.state IN ('lobby','countdown') THEN
  IF changed THEN UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=lobby.id AND controller_kind='human';UPDATE app.room SET state='lobby',countdown_at=NULL WHERE id=lobby.id; END IF;
  UPDATE app.room SET revision=revision+1 WHERE id=lobby.id;
 END IF;
 accepted:=app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',binding.id,'epoch',p_epoch::text,'revisionId',p_revision,'qualificationRef',p_qualification,'authenticationRef',p_authentication,'configurationId',p_configuration));
 IF lobby.state IN ('lobby','countdown') THEN PERFORM app.refresh_room_countdown(lobby.id,false); END IF;
 RETURN accepted;
END $$;
COMMIT;
