BEGIN;
CREATE FUNCTION app.valid_deck_import_input(p_input jsonb) RETURNS boolean LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>264192 THEN RETURN false;END IF;
 RETURN coalesce( (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_input) key)=ARRAY['entries','formatId','mode','name','requestId']::text[]
 AND jsonb_typeof(p_input->'requestId')='string' AND p_input->>'requestId' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 AND p_input->>'requestId'<>'00000000-0000-0000-0000-000000000000'
 AND jsonb_typeof(p_input->'formatId')='string' AND p_input->>'formatId' ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 AND p_input->>'formatId'<>'00000000-0000-0000-0000-000000000000'
 AND jsonb_typeof(p_input->'name')='string' AND length(btrim(p_input->>'name')) BETWEEN 1 AND 120
 AND jsonb_typeof(p_input->'mode')='string' AND p_input->>'mode' IN('open','collection')
 AND jsonb_typeof(p_input->'entries')='array',false);
END $$;
CREATE FUNCTION app.import_deck_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF NOT app.valid_deck_import_input(p_input) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.import_deck(player.id,p_input->>'name',p_input->>'mode',(p_input->>'formatId')::uuid,p_input->'entries',(p_input->>'requestId')::uuid);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_deck_import_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;terms jsonb;request_id uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF NOT app.valid_deck_import_input(p_input) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(p_input->>'requestId')::uuid;
 terms:=jsonb_build_object('name',p_input->>'name','mode',p_input->>'mode','format',(p_input->>'formatId')::uuid,'entries',p_input->'entries');
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='deck_import' AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM player.id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'deck_import',request_id));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.valid_deck_import_input(jsonb),app.import_deck_command(uuid,uuid,jsonb),app.read_deck_import_command(uuid,uuid,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.import_deck_command(uuid,uuid,jsonb),app.read_deck_import_command(uuid,uuid,jsonb) TO service_role;
COMMIT;
