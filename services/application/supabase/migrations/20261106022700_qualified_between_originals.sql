BEGIN;

CREATE FUNCTION app.native_between_intent_is_valid(p_original jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text;
BEGIN
 IF jsonb_typeof(p_original) IS DISTINCT FROM 'object' OR octet_length(p_original::text)>32768 THEN RETURN false;END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_original) k WHERE k NOT IN('requestId','decisionId','expectedRevision','selectedRevisionId'))
  OR jsonb_typeof(p_original->'expectedRevision') IS DISTINCT FROM 'string'
  OR p_original->>'expectedRevision' !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN false;END IF;
 FOREACH key IN ARRAY ARRAY['requestId','decisionId','selectedRevisionId'] LOOP
  IF jsonb_typeof(p_original->key) IS DISTINCT FROM 'string'
   OR p_original->>key !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR p_original->>key='00000000-0000-0000-0000-000000000000' THEN RETURN false;END IF;
 END LOOP;
 RETURN true;
END $$;

CREATE FUNCTION app.qualify_native_between_command(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor jsonb;decision app.between_game_decision;m app.match;
 parent app.command_receipt;child app.command_receipt;source jsonb;terms jsonb;response jsonb;
 request_id uuid;decision_id uuid;revision_id uuid;basis numeric;is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified reply requires read committed retry';END IF;
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  player:=app.lock_session_account(p_subject,p_session);
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_between_intent_is_valid(p_original) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(p_original->>'requestId')::uuid;decision_id:=(p_original->>'decisionId')::uuid;
 revision_id:=(p_original->>'selectedRevisionId')::uuid;basis:=(p_original->>'expectedRevision')::numeric;
 SELECT * INTO decision FROM app.between_game_decision WHERE id=decision_id;
 IF is_human THEN
  SELECT jsonb_build_object('seatId',seat.id,'kind','human','accountId',player.id,'grantId',NULL,'bindingId',NULL,'bindingEpoch',NULL) INTO actor
   FROM app.match_seat seat WHERE seat.id=decision.seat_id AND seat.match_id=decision.match_id AND seat.controller_kind='human' AND seat.account_id=player.id;
 ELSE actor:=app.lock_native_controller_actor(p_grant,p_epoch,p_secret_ref,decision.match_id);
  IF actor->>'seatId' IS DISTINCT FROM decision.seat_id::text THEN actor:=NULL;END IF;
 END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF is_human THEN parent:=app.begin_account_command(player.id,request_id,'qualified_between_intent_v1',decision_id::text,jsonb_build_object('decision',decision_id,'original',p_original));
 ELSE parent:=app.begin_native_source_receipt('controller:'||p_grant::text,request_id::text,'qualified_between_intent_v1',decision_id,jsonb_build_object('decision',decision_id,'original',p_original),NULL,p_grant);END IF;
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted reply preparation';END IF;
 IF p_prepared->>'version' IS DISTINCT FROM 'native-between-prepared-v1'
  OR EXISTS(SELECT 1 FROM unnest(ARRAY['sourceHash','replyRef','qualificationRef']) k WHERE jsonb_typeof(p_prepared->k) IS DISTINCT FROM 'string')
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('version','sourceHash','replyRef','qualificationRef'))
  OR p_prepared->>'sourceHash' !~ '^[0-9a-f]{64}$'
  OR length(btrim(p_prepared->>'replyRef')) NOT BETWEEN 1 AND 2048 OR length(btrim(p_prepared->>'qualificationRef')) NOT BETWEEN 1 AND 2048 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted reply source';END IF;
 child:=app.claim_between_game_reply(decision_id,basis,revision_id,p_prepared->>'replyRef',p_prepared->>'qualificationRef',parent.id);
 SELECT * INTO m FROM app.match WHERE id=decision.match_id FOR NO KEY UPDATE;
 SELECT * INTO decision FROM app.between_game_decision WHERE id=decision_id FOR NO KEY UPDATE;
 PERFORM id FROM app.match_clock WHERE match_seat_id=decision.seat_id FOR SHARE;
 IF decision.match_id IS DISTINCT FROM m.id OR decision.seat_id::text IS DISTINCT FROM actor->>'seatId' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='reply source lineage changed before fence';END IF;
 IF is_human THEN
  player:=app.lock_session_account(p_subject,p_session);
  IF NOT app.native_account_may_continue(player,m.id) THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='admitted match continuation required';END IF;
 ELSE IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,m.id) IS DISTINCT FROM actor THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='current controller authority required';END IF;END IF;
 source:=app.read_native_between_source(decision_id,basis,revision_id);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'sourceHash' IS DISTINCT FROM p_prepared->>'sourceHash' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='trusted reply source changed; reprepare original';END IF;
 terms:=source->'data'->'terms';
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','qualified-human-v1','operation','between','original',p_original,
  'targetId',decision_id,'sourceTerms',terms,'sourceHash',source->'data'->>'sourceHash','prepared',p_prepared,'childReceiptId',child.id) WHERE id=parent.id;
 IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new reply original has preexisting canonical child';END IF;
 response:=app.apply_between_game_reply(child.id,decision_id,basis,revision_id,p_prepared->>'replyRef',p_prepared->>'qualificationRef',NULL);
 IF is_human THEN player:=app.lock_session_account(p_subject,p_session);
 ELSE IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,m.id) IS DISTINCT FROM actor THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='current controller authority required';END IF;END IF;
 RETURN app.finish_account_command(parent.id,response->>'status'='success',(response->'data')-'qualificationRef');
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object(CASE WHEN is_human THEN 'session' ELSE 'actor' END,CASE WHEN is_human THEN 'unauthorized' ELSE 'unavailable' END));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;

CREATE FUNCTION app.qualify_between_game_command(p_subject uuid,p_session uuid,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.qualify_native_between_command(p_subject,p_session,NULL,NULL,NULL,p_original,p_prepared) $$;
CREATE FUNCTION app.qualify_controller_between_game_command(p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb,p_prepared jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.qualify_native_between_command(NULL,NULL,p_grant,p_epoch,p_secret_ref,p_original,p_prepared) $$;

CREATE FUNCTION app.read_qualified_native_between_command(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor jsonb;decision app.between_game_decision;receipt app.command_receipt;principal text;
 is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  player:=app.lock_session_account(p_subject,p_session);principal:='human:'||player.id::text;
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_between_intent_is_valid(p_original) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO decision FROM app.between_game_decision WHERE id=(p_original->>'decisionId')::uuid;
 IF is_human THEN
  IF NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=decision.seat_id AND match_id=decision.match_id AND controller_kind='human' AND account_id=player.id) THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 ELSE
  actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,decision.match_id);
  IF actor IS NULL OR actor->>'seatId' IS DISTINCT FROM decision.seat_id::text THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
  principal:='controller:'||p_grant::text;
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key=principal AND scope='qualified_between_intent_v1' AND request_ref=p_original->>'requestId';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','success','data',jsonb_build_object('state','unknown','response',NULL));END IF;
 IF receipt.target_ref IS DISTINCT FROM p_original->>'decisionId' OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('decision',(p_original->>'decisionId')::uuid,'original',p_original)) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified original not terminal';END IF;
 IF NOT is_human AND app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,decision.match_id) IS DISTINCT FROM actor THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('state',receipt.state,'response',receipt.response_payload));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_qualified_between_game_command(p_subject uuid,p_session uuid,p_original jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_qualified_native_between_command(p_subject,p_session,NULL,NULL,NULL,p_original) $$;
CREATE FUNCTION app.read_qualified_controller_between_game_command(p_grant uuid,p_epoch numeric,p_secret_ref text,p_original jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_qualified_native_between_command(NULL,NULL,p_grant,p_epoch,p_secret_ref,p_original) $$;

CREATE FUNCTION app.check_qualified_between_original(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE parent app.command_receipt;child app.command_receipt;source jsonb;input jsonb;seat app.match_seat;
BEGIN
 SELECT * INTO parent FROM app.command_receipt WHERE id=p_receipt AND scope='qualified_between_intent_v1';
 IF parent.id IS NULL THEN RETURN;END IF;source:=parent.normalized_source;
 IF parent.state='pending' OR source->>'version' IS DISTINCT FROM 'qualified-human-v1' OR source->>'operation' IS DISTINCT FROM 'between'
  OR NOT app.native_between_intent_is_valid(source->'original') OR source->>'targetId' IS DISTINCT FROM parent.target_ref
  OR source->'original'->>'decisionId' IS DISTINCT FROM parent.target_ref OR source->'original'->>'requestId' IS DISTINCT FROM parent.request_ref
  OR source->'sourceTerms'->'decision'->>'id' IS DISTINCT FROM parent.target_ref
  OR source->'sourceTerms'->'match'->>'revision' IS DISTINCT FROM source->'original'->>'expectedRevision'
  OR source->'sourceTerms'->>'selectedRevisionId' IS DISTINCT FROM source->'original'->>'selectedRevisionId'
  OR parent.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('decision',parent.target_ref::uuid,'original',source->'original'))
  OR source->>'sourceHash' IS DISTINCT FROM app.auth_json_hash(source->'sourceTerms')
  OR source->'prepared'->>'sourceHash' IS DISTINCT FROM source->>'sourceHash' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified reply original lacks exact terminal source';END IF;
 SELECT * INTO seat FROM app.match_seat WHERE id=(source->'sourceTerms'->'decision'->>'seatId')::uuid;
 IF (parent.account_id IS NULL)=(parent.controller_grant_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified reply requires one retained actor owner';END IF;
 IF seat.id IS NULL OR seat.match_id::text IS DISTINCT FROM source->'sourceTerms'->'decision'->>'matchId'
  OR NOT((parent.account_id IS NOT NULL AND parent.controller_grant_id IS NULL AND parent.principal_key='human:'||parent.account_id::text AND seat.controller_kind='human' AND seat.account_id=parent.account_id)
   OR(parent.account_id IS NULL AND parent.controller_grant_id IS NOT NULL AND parent.principal_key='controller:'||parent.controller_grant_id::text AND seat.controller_kind='ai'
    AND EXISTS(SELECT 1 FROM app.controller_grant WHERE id=parent.controller_grant_id AND match_seat_id=seat.id))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified reply original has wrong retained actor';END IF;
 SELECT * INTO child FROM app.command_receipt WHERE id=(source->>'childReceiptId')::uuid;
 input:=jsonb_build_object('decision',parent.target_ref::uuid,'basis',source->'original'->>'expectedRevision','revision',source->'original'->>'selectedRevisionId',
  'reply',source->'prepared'->>'replyRef','qualification',source->'prepared'->>'qualificationRef');
 IF child.id IS NULL OR child.scope<>'between_game_reply_v1' OR child.principal_key IS DISTINCT FROM 'match-seat:'||seat.id::text
  OR child.target_ref IS DISTINCT FROM parent.target_ref OR child.request_ref IS DISTINCT FROM parent.id::text
  OR child.payload_hash IS DISTINCT FROM app.auth_json_hash(input) OR child.state IS DISTINCT FROM parent.state
  OR parent.response_payload IS DISTINCT FROM jsonb_build_object('status',child.response_payload->>'status','data',(child.response_payload->'data')-'qualificationRef') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified reply original differs from canonical child';END IF;
END $$;
CREATE FUNCTION app.guard_qualified_between_original() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$ BEGIN PERFORM app.check_qualified_between_original(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER qualified_between_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope='qualified_between_intent_v1') EXECUTE FUNCTION app.guard_qualified_between_original();

REVOKE ALL ON FUNCTION app.native_between_intent_is_valid(jsonb),app.qualify_native_between_command(uuid,uuid,uuid,numeric,text,jsonb,jsonb),
 app.qualify_between_game_command(uuid,uuid,jsonb,jsonb),app.qualify_controller_between_game_command(uuid,numeric,text,jsonb,jsonb),
 app.read_qualified_native_between_command(uuid,uuid,uuid,numeric,text,jsonb),app.read_qualified_between_game_command(uuid,uuid,jsonb),
 app.read_qualified_controller_between_game_command(uuid,numeric,text,jsonb),app.check_qualified_between_original(uuid),app.guard_qualified_between_original()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
