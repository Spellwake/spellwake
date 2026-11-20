BEGIN;
-- These elevated ports are for trusted destination adapters only, never human RPCs.
CREATE FUNCTION app.require_handoff_destination(p_destination text,p_handoff uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_destination IS NULL OR p_destination NOT IN('game_host','match_lifecycle','match_settlement') OR p_handoff IS NULL OR p_handoff='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff destination identity';END IF;
 IF NOT EXISTS(SELECT 1 FROM app.durable_handoff WHERE id=p_handoff AND destination=p_destination) THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='handoff destination unavailable';END IF;
END $$;
CREATE FUNCTION app.read_handoff_delivery_work(p_destination text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff cursor';END IF;
 RETURN app.read_pending_handoffs(p_destination,p_after,p_limit);
END $$;
CREATE FUNCTION app.prepare_handoff_delivery(p_destination text,p_handoff uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE response jsonb;receipt app.command_receipt;
BEGIN
 PERFORM app.require_handoff_destination(p_destination,p_handoff);
 response:=app.prepare_handoff(p_handoff);
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='handoff:'||p_handoff::text AND scope='handoff_prepare_v1' AND target_ref=p_handoff::text AND request_ref=p_handoff::text AND payload_hash=app.auth_json_hash(jsonb_build_object('handoffId',p_handoff)) AND state='accepted';
 IF receipt.id IS NULL OR response IS DISTINCT FROM receipt.response_payload THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='handoff preparation source unavailable';END IF;
 PERFORM app.check_handoff_receipt(receipt.id);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('handoffId',p_handoff,'preparationId',receipt.id,'dispatchKey',response->'data'->'dispatchKey','destination',response->'data'->'destination','sourceHash',response->'data'->'sourceHash','manifest',receipt.normalized_source->'manifest'));
END $$;
CREATE FUNCTION app.acknowledge_handoff_delivery(p_destination text,p_handoff uuid,p_preparation uuid,p_outcome text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 PERFORM app.require_handoff_destination(p_destination,p_handoff);
 RETURN app.acknowledge_handoff(p_handoff,p_preparation,p_outcome,p_proof,p_request);
END $$;
CREATE FUNCTION app.read_handoff_delivery_ack(p_destination text,p_handoff uuid,p_preparation uuid,p_outcome text,p_proof text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;input jsonb;
BEGIN
 PERFORM app.require_handoff_destination(p_destination,p_handoff);
 IF p_preparation IS NULL OR p_request IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_preparation,p_request) OR p_outcome IS NULL OR p_outcome NOT IN('unknown','published') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid handoff observation identity';END IF;
 input:=jsonb_build_object('handoffId',p_handoff,'preparationId',p_preparation,'outcome',p_outcome,'proofRef',p_proof);
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='handoff:'||p_handoff::text AND scope='handoff_ack_v1' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_handoff::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(input)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('state',coalesce(receipt.state,'unknown'),'response',receipt.response_payload));
END $$;
REVOKE ALL ON FUNCTION app.require_handoff_destination(text,uuid),app.read_handoff_delivery_work(text,uuid,integer),app.prepare_handoff_delivery(text,uuid),app.acknowledge_handoff_delivery(text,uuid,uuid,text,text,uuid),app.read_handoff_delivery_ack(text,uuid,uuid,text,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_handoff_delivery_work(text,uuid,integer),app.prepare_handoff_delivery(text,uuid),app.acknowledge_handoff_delivery(text,uuid,uuid,text,text,uuid),app.read_handoff_delivery_ack(text,uuid,uuid,text,text,uuid) TO service_role;
COMMIT;
