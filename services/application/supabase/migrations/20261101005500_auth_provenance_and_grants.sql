BEGIN;

-- Application hook/config remains disabled until identity/admission integration
-- and real managed-role qualification. These are the private source-bound writers.
CREATE FUNCTION app.auth_audit_fingerprint(p_id uuid,p_payload jsonb) RETURNS jsonb
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 result:=jsonb_build_object('version',1,'sourceId',p_id::text,
  'actorHash',app.auth_uuid_hash((p_payload->>'actor_id')::uuid),'action',p_payload->>'action',
  'logType',p_payload->>'log_type','provider',p_payload->'traits'->>'provider',
  'providerType',p_payload->'traits'->>'provider_type','xact',pg_current_xact_id()::text);
 IF p_payload->>'action'='user_deleted' THEN
  result:=result||jsonb_build_object('targetHash',app.auth_uuid_hash((p_payload->'traits'->>'user_id')::uuid));
 END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.auth_audit_form(p_payload jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE action text:=p_payload->>'action'; log_type text:=p_payload->>'log_type';
 traits jsonb:=p_payload->'traits'; provider text:=traits->>'provider'; provider_type text:=traits->>'provider_type';
 has_provider boolean:=coalesce(traits?'provider',false); has_type boolean:=coalesce(traits?'provider_type',false);
 candidate text;
BEGIN
 IF jsonb_typeof(p_payload->'actor_id') IS DISTINCT FROM 'string'
 OR (p_payload?'traits' AND jsonb_typeof(traits) IS DISTINCT FROM 'object') THEN RETURN jsonb_build_object('kind','invalid'); END IF;
 IF action IN ('token_refreshed','token_revoked') AND log_type='token' AND NOT has_provider AND NOT has_type THEN
  RETURN jsonb_build_object('kind','refresh');
 ELSIF action='user_deleted' AND log_type='team' AND NOT has_provider AND NOT has_type
 AND jsonb_typeof(traits->'user_id')='string' THEN RETURN jsonb_build_object('kind','cleanup');
 ELSIF (action='login' AND log_type='account') OR (action='user_signedup' AND log_type='team') THEN
  IF has_provider AND NOT has_type AND jsonb_typeof(traits->'provider')='string'
   AND provider IN ('google','apple','discord') THEN candidate:=provider;
  ELSIF action='login' AND has_type AND NOT has_provider AND jsonb_typeof(traits->'provider_type')='string'
   AND provider_type IN ('google','apple','discord') THEN candidate:=provider_type;
  ELSIF action='user_signedup' AND has_provider AND NOT has_type AND provider='email'
   AND jsonb_typeof(traits->'provider')='string' THEN candidate:='email_code';
  ELSIF action='login' AND NOT has_provider AND NOT has_type THEN candidate:='email_code';
  ELSIF action='login' AND has_type AND NOT has_provider AND provider_type='email'
   AND jsonb_typeof(traits->'provider_type')='string' THEN candidate:='email_code';
  END IF;
  IF candidate IS NOT NULL THEN RETURN jsonb_build_object('kind','interactive','candidate',candidate); END IF;
 END IF;
 RETURN jsonb_build_object('kind','invalid');
END $$;

CREATE FUNCTION app.capture_auth_provenance() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE fingerprint jsonb; form jsonb; previous jsonb; context jsonb; current_xact text:=pg_current_xact_id()::text;
BEGIN
 IF session_user<>'supabase_auth_admin' THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified Auth source login';
 END IF;
 BEGIN
  fingerprint:=app.auth_audit_fingerprint(NEW.id,NEW.payload::jsonb);
  form:=app.auth_audit_form(NEW.payload::jsonb);
  previous:=nullif(current_setting('spellwake.auth_provenance',true),'')::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  PERFORM set_config('spellwake.auth_provenance',jsonb_build_object('version',1,'kind','invalid','xact',current_xact)::text,true);
  RETURN NEW;
 END;
 context:=jsonb_build_object('version',1,'kind',form->>'kind','candidate',form->>'candidate',
  'actorHash',fingerprint->>'actorHash','targetHash',fingerprint->>'targetHash',
  'sourceId',NEW.id::text,'sourceHash',app.auth_json_hash(fingerprint),'xact',current_xact);
 IF fingerprint->>'actorHash' IS NULL OR (form->>'kind'='cleanup' AND fingerprint->>'targetHash' IS NULL)
 OR (previous->>'xact'=current_xact AND (
   previous->>'kind' IS DISTINCT FROM context->>'kind'
   OR previous->>'actorHash' IS DISTINCT FROM context->>'actorHash'
   OR previous->>'candidate' IS DISTINCT FROM context->>'candidate'
   OR previous->>'targetHash' IS DISTINCT FROM context->>'targetHash')) THEN
  context:=jsonb_build_object('version',1,'kind','invalid','xact',current_xact);
 END IF;
 PERFORM set_config('spellwake.auth_provenance',context::text,true);
 RETURN NEW;
END $$;
CREATE TRIGGER application_auth_provenance AFTER INSERT ON auth.audit_log_entries
 FOR EACH ROW EXECUTE FUNCTION app.capture_auth_provenance();

CREATE FUNCTION app.read_auth_provenance(p_subject uuid,p_class text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; source auth.audit_log_entries; fingerprint jsonb; form jsonb; method text;
BEGIN
 IF session_user<>'supabase_auth_admin' THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified Auth issuance login';
 END IF;
 context:=nullif(current_setting('spellwake.auth_provenance',true),'')::jsonb;
 IF context IS NULL OR context->'version' IS DISTINCT FROM '1'::jsonb
 OR context->>'xact' IS DISTINCT FROM pg_current_xact_id()::text
 OR context->>'actorHash' IS DISTINCT FROM app.auth_uuid_hash(p_subject) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='missing current Auth provenance';
 END IF;
 SELECT * INTO source FROM auth.audit_log_entries WHERE id=(context->>'sourceId')::uuid;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth source absent'; END IF;
 fingerprint:=app.auth_audit_fingerprint(source.id,source.payload::jsonb);
 form:=app.auth_audit_form(source.payload::jsonb);
 IF context->>'sourceHash' IS DISTINCT FROM app.auth_json_hash(fingerprint)
 OR context->>'kind' IS DISTINCT FROM form->>'kind'
 OR context->>'candidate' IS DISTINCT FROM form->>'candidate'
 OR fingerprint->>'actorHash' IS DISTINCT FROM app.auth_uuid_hash(p_subject) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth source differs';
 END IF;
 IF p_class='token_refresh' AND form->>'kind'='refresh' THEN
  RETURN jsonb_build_object('kind','refresh','proofHash',app.auth_json_hash(fingerprint));
 END IF;
 method:=form->>'candidate';
 IF form->>'kind' IS DISTINCT FROM 'interactive' OR method IS NULL
 OR p_class IS DISTINCT FROM (CASE WHEN method='email_code' THEN 'otp' ELSE 'oauth' END) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unsupported Auth producer and method';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM auth.users u JOIN auth.identities i ON i.user_id=u.id
  WHERE u.id=p_subject AND u.email_confirmed_at IS NOT NULL AND coalesce(u.email,'')<>''
   AND i.provider=CASE WHEN method='email_code' THEN 'email' ELSE method END
   AND i.identity_data->'email_verified'='true'::jsonb
   AND (method<>'email_code' OR (coalesce(u.phone,'')=''
    AND NOT EXISTS(SELECT 1 FROM auth.identities phone WHERE phone.user_id=u.id AND phone.provider='phone')))) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified supported Auth identity required';
 END IF;
 RETURN jsonb_build_object('kind','interactive','method',method,'proofHash',app.auth_json_hash(fingerprint));
EXCEPTION WHEN invalid_text_representation THEN
 RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='invalid Auth provenance encoding';
END $$;

-- Shared only within Auth writers; generic domain completion cannot populate
-- result_ref atomically with the exact acknowledgment required by053.
CREATE FUNCTION app.accept_auth_receipt(p_id uuid,p_result jsonb) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.command_receipt SET state='accepted',result_ref=p_result::text,
  response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',id::text))
 WHERE id=p_id AND state='pending' AND scope IN ('auth_binding_v1','auth_session_v1','auth_retirement_v1')
  AND principal_key~'^auth:[0-9a-f]{64}$';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth receipt disposition changed'; END IF;
END $$;

CREATE FUNCTION app.authorize_auth_session(p_subject uuid,p_session uuid,p_class text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE provenance jsonb; principal text; subject_hash text; method text; initial_class text;
 binding app.command_receipt; grant_row app.command_receipt; retired app.command_receipt;
 payload jsonb; created boolean;
BEGIN
 provenance:=app.read_auth_provenance(p_subject,p_class);
 subject_hash:=app.auth_uuid_hash(p_subject); principal:='auth:'||subject_hash;
 PERFORM pg_advisory_xact_lock(hashtextextended('auth_authority_v1:'||subject_hash,0));
 SELECT * INTO retired FROM app.command_receipt WHERE principal_key=principal AND scope='auth_retirement_v1' AND request_ref='primary';
 IF FOUND THEN
  PERFORM app.check_auth_receipt_shape(retired);
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth subject retired';
 END IF;
 SELECT * INTO binding FROM app.command_receipt WHERE principal_key=principal AND scope='auth_binding_v1' AND request_ref='primary';
 IF p_class='token_refresh' THEN
  IF NOT FOUND OR binding.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='refresh binding absent'; END IF;
 ELSE
  method:=provenance->>'method';
  IF NOT FOUND THEN
   payload:=jsonb_build_object('version',1,'scope','auth_binding_v1','subjectHash',subject_hash);
   INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
   VALUES(principal,'auth_binding_v1','primary','primary',app.auth_json_hash(payload),'pending','')
   ON CONFLICT DO NOTHING RETURNING * INTO binding;
   created:=FOUND;
   IF created THEN
    PERFORM app.accept_auth_receipt(binding.id,jsonb_build_object('version',1,'method',method,'proofHash',provenance->>'proofHash'));
   END IF;
   SELECT * INTO binding FROM app.command_receipt WHERE principal_key=principal AND scope='auth_binding_v1' AND request_ref='primary';
   IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth binding winner requires retry'; END IF;
  END IF;
 END IF;
 PERFORM app.check_auth_receipt_shape(binding);
 IF binding.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth binding not terminal'; END IF;
 IF p_class<>'token_refresh' AND (binding.result_ref::jsonb)->>'method' IS DISTINCT FROM method THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='account uses another Auth method';
 END IF;
 method:=(binding.result_ref::jsonb)->>'method';
 initial_class:=CASE WHEN method='email_code' THEN 'otp' ELSE 'oauth' END;
 IF NOT app.auth_session_is_live(p_subject,p_session,initial_class) THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='live matching Auth session required';
 END IF;
 SELECT * INTO grant_row FROM app.command_receipt WHERE principal_key=principal AND scope='auth_session_v1' AND request_ref=app.auth_uuid_hash(p_session);
 IF NOT FOUND THEN
  IF p_class='token_refresh' THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='refresh session grant absent'; END IF;
  payload:=jsonb_build_object('version',1,'scope','auth_session_v1','subjectHash',subject_hash,
   'sessionHash',app.auth_uuid_hash(p_session),'bindingId',binding.id::text);
  INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
  VALUES(principal,'auth_session_v1',app.auth_uuid_hash(p_session),binding.id::text,app.auth_json_hash(payload),'pending','')
  ON CONFLICT DO NOTHING RETURNING * INTO grant_row;
  created:=FOUND;
  IF created THEN
   PERFORM app.accept_auth_receipt(grant_row.id,jsonb_build_object('version',1,'method',method,'initialClass',initial_class,
    'bindingId',binding.id::text,'proofHash',provenance->>'proofHash'));
  END IF;
  SELECT * INTO grant_row FROM app.command_receipt WHERE principal_key=principal AND scope='auth_session_v1' AND request_ref=app.auth_uuid_hash(p_session);
  IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='Auth session winner requires retry'; END IF;
 END IF;
 PERFORM app.check_auth_receipt_shape(grant_row);
 IF grant_row.state<>'accepted' OR grant_row.target_ref<>binding.id::text THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth session grant mismatch';
 END IF;
 PERFORM set_config('spellwake.auth_provenance','',true);
 RETURN jsonb_build_object('version',1,'method',method,'bindingId',binding.id::text,'sessionGrantId',grant_row.id::text);
END $$;

CREATE FUNCTION app.custom_access_token_hook(p_event jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE subject uuid; session_id uuid; claims jsonb; authority jsonb;
BEGIN
 IF session_user<>'supabase_auth_admin' THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='unqualified Auth hook login'; END IF;
 claims:=p_event->'claims'; subject:=(p_event->>'user_id')::uuid; session_id:=(claims->>'session_id')::uuid;
 IF jsonb_typeof(claims) IS DISTINCT FROM 'object' OR subject IS NULL OR session_id IS NULL
 OR (claims->>'sub')::uuid IS DISTINCT FROM subject THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='invalid Auth token subject or session';
 END IF;
 authority:=app.authorize_auth_session(subject,session_id,p_event->>'authentication_method');
 RETURN jsonb_build_object('claims',jsonb_set(claims,'{spellwake_auth}',authority,true));
EXCEPTION WHEN invalid_text_representation THEN
 RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='invalid Auth hook encoding';
END $$;

REVOKE EXECUTE ON FUNCTION app.auth_audit_fingerprint(uuid,jsonb),app.auth_audit_form(jsonb),
 app.capture_auth_provenance(),app.read_auth_provenance(uuid,text),app.accept_auth_receipt(uuid,jsonb),
 app.authorize_auth_session(uuid,uuid,text),app.custom_access_token_hook(jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT USAGE ON SCHEMA app TO supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.custom_access_token_hook(jsonb) TO supabase_auth_admin;

COMMIT;
