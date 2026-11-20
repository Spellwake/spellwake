-- Bind existing social writers to live sessions without reversing pair lock order.
BEGIN;

CREATE FUNCTION app.social_command_terms(p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE kind text; command jsonb; request_id uuid; target uuid; terms jsonb; scope_name text:='social'; keys text[];
BEGIN
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>32768 THEN RETURN NULL; END IF;
 IF (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k) IS DISTINCT FROM ARRAY['command','kind']::text[]
 OR jsonb_typeof(p_input->'kind') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p_input->'command') IS DISTINCT FROM 'object' THEN RETURN NULL; END IF;
 kind:=p_input->>'kind'; command:=p_input->'command';
 SELECT array_agg(k ORDER BY k) INTO keys FROM jsonb_object_keys(command) k;
 IF jsonb_typeof(command->'requestId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
 request_id:=(command->>'requestId')::uuid;
 IF request_id='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN NULL; END IF;
 IF kind='send_dm' THEN
  IF keys IS DISTINCT FROM ARRAY['body','conversationId','requestId']::text[]
  OR jsonb_typeof(command->'conversationId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'body') IS DISTINCT FROM 'string'
  OR length(command->>'body') NOT BETWEEN 1 AND 4000 OR command->>'body' !~ '[^[:space:]]' THEN RETURN NULL; END IF;
  target:=(command->>'conversationId')::uuid; scope_name:='chat';
  terms:=jsonb_build_object('action','send_dm','body',command->>'body');
 ELSE
  IF jsonb_typeof(command->'otherAccountId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
  target:=(command->>'otherAccountId')::uuid;
  IF kind='friendship' THEN
   IF keys IS DISTINCT FROM ARRAY['action','otherAccountId','requestId']::text[]
   OR jsonb_typeof(command->'action') IS DISTINCT FROM 'string'
   OR command->>'action' NOT IN ('request','accept','remove') THEN RETURN NULL; END IF;
   terms:=jsonb_build_object('action',command->>'action','other',target::text);
  ELSIF kind='block' THEN
   IF keys IS DISTINCT FROM ARRAY['blocked','otherAccountId','requestId']::text[]
   OR jsonb_typeof(command->'blocked') IS DISTINCT FROM 'boolean' THEN RETURN NULL; END IF;
   terms:=jsonb_build_object('action','block','other',target::text,'blocked',command->'blocked');
  ELSIF kind='open_dm' THEN
   IF keys IS DISTINCT FROM ARRAY['otherAccountId','requestId']::text[] THEN RETURN NULL; END IF;
   terms:=jsonb_build_object('action','open_dm','other',target::text);
  ELSE RETURN NULL;
  END IF;
 END IF;
 IF target='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('requestId',request_id,'scope',scope_name,'targetId',target,'terms',terms);
EXCEPTION WHEN invalid_text_representation THEN RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION app.social_command_terms(jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.execute_social_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; actor_id uuid; peer_id uuid; target uuid; request_id uuid; player app.account; command jsonb; kind text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='social session command requires read committed'; END IF;
 parsed:=app.social_command_terms(p_input);
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 target:=(parsed->>'targetId')::uuid; request_id:=(parsed->>'requestId')::uuid;
 kind:=p_input->>'kind'; command:=p_input->'command';
 -- Do not take the actor's account lock before the sorted pair. Reuse the
 -- shared session check only after both accounts have been locked in order.
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified social account required'; END IF;
 IF kind='send_dm' THEN
  SELECT CASE WHEN c.dm_low_account_id=actor_id THEN c.dm_high_account_id
   WHEN c.dm_high_account_id=actor_id THEN c.dm_low_account_id END INTO peer_id
   FROM app.conversation c WHERE c.id=target AND c.kind='dm';
 ELSE peer_id:=target;
 END IF;
 PERFORM id FROM app.account WHERE id IN (actor_id,peer_id) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified social account changed'; END IF;
 -- Canonical writers own contact policy, source receipts, lifecycle and effects.
 CASE kind
 WHEN 'friendship' THEN RETURN app.change_friendship(player.id,target,command->>'action',request_id);
 WHEN 'block' THEN RETURN app.change_block(player.id,target,(command->>'blocked')::boolean,request_id);
 WHEN 'open_dm' THEN RETURN app.open_direct_conversation(player.id,target,request_id);
 WHEN 'send_dm' THEN RETURN app.send_direct_message(player.id,target,command->>'body',request_id);
 END CASE;
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='validated social operation missing';
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

-- Exact disposition does not send a message, open a conversation or change policy.
CREATE FUNCTION app.read_social_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; player app.account; receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='social session command requires read committed'; END IF;
 parsed:=app.social_command_terms(p_input);
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

REVOKE ALL ON FUNCTION app.execute_social_command(uuid,uuid,jsonb),app.read_social_command(uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.execute_social_command(uuid,uuid,jsonb),app.read_social_command(uuid,uuid,jsonb) TO service_role;
-- These remain private implementations used by the session facade, not alternate APIs.
REVOKE EXECUTE ON FUNCTION app.change_friendship(uuid,uuid,text,uuid),app.change_block(uuid,uuid,boolean,uuid),
 app.open_direct_conversation(uuid,uuid,uuid),app.send_direct_message(uuid,uuid,text,uuid) FROM service_role;
COMMIT;
