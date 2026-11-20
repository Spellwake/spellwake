BEGIN;

-- Receipt recovery is a read of the exact originally submitted command. It
-- never creates a receipt, changes its terms, or guesses from a missing deck.
CREATE FUNCTION app.read_deck_command(p_subject uuid,p_session uuid,p_kind text,p_command jsonb)
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
REVOKE EXECUTE ON FUNCTION app.read_deck_command(uuid,uuid,text,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_deck_command(uuid,uuid,text,jsonb) TO service_role;

COMMIT;
