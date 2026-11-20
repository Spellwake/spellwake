-- Authenticated original-intent rerolls. No definitions are activated here.
BEGIN;
ALTER TABLE app.quest_definition_revision ADD COLUMN reroll_enabled boolean NOT NULL DEFAULT false;
ALTER TABLE app.quest_definition_revision ADD CONSTRAINT enabled_reroll_qualified
 CHECK(NOT reroll_enabled OR (fact_kind IS NOT NULL AND fact_schema_revision IS NOT NULL AND objective_qualification_ref IS NOT NULL));
-- One enabled revision per logical quest avoids weighting by retained history.
CREATE UNIQUE INDEX quest_reroll_enabled_definition ON app.quest_definition_revision(definition_key) WHERE reroll_enabled;
CREATE FUNCTION app.guard_quest_definition_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest definition history cannot be deleted'; END IF;
 IF (to_jsonb(NEW)-'reroll_enabled') IS DISTINCT FROM (to_jsonb(OLD)-'reroll_enabled') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest definition content is immutable'; END IF;
 RETURN NEW;
END $$;
DROP TRIGGER quest_definition_history ON app.quest_definition_revision;
CREATE TRIGGER quest_definition_history BEFORE UPDATE OR DELETE ON app.quest_definition_revision
 FOR EACH ROW EXECUTE FUNCTION app.guard_quest_definition_revision();

-- Reuse195's mutation body for legacy trusted selection and session-bound intent.
CREATE FUNCTION app.apply_quest_reroll(p_receipt uuid,p_actor uuid,p_assignment uuid,p_definition uuid,p_day text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; daily app.daily_quest_state; previous app.quest_assignment; replacement uuid;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt FOR UPDATE;
 IF receipt.id IS NULL OR receipt.state<>'pending' OR receipt.account_id IS DISTINCT FROM p_actor
 OR receipt.principal_key IS DISTINCT FROM 'human:'||p_actor::text OR receipt.target_ref IS DISTINCT FROM p_assignment::text
 OR receipt.scope NOT IN('quest_reroll','quest_reroll_intent_v1') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='original pending reroll receipt required'; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF app.valid_utc_day(p_day) IS DISTINCT FROM true OR p_day<>to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('day','not current UTC day')); END IF;
 SELECT * INTO previous FROM app.quest_assignment WHERE id=p_assignment AND account_id=p_actor FOR NO KEY UPDATE;
 IF previous.id IS NULL OR previous.completed_at IS NOT NULL OR previous.replaced_by_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assignment','unavailable')); END IF;
 IF p_definition IS NOT DISTINCT FROM previous.definition_id OR NOT EXISTS(SELECT 1 FROM app.quest_definition_revision WHERE id=p_definition AND fact_kind IS NOT NULL AND fact_schema_revision IS NOT NULL AND objective_qualification_ref IS NOT NULL) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('definition','different available definition required')); END IF;
 SELECT * INTO daily FROM app.daily_quest_state WHERE account_id=p_actor AND utc_day=p_day FOR NO KEY UPDATE;
 IF daily.rerolls_used=1 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('reroll','already used')); END IF;
 INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress)
 VALUES(p_actor,p_definition,p_day,'reroll:'||p_day||':'||receipt.id::text,0) RETURNING id INTO replacement;
 UPDATE app.quest_assignment SET replaced_by_id=replacement WHERE id=p_assignment;
 INSERT INTO app.daily_quest_state(account_id,utc_day,rerolls_used,revision) VALUES(p_actor,p_day,1,0)
 ON CONFLICT(account_id,utc_day) DO UPDATE SET rerolls_used=1,revision=app.daily_quest_state.revision+1;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',replacement,'replacedId',p_assignment));
END $$;
CREATE OR REPLACE FUNCTION app.reroll_daily_quest(p_actor uuid,p_assignment uuid,p_definition uuid,p_day text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'quest_reroll',p_assignment::text,
  jsonb_build_object('definition',p_definition,'day',p_day));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 RETURN app.apply_quest_reroll(receipt.id,p_actor,p_assignment,p_definition,p_day);
END $$;

CREATE FUNCTION app.reroll_quest_command(p_subject uuid,p_session uuid,p_request uuid,p_assignment uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; previous app.quest_assignment; selected uuid; source_day text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest reroll requires read committed retry'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_assignment IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_assignment) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'quest_reroll_intent_v1',p_assignment::text,jsonb_build_object('assignmentId',p_assignment));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state<>'active' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO previous FROM app.quest_assignment WHERE id=p_assignment AND account_id=player.id FOR NO KEY UPDATE;
 IF previous.id IS NULL OR previous.completed_at IS NOT NULL OR previous.replaced_by_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assignment','unavailable')); END IF;
 source_day:=to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD');
 -- Operational membership is snapshotted at selection. Later disabling does not
 -- invalidate an in-flight selection or an accepted historical receipt.
 SELECT candidate.id INTO selected FROM app.quest_definition_revision candidate
 JOIN app.quest_definition_revision original ON original.id=previous.definition_id
 WHERE candidate.reroll_enabled AND candidate.fact_kind IS NOT NULL
 AND candidate.fact_schema_revision IS NOT NULL AND candidate.objective_qualification_ref IS NOT NULL
 AND candidate.definition_key<>original.definition_key ORDER BY gen_random_uuid() LIMIT 1;
 IF selected IS NULL THEN RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='quest reroll selection unavailable'; END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','quest-reroll-equal-v1','definitionId',selected,'day',source_day) WHERE id=receipt.id;
 RETURN app.apply_quest_reroll(receipt.id,player.id,p_assignment,selected,source_day);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.read_quest_reroll_command(p_subject uuid,p_session uuid,p_request uuid,p_assignment uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest reroll requires read committed retry'; END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_assignment IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_request,p_assignment) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='quest_reroll_intent_v1' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_assignment::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('assignmentId',p_assignment))) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'quest_reroll_intent_v1',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;

REVOKE ALL ON FUNCTION app.guard_quest_definition_revision(),app.apply_quest_reroll(uuid,uuid,uuid,uuid,text),app.reroll_quest_command(uuid,uuid,uuid,uuid),app.read_quest_reroll_command(uuid,uuid,uuid,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.reroll_quest_command(uuid,uuid,uuid,uuid),app.read_quest_reroll_command(uuid,uuid,uuid,uuid) TO service_role;
COMMIT;
