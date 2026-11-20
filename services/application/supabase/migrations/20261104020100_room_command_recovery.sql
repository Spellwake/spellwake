BEGIN;
-- Recover the original outer receipt even after leaving, closing or canceling a claimed room.
-- Numeric JSON in fingerprints is intentional: wire quantities are strings, canonical terms are numeric.
CREATE FUNCTION app.read_room_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; body jsonb; kind text; keys text[]; request_id uuid; target uuid;
 scope_name text; terms jsonb; receipt app.command_receipt; expected numeric; revision_id uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>32768 THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k) IS DISTINCT FROM ARRAY['command','kind']::text[]
 OR jsonb_typeof(p_input->'kind') IS DISTINCT FROM 'string' OR jsonb_typeof(p_input->'command') IS DISTINCT FROM 'object' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 kind:=p_input->>'kind';body:=p_input->'command';
 SELECT array_agg(k ORDER BY k) INTO keys FROM jsonb_object_keys(body) k;
 IF jsonb_typeof(body->'requestId') IS DISTINCT FROM 'string' OR body->>'requestId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(body->>'requestId')::uuid;
 IF kind='create' THEN
  IF keys IS DISTINCT FROM ARRAY['competitive','formatId','mode','profileId','requestId','visibility']::text[]
  OR jsonb_typeof(body->'formatId') IS DISTINCT FROM 'string' OR body->>'formatId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  OR jsonb_typeof(body->'profileId') IS DISTINCT FROM 'string' OR body->>'profileId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  OR jsonb_typeof(body->'mode') IS DISTINCT FROM 'string' OR body->>'mode' NOT IN('open','collection')
  OR jsonb_typeof(body->'competitive') IS DISTINCT FROM 'boolean'
  OR jsonb_typeof(body->'visibility') IS DISTINCT FROM 'string' OR body->>'visibility' NOT IN('public','private')
  OR ((body->>'competitive')::boolean AND body->>'mode'<>'collection')
  OR '00000000-0000-0000-0000-000000000000'::uuid IN((body->>'formatId')::uuid,(body->>'profileId')::uuid) THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
  target:=player.id;scope_name:='room_create';
  terms:=jsonb_build_object('format',(body->>'formatId')::uuid,'profile',(body->>'profileId')::uuid,'mode',body->>'mode','competitive',(body->>'competitive')::boolean,'visibility',body->>'visibility');
 ELSIF kind IN('select_deck','leave') THEN
  IF jsonb_typeof(body->'roomId') IS DISTINCT FROM 'string' OR body->>'roomId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  OR jsonb_typeof(body->'expectedRevision') IS DISTINCT FROM 'string' OR body->>'expectedRevision' !~ '^(0|[1-9][0-9]{0,77})$' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
  target:=(body->>'roomId')::uuid;expected:=(body->>'expectedRevision')::numeric;
  IF kind='select_deck' THEN
   IF keys IS DISTINCT FROM ARRAY['deckRevisionId','expectedRevision','requestId','roomId']::text[]
   OR jsonb_typeof(body->'deckRevisionId') NOT IN('null','string') THEN
    RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
   IF jsonb_typeof(body->'deckRevisionId')='string' THEN
    IF body->>'deckRevisionId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
     RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
    revision_id:=(body->>'deckRevisionId')::uuid;
    IF revision_id='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
   END IF;
   scope_name:='room_deck';terms:=jsonb_build_object('room',target,'expected',expected,'revision',revision_id);
  ELSE
   IF keys IS DISTINCT FROM ARRAY['expectedRevision','requestId','roomId']::text[] THEN
    RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
   scope_name:='room_leave';terms:=jsonb_build_object('room',target,'expected',expected);
  END IF;
 ELSE RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN(request_id,target) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope=scope_name AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM target::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,scope_name,request_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.read_room_command(uuid,uuid,jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_room_command(uuid,uuid,jsonb) TO service_role;
COMMIT;
