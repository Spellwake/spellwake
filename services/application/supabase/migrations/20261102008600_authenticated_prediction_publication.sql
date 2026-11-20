-- Qualified publication and original-caller preflight share one evidence check.
BEGIN;
CREATE FUNCTION app.prediction_publication_price(p_printing uuid,p_probe uuid) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE printing app.printing; probe app.command_receipt; evidence jsonb; manifest jsonb; field text;
BEGIN
 SELECT * INTO printing FROM app.printing WHERE id=p_printing AND app.catalogue_source_visible(catalogue_receipt_id);
 IF NOT FOUND THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('printing','printing unavailable')); END IF;
 field:=CASE printing.finish WHEN 'nonfoil' THEN 'usd' WHEN 'foil' THEN 'usd_foil' WHEN 'etched' THEN 'usd_etched' END;
 SELECT * INTO probe FROM app.command_receipt WHERE id=p_probe AND principal_key='collector:scryfall-v1' AND scope='price_probe' AND state='accepted';
 IF NOT FOUND OR field IS NULL THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('price','qualified printing price unavailable'));
 END IF;
 evidence:=probe.result_ref::jsonb;
 manifest:=app.read_price_manifest((evidence->>'manifestId')::uuid);
 IF probe.target_ref<>printing.external_printing_ref OR evidence->>'printingRef'<>printing.external_printing_ref
 OR probe.payload_hash<>encode(sha256(convert_to(evidence::text,'UTF8')),'hex')
 OR manifest->>'state'<>'valid' OR (manifest->>'acquiredAt')::timestamptz>clock_timestamp()
 OR (manifest->>'acquiredAt')::timestamptz<clock_timestamp()-interval '24 hours'
 OR NOT (evidence->'finishes' ? printing.finish) OR jsonb_typeof(evidence->'prices'->field)<>'string'
 OR NOT app.price_decimal_valid(evidence->'prices'->>field) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('price','usable current exact-printing price required'));
 END IF;

 RETURN jsonb_build_object('status','success','data',jsonb_build_object('printingId',printing.id,'probeId',p_probe,
 'priceMetric','scryfall:'||field,'currentUsd',evidence->'prices'->>field,'acquiredAt',(manifest->>'acquiredAt')::timestamptz,
 'usableUntil',(manifest->>'acquiredAt')::timestamptz+interval '24 hours'));
END $$;
CREATE OR REPLACE FUNCTION app.publish_prediction(p_actor uuid,p_printing uuid,p_target text,p_condition text,p_close timestamptz,
 p_start timestamptz,p_deadline timestamptz,p_probe uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE receipt app.command_receipt; evidence jsonb; market app.prediction_market; slots integer; terminal date; result uuid:=gen_random_uuid();
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'prediction_publish',p_printing::text,jsonb_build_object(
 'target',p_target,'condition',p_condition,'tradingClosesAt',p_close,'observationStartsAt',p_start,'deadline',p_deadline,'probeId',p_probe));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));
 END IF;
 IF NOT app.price_decimal_valid(p_target) OR p_target::numeric<=0 OR p_condition IS NULL OR p_condition NOT IN ('reached_by','at_deadline')
 OR p_close IS NULL OR p_start IS NULL OR p_deadline IS NULL OR NOT isfinite(p_close) OR NOT isfinite(p_start) OR NOT isfinite(p_deadline)
 OR p_close<=clock_timestamp() OR p_close>=p_start OR p_start>p_deadline OR p_deadline>p_start+interval '366 days' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','invalid future sampled-price terms'));
 END IF;
 evidence:=app.prediction_publication_price(p_printing,p_probe);
 IF evidence->>'status'<>'success' THEN RETURN app.finish_account_command(receipt.id,false,evidence->'data'); END IF;
 market.observation_starts_at:=p_start; market.deadline:=p_deadline;
 SELECT count(*),max(s.slot) INTO slots,terminal FROM app.prediction_slots(market) s;
 IF slots=0 OR slots>366 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','at least one complete daily acceptance window required'));
 END IF;
 INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,
 observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state,publication_probe_id)
 VALUES(result,p_actor,p_printing,evidence->'data'->>'priceMetric',p_target,p_condition,p_deadline,p_close,p_start,'scryfall-daily-12-13-utc-v1',terminal::text,
 'scryfall-sampled-gte-v1',100,1,'open',p_probe);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',result));
END $$;

ALTER FUNCTION app.prediction_command_terms(jsonb) RENAME TO prediction_order_terms;
CREATE FUNCTION app.prediction_command_terms(p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE command jsonb; request_id uuid; printing uuid; probe uuid; closed timestamptz; started timestamptz; deadline timestamptz; value text;
BEGIN
 IF p_input->>'kind' IS DISTINCT FROM 'publish_market' THEN RETURN app.prediction_order_terms(p_input); END IF;
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>4096
 OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k) IS DISTINCT FROM ARRAY['command','kind']::text[]
 OR jsonb_typeof(p_input->'command') IS DISTINCT FROM 'object' THEN RETURN NULL; END IF;
 command:=p_input->'command';
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(command) k) IS DISTINCT FROM
 ARRAY['condition','deadline','observationStartsAt','printingId','probeId','requestId','targetUsd','tradingClosesAt']::text[] THEN RETURN NULL; END IF;
 FOREACH value IN ARRAY ARRAY['printingId','probeId','requestId','targetUsd','condition','tradingClosesAt','observationStartsAt','deadline'] LOOP
  IF jsonb_typeof(command->value) IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
 END LOOP;
 IF NOT app.price_decimal_valid(command->>'targetUsd') OR (command->>'targetUsd')::numeric<=0
 OR command->>'condition' NOT IN ('reached_by','at_deadline') THEN RETURN NULL; END IF;
 FOREACH value IN ARRAY ARRAY['tradingClosesAt','observationStartsAt','deadline'] LOOP
  IF command->>value !~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,6})?(Z|[+-][0-9]{2}:[0-9]{2})$' THEN RETURN NULL; END IF;
 END LOOP;
 request_id:=(command->>'requestId')::uuid; printing:=(command->>'printingId')::uuid; probe:=(command->>'probeId')::uuid;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN (request_id,printing,probe) THEN RETURN NULL; END IF;
 closed:=(command->>'tradingClosesAt')::timestamptz; started:=(command->>'observationStartsAt')::timestamptz; deadline:=(command->>'deadline')::timestamptz;
 IF closed>=started OR started>deadline OR deadline>started+interval '366 days' THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('requestId',request_id,'scope','prediction_publish','targetId',printing,'terms',jsonb_build_object(
 'target',command->>'targetUsd','condition',command->>'condition','tradingClosesAt',closed,'observationStartsAt',started,'deadline',deadline,'probeId',probe));
EXCEPTION WHEN invalid_text_representation OR invalid_parameter_value OR numeric_value_out_of_range OR datetime_field_overflow OR invalid_datetime_format THEN RETURN NULL;
END $$;
ALTER FUNCTION app.execute_prediction_command(uuid,uuid,jsonb) RENAME TO execute_prediction_order;
CREATE FUNCTION app.execute_prediction_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE parsed jsonb; player app.account; actor_id uuid; command jsonb; result jsonb;
BEGIN
 IF p_input->>'kind' IS DISTINCT FROM 'publish_market' THEN RETURN app.execute_prediction_order(p_subject,p_session,p_input); END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='prediction session command requires read committed'; END IF;
 parsed:=app.prediction_command_terms(p_input);
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 player:=app.lock_session_account(p_subject,p_session); actor_id:=player.id; command:=p_input->'command';
 result:=app.publish_prediction(player.id,(command->>'printingId')::uuid,command->>'targetUsd',command->>'condition',
  (command->>'tradingClosesAt')::timestamptz,(command->>'observationStartsAt')::timestamptz,(command->>'deadline')::timestamptz,
  (command->>'probeId')::uuid,(command->>'requestId')::uuid);
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified prediction account changed'; END IF;
 RETURN result;
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
CREATE OR REPLACE FUNCTION app.read_prediction_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$
 SELECT app.read_session_command(p_subject,p_session,app.prediction_command_terms(p_input))
$$;

CREATE FUNCTION app.read_my_prediction_publication_source(p_printing uuid) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; printing app.printing; candidate uuid; result jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO printing FROM app.printing WHERE id=p_printing AND app.catalogue_source_visible(catalogue_receipt_id);
 IF NOT FOUND THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('printing','unavailable')); END IF;
 -- Each accepted daily manifest contributes at most one probe per exact printing.
 FOR candidate IN SELECT p.id FROM app.command_receipt p JOIN app.command_receipt m ON m.id=(CASE WHEN p.principal_key='collector:scryfall-v1' AND p.scope='price_probe' AND p.state='accepted' THEN p.result_ref::jsonb->>'manifestId' END)::uuid
  WHERE p.principal_key='collector:scryfall-v1' AND p.scope='price_probe' AND p.state='accepted' AND p.target_ref=printing.external_printing_ref
  AND m.principal_key='collector:scryfall-v1' AND m.scope='price_slot' AND m.state='accepted'
  AND m.result_ref::jsonb->>'state'='valid' AND (m.result_ref::jsonb->>'acquiredAt')::timestamptz BETWEEN clock_timestamp()-interval '24 hours' AND clock_timestamp()
  ORDER BY (m.result_ref::jsonb->>'acquiredAt')::timestamptz DESC,p.id LOOP
  result:=app.prediction_publication_price(p_printing,candidate);
  IF result->>'status'='success' THEN RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'source',result->'data')); END IF;
 END LOOP;
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('price','unavailable'));
END $$;
REVOKE ALL ON FUNCTION app.prediction_publication_price(uuid,uuid),app.prediction_order_terms(jsonb),app.prediction_command_terms(jsonb),
 app.execute_prediction_order(uuid,uuid,jsonb),app.execute_prediction_command(uuid,uuid,jsonb),app.read_my_prediction_publication_source(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.execute_prediction_command(uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_my_prediction_publication_source(uuid) TO authenticated;
REVOKE EXECUTE ON FUNCTION app.publish_prediction(uuid,uuid,text,text,timestamptz,timestamptz,timestamptz,uuid,uuid) FROM service_role;
COMMIT;
