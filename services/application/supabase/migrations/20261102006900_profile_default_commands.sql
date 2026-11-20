BEGIN;

CREATE FUNCTION app.valid_appearance_selection(p_appearance jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text; value uuid;
BEGIN
 IF jsonb_typeof(p_appearance) IS DISTINCT FROM 'object' OR
 (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_appearance) k)
 IS DISTINCT FROM ARRAY['backId','diceId','playmatId','sleeveId']::text[] THEN RETURN false; END IF;
 FOREACH key IN ARRAY ARRAY['backId','sleeveId','diceId','playmatId'] LOOP
  IF key='playmatId' AND p_appearance->key='null'::jsonb THEN CONTINUE; END IF;
  IF jsonb_typeof(p_appearance->key) IS DISTINCT FROM 'string' THEN RETURN false; END IF;
  value:=(p_appearance->>key)::uuid;
  IF value='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN false; END IF;
 END LOOP;
 RETURN true;
EXCEPTION WHEN invalid_text_representation THEN RETURN false;
END $$;
REVOKE ALL ON FUNCTION app.valid_appearance_selection(jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- The same strict slot shape applies to both existing deck and profile commands.
CREATE OR REPLACE FUNCTION app.valid_deck_settings(p_settings jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE value uuid;
BEGIN
 IF jsonb_typeof(p_settings) IS DISTINCT FROM 'object' OR
 (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_settings) k)
 IS DISTINCT FROM ARRAY['appearance','formatId','mode','name']::text[] THEN RETURN false; END IF;
 IF jsonb_typeof(p_settings->'name') IS DISTINCT FROM 'string'
 OR length(btrim(p_settings->>'name')) NOT BETWEEN 1 AND 120
 OR jsonb_typeof(p_settings->'mode') IS DISTINCT FROM 'string'
 OR p_settings->>'mode' NOT IN ('open','collection')
 OR jsonb_typeof(p_settings->'formatId') IS DISTINCT FROM 'string' THEN RETURN false; END IF;
 value:=(p_settings->>'formatId')::uuid;
 RETURN value<>'00000000-0000-0000-0000-000000000000'::uuid AND app.valid_appearance_selection(p_settings->'appearance');
EXCEPTION WHEN invalid_text_representation THEN RETURN false;
END $$;

-- Account defaults have their own command. They never update existing decks.
CREATE FUNCTION app.change_profile_defaults_command(p_subject uuid,p_session uuid,p_request uuid,p_expected jsonb,p_appearance jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; defaults app.profile; observed jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR NOT app.valid_appearance_selection(p_expected) OR NOT app.valid_appearance_selection(p_appearance) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'profile_defaults',player.id::text,
  jsonb_build_object('expected',p_expected,'appearance',p_appearance));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state<>'active' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO STRICT defaults FROM app.profile WHERE account_id=player.id FOR NO KEY UPDATE;
 observed:=jsonb_build_object('backId',defaults.default_back_id,'sleeveId',defaults.default_sleeve_id,
  'diceId',defaults.default_dice_id,'playmatId',defaults.default_playmat_id);
 IF observed IS DISTINCT FROM p_expected THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('defaults','account defaults changed; reload before saving')); END IF;
 IF NOT app.has_cosmetic(player.id,(p_appearance->>'backId')::uuid,'back')
 OR NOT app.has_cosmetic(player.id,(p_appearance->>'sleeveId')::uuid,'sleeve')
 OR NOT app.has_cosmetic(player.id,(p_appearance->>'diceId')::uuid,'dice')
 OR (p_appearance->>'playmatId' IS NOT NULL AND NOT app.has_cosmetic(player.id,(p_appearance->>'playmatId')::uuid,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','cosmetic selection unavailable')); END IF;
 UPDATE app.profile SET default_back_id=(p_appearance->>'backId')::uuid,default_sleeve_id=(p_appearance->>'sleeveId')::uuid,
  default_dice_id=(p_appearance->>'diceId')::uuid,default_playmat_id=(p_appearance->>'playmatId')::uuid WHERE id=defaults.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',defaults.id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.read_profile_defaults_command(p_subject uuid,p_session uuid,p_command jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; request_id uuid; terms jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF jsonb_typeof(p_command) IS DISTINCT FROM 'object' OR octet_length(p_command::text)>2048 OR
 (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_command) k)
 IS DISTINCT FROM ARRAY['appearance','expected','requestId']::text[]
 OR jsonb_typeof(p_command->'requestId') IS DISTINCT FROM 'string'
 OR NOT app.valid_appearance_selection(p_command->'expected') OR NOT app.valid_appearance_selection(p_command->'appearance') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 request_id:=(p_command->>'requestId')::uuid;
 IF request_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 terms:=jsonb_build_object('expected',p_command->'expected','appearance',p_command->'appearance');
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope='profile_defaults' AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM player.id::text
  OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to(terms::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'profile_defaults',request_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_text_representation THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
END $$;
REVOKE ALL ON FUNCTION app.change_profile_defaults_command(uuid,uuid,uuid,jsonb,jsonb),app.read_profile_defaults_command(uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.change_profile_defaults_command(uuid,uuid,uuid,jsonb,jsonb),app.read_profile_defaults_command(uuid,uuid,jsonb) TO service_role;

-- No client consumed the actor-only prototype. Its test callers now use the
-- authenticated, baseline-bound command, leaving one current defaults writer.
DROP FUNCTION app.change_profile_defaults(uuid,uuid,uuid,uuid,uuid,uuid);

COMMIT;
