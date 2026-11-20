BEGIN;

-- Full observed settings are the compare-and-set baseline. Content revisions
-- remain independent; changing settings does not claim format admission.
CREATE FUNCTION app.valid_deck_settings(p_settings jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE appearance jsonb; key text; value uuid;
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
 IF value='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN false; END IF;
 appearance:=p_settings->'appearance';
 IF jsonb_typeof(appearance) IS DISTINCT FROM 'object' OR
 (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(appearance) k)
 IS DISTINCT FROM ARRAY['backId','diceId','playmatId','sleeveId']::text[] THEN RETURN false; END IF;
 FOREACH key IN ARRAY ARRAY['backId','sleeveId','diceId','playmatId'] LOOP
  IF key='playmatId' AND appearance->key='null'::jsonb THEN CONTINUE; END IF;
  IF jsonb_typeof(appearance->key) IS DISTINCT FROM 'string' THEN RETURN false; END IF;
  value:=(appearance->>key)::uuid;
  IF value='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN false; END IF;
 END LOOP;
 RETURN true;
EXCEPTION WHEN invalid_text_representation THEN RETURN false;
END $$;
REVOKE ALL ON FUNCTION app.valid_deck_settings(jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.change_deck_settings_command(p_subject uuid,p_session uuid,p_request uuid,p_deck uuid,p_expected jsonb,p_settings jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; saved app.deck; observed jsonb; appearance jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR p_deck IS NULL OR p_deck='00000000-0000-0000-0000-000000000000'::uuid
 OR NOT app.valid_deck_settings(p_expected) OR NOT app.valid_deck_settings(p_settings)
 OR octet_length(jsonb_build_object('expected',p_expected,'settings',p_settings)::text)>4096 THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'deck_settings',p_deck::text,
  jsonb_build_object('expected',p_expected,'settings',p_settings));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state<>'active' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO saved FROM app.deck WHERE id=p_deck AND account_id=player.id AND source_kind='account' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','deck unavailable')); END IF;
 observed:=jsonb_build_object('name',saved.name,'mode',saved.mode,'formatId',saved.format_id,
  'appearance',jsonb_build_object('backId',saved.back_id,'sleeveId',saved.sleeve_id,'diceId',saved.dice_id,'playmatId',saved.playmat_id));
 IF observed IS DISTINCT FROM p_expected THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('settings','deck settings changed; reload before saving')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.format WHERE id=(p_settings->>'formatId')::uuid) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','invalid deck metadata')); END IF;
 appearance:=p_settings->'appearance';
 IF NOT app.has_cosmetic(player.id,(appearance->>'backId')::uuid,'back')
 OR NOT app.has_cosmetic(player.id,(appearance->>'sleeveId')::uuid,'sleeve')
 OR NOT app.has_cosmetic(player.id,(appearance->>'diceId')::uuid,'dice')
 OR (appearance->>'playmatId' IS NOT NULL AND NOT app.has_cosmetic(player.id,(appearance->>'playmatId')::uuid,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','cosmetic selection unavailable')); END IF;
 UPDATE app.deck SET name=btrim(p_settings->>'name'),mode=p_settings->>'mode',format_id=(p_settings->>'formatId')::uuid,
  back_id=(appearance->>'backId')::uuid,sleeve_id=(appearance->>'sleeveId')::uuid,
  dice_id=(appearance->>'diceId')::uuid,playmat_id=(appearance->>'playmatId')::uuid WHERE id=p_deck;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_deck));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
REVOKE ALL ON FUNCTION app.change_deck_settings_command(uuid,uuid,uuid,uuid,jsonb,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.change_deck_settings_command(uuid,uuid,uuid,uuid,jsonb,jsonb) TO service_role;

CREATE OR REPLACE FUNCTION app.read_deck_command(p_subject uuid,p_session uuid,p_kind text,p_command jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; request_id uuid;
 scope_name text; target text; terms jsonb; format_id uuid; deck_id uuid; expected uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF jsonb_typeof(p_command) IS DISTINCT FROM 'object' OR octet_length(p_command::text)>264192
 OR jsonb_typeof(p_command->'requestId') IS DISTINCT FROM 'string' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 request_id:=(p_command->>'requestId')::uuid;
 IF request_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 IF p_kind='create' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['formatId','mode','name','requestId']::text[]
  OR jsonb_typeof(p_command->'name') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'mode') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'formatId') IS DISTINCT FROM 'string' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  format_id:=(p_command->>'formatId')::uuid;
  scope_name:='deck_create'; target:=player.id::text;
  terms:=jsonb_build_object('name',p_command->>'name','mode',p_command->>'mode','format',format_id);
 ELSIF p_kind='save' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['deckId','entries','expectedRevisionId','requestId']::text[]
  OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'entries') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_command->'expectedRevisionId') NOT IN ('string','null') THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  deck_id:=(p_command->>'deckId')::uuid; expected:=(p_command->>'expectedRevisionId')::uuid;
  scope_name:='deck_save'; target:=deck_id::text;
  terms:=jsonb_build_object('expectedRevision',expected,'entries',p_command->'entries');
 ELSIF p_kind='settings' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['deckId','expected','requestId','settings']::text[]
  OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
  OR NOT app.valid_deck_settings(p_command->'expected') OR NOT app.valid_deck_settings(p_command->'settings')
  OR octet_length(p_command::text)>4096 THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  deck_id:=(p_command->>'deckId')::uuid;
  IF deck_id='00000000-0000-0000-0000-000000000000'::uuid THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  scope_name:='deck_settings'; target:=deck_id::text;
  terms:=jsonb_build_object('expected',p_command->'expected','settings',p_command->'settings');
 ELSE
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope=scope_name AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM target
  OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to(terms::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));
 END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,scope_name,request_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_text_representation THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
END $$;

COMMIT;
