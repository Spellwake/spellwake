-- Session-bound prediction commitments reuse canonical matching and exact account receipts.
BEGIN;
CREATE FUNCTION app.prediction_command_terms(p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE command jsonb; kind text; keys text[]; request_id uuid; target uuid; terms jsonb; scope_name text;
BEGIN
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>4096
 OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k) IS DISTINCT FROM ARRAY['command','kind']::text[]
 OR jsonb_typeof(p_input->'kind') IS DISTINCT FROM 'string' OR jsonb_typeof(p_input->'command') IS DISTINCT FROM 'object' THEN RETURN NULL; END IF;
 kind:=p_input->>'kind'; command:=p_input->'command';
 SELECT array_agg(k ORDER BY k) INTO keys FROM jsonb_object_keys(command) k;
 IF jsonb_typeof(command->'requestId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
 request_id:=(command->>'requestId')::uuid;
 IF kind='place_order' THEN
  IF keys IS DISTINCT FROM ARRAY['limitCost','marketId','quantity','requestId','side']::text[]
  OR jsonb_typeof(command->'marketId') IS DISTINCT FROM 'string' OR jsonb_typeof(command->'side') IS DISTINCT FROM 'string'
  OR command->>'side' NOT IN ('yes','no') OR jsonb_typeof(command->'limitCost') IS DISTINCT FROM 'string'
  OR command->>'limitCost' !~ '^[1-9][0-9]?$' OR jsonb_typeof(command->'quantity') IS DISTINCT FROM 'string'
  OR command->>'quantity' !~ '^[1-9][0-9]{0,77}$' THEN RETURN NULL; END IF;
  IF (command->>'limitCost')::numeric*(command->>'quantity')::numeric>=power(10::numeric,78) THEN RETURN NULL; END IF;
  target:=(command->>'marketId')::uuid; scope_name:='prediction_order';
  terms:=jsonb_build_object('side',command->>'side','limit',command->>'limitCost','quantity',command->>'quantity');
 ELSIF kind='cancel_order' THEN
  IF keys IS DISTINCT FROM ARRAY['orderId','requestId']::text[] OR jsonb_typeof(command->'orderId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
  target:=(command->>'orderId')::uuid; scope_name:='prediction_cancel'; terms:='{}'::jsonb;
 ELSE RETURN NULL; END IF;
 IF request_id='00000000-0000-0000-0000-000000000000'::uuid OR target='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('requestId',request_id,'scope',scope_name,'targetId',target,'terms',terms);
EXCEPTION WHEN invalid_text_representation OR invalid_parameter_value OR numeric_value_out_of_range THEN RETURN NULL;
END $$;
CREATE FUNCTION app.read_session_command(p_subject uuid,p_session uuid,p_parsed jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; player app.account; receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='session command requires read committed'; END IF;
 parsed:=p_parsed;
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 player:=app.lock_session_account(p_subject,p_session);
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope=parsed->>'scope' AND request_ref=parsed->>'requestId';
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM parsed->>'targetId'
 OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to((parsed->'terms')::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,parsed->>'scope',(parsed->>'requestId')::uuid));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.execute_prediction_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; actor_id uuid; market_id uuid; target uuid; request_id uuid; player app.account; command jsonb; result jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='prediction session command requires read committed'; END IF;
 parsed:=app.prediction_command_terms(p_input);
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 target:=(parsed->>'targetId')::uuid; request_id:=(parsed->>'requestId')::uuid; command:=p_input->'command';
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified prediction account required'; END IF;
 IF p_input->>'kind'='place_order' THEN SELECT id INTO market_id FROM app.prediction_market WHERE id=target;
 ELSE SELECT o.market_id INTO market_id FROM app.prediction_order o WHERE o.id=target AND o.account_id=actor_id; END IF;
 -- Match the scheduler and canonical writer: all participants before market/hold/inventory.
 -- A changed participant snapshot retries rather than reversing account lock order.
 IF market_id IS NOT NULL THEN PERFORM app.lock_prediction_market(market_id,actor_id);
 ELSE PERFORM id FROM app.account WHERE id=actor_id FOR NO KEY UPDATE; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified prediction account changed'; END IF;
 IF p_input->>'kind'='place_order' THEN
  result:=app.place_prediction_order(actor_id,target,command->>'side',command->>'limitCost',command->>'quantity',request_id);
 ELSE result:=app.cancel_prediction_order(actor_id,target,request_id); END IF;
 -- Roll back canonical effects and receipt if Auth changed while the writer waited.
 -- This is not serialization of all managed Auth changes through COMMIT.
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified prediction account changed'; END IF;
 RETURN result;
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
CREATE OR REPLACE FUNCTION app.read_economy_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$
 SELECT app.read_session_command(p_subject,p_session,app.economy_command_terms(p_input))
$$;
CREATE FUNCTION app.read_prediction_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$
 SELECT app.read_session_command(p_subject,p_session,app.prediction_command_terms(p_input))
$$;
REVOKE ALL ON FUNCTION app.prediction_command_terms(jsonb),app.read_session_command(uuid,uuid,jsonb),
 app.execute_prediction_command(uuid,uuid,jsonb),app.read_prediction_command(uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.execute_prediction_command(uuid,uuid,jsonb),app.read_prediction_command(uuid,uuid,jsonb) TO service_role;
REVOKE EXECUTE ON FUNCTION app.place_prediction_order(uuid,uuid,text,text,text,uuid),app.cancel_prediction_order(uuid,uuid,uuid) FROM service_role;
COMMIT;
