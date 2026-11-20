BEGIN;
CREATE OR REPLACE FUNCTION app.check_ai_runtime_source(p_binding uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE binding app.ai_runner_binding; source app.command_receipt; observation app.command_receipt; grant_row app.controller_grant;
BEGIN
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_binding;
 IF binding.id IS NULL OR binding.bootstrap_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.bootstrap_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'ai_bootstrap_v1' OR source.normalized_source->>'version' IS DISTINCT FROM 'ai-bootstrap-v1'
 OR source.normalized_source->>'sponsorId' IS DISTINCT FROM (SELECT sponsor_id::text FROM app.room_slot WHERE id=binding.room_slot_id) OR source.normalized_source->>'roomId' IS DISTINCT FROM (SELECT room_id::text FROM app.room_slot WHERE id=binding.room_slot_id)
 OR source.normalized_source->>'slotId' IS DISTINCT FROM binding.room_slot_id::text OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM binding.id::text
 OR source.normalized_source->>'startKey' IS DISTINCT FROM source.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner lacks original accepted bootstrap source';END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.epoch_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR NOT((source.scope='ai_bootstrap_v1' AND source.normalized_source->>'epoch'=binding.binding_epoch::text AND source.id=binding.bootstrap_receipt_id)
 OR (source.scope='ai_adopt_v1' AND source.normalized_source->>'newEpoch'=binding.binding_epoch::text AND source.normalized_source->>'bindingId'=binding.id::text AND source.normalized_source->>'seatId'=binding.match_seat_id::text)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner epoch lacks accepted source';END IF;
 IF source.scope='ai_adopt_v1' AND NOT EXISTS(
 SELECT 1 FROM app.command_receipt parent JOIN app.match_seat seat ON seat.id=binding.match_seat_id JOIN app.match m ON m.id=seat.match_id
 WHERE parent.id::text=source.normalized_source->>'startId' AND parent.state='accepted' AND parent.scope='room_start_v1' AND parent.principal_key='room-start:'||m.room_id::text AND parent.target_ref=m.room_id::text
 AND parent.response_payload->'data'->>'resourceId'=m.id::text AND source.normalized_source->>'roomId'=m.room_id::text AND source.normalized_source->>'matchId'=m.id::text
 ) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI adoption lacks its exact accepted room-start source';END IF;
 IF binding.provider_runner_ref IS NOT NULL THEN
 SELECT * INTO observation FROM app.command_receipt WHERE id=binding.provider_observation_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_provider_ack_v1' OR observation.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text
 OR observation.normalized_source->>'outcome' IS DISTINCT FROM 'running' OR observation.normalized_source->>'providerRef' IS DISTINCT FROM binding.provider_runner_ref
 OR observation.normalized_source->>'startId' IS DISTINCT FROM binding.bootstrap_receipt_id::text OR observation.normalized_source->>'epoch' IS DISTINCT FROM '1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='provider identity lacks accepted process observation';END IF;
 END IF;
 FOR grant_row IN SELECT * FROM app.controller_grant WHERE binding_id=binding.id LOOP
 SELECT * INTO source FROM app.command_receipt WHERE id=grant_row.issue_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope NOT IN ('ai_grant_issue_v1','ai_adopt_v1') OR source.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text
 OR (source.scope='ai_grant_issue_v1' AND (source.normalized_source->>'epoch' IS DISTINCT FROM grant_row.binding_epoch::text OR source.normalized_source->>'secretRef' IS DISTINCT FROM grant_row.grant_secret_ref OR source.response_payload->'data'->>'resourceId' IS DISTINCT FROM grant_row.id::text))
 OR (source.scope='ai_adopt_v1' AND (source.normalized_source->>'newEpoch' IS DISTINCT FROM grant_row.binding_epoch::text OR source.normalized_source->>'seatId' IS DISTINCT FROM grant_row.match_seat_id::text OR source.response_payload->'data'->>'grantId' IS DISTINCT FROM grant_row.id::text OR grant_row.grant_secret_ref IS DISTINCT FROM 'controller-secret:'||grant_row.id::text)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant lacks accepted issue source';END IF;
 IF grant_row.delivery_state='pending' THEN
 IF grant_row.delivery_receipt_id IS NOT NULL OR grant_row.delivery_revision<>0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending grant has invented delivery history';END IF;
 ELSE
 SELECT * INTO observation FROM app.command_receipt WHERE id=grant_row.delivery_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_delivery_ack_v1' OR observation.normalized_source->>'grantId' IS DISTINCT FROM grant_row.id::text
 OR observation.normalized_source->>'outcome' IS DISTINCT FROM grant_row.delivery_state OR observation.normalized_source->>'basis' IS DISTINCT FROM (grant_row.delivery_revision-1)::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant delivery lacks exact accepted observation';END IF;
 END IF;
 END LOOP;
 IF binding.desired_state='stopped' THEN
 SELECT * INTO source FROM app.command_receipt WHERE id=binding.stop_receipt_id;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'ai_stop_request_v1' OR NOT EXISTS(SELECT 1 FROM app.command_receipt cause WHERE cause.id::text=source.normalized_source->>'causeId' AND cause.state='accepted') OR source.normalized_source->>'version' IS DISTINCT FROM 'ai-stop-v1' OR source.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text OR source.normalized_source->>'epoch' IS DISTINCT FROM binding.binding_epoch::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='runner stop requires durable source work';END IF;
 END IF;
 IF binding.observed_state='stopped' THEN
 SELECT * INTO observation FROM app.command_receipt WHERE id=binding.stop_observation_receipt_id;
 IF observation.state IS DISTINCT FROM 'accepted' OR observation.scope IS DISTINCT FROM 'ai_stop_ack_v1' OR observation.normalized_source->>'stopId' IS DISTINCT FROM binding.stop_receipt_id::text
 OR observation.normalized_source->>'epoch' IS DISTINCT FROM binding.binding_epoch::text OR observation.normalized_source->>'bindingId' IS DISTINCT FROM binding.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='observed stop requires exact normalized acknowledgment';END IF;
 END IF;
END $$;
COMMIT;
