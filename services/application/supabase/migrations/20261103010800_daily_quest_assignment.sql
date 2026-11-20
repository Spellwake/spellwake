BEGIN;
ALTER TABLE app.quest_definition_revision ADD CONSTRAINT quest_reward_amount CHECK(reward_coins IN (500,750));

CREATE FUNCTION app.check_quest_bank_capacity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest bank changes require read committed retry'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF (SELECT count(*) FROM app.quest_assignment WHERE account_id=NEW.account_id AND completed_at IS NULL AND replaced_by_id IS NULL)>3 THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest bank cannot contain more than three active assignments'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER quest_bank_capacity AFTER INSERT OR UPDATE ON app.quest_assignment
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_quest_bank_capacity();

CREATE FUNCTION app.guard_daily_quest_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR (NEW.id,NEW.account_id,NEW.utc_day) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.utc_day)
  OR (OLD.generated_assignment_id IS NOT NULL AND NEW.generated_assignment_id IS DISTINCT FROM OLD.generated_assignment_id)
  OR NEW.rerolls_used<OLD.rerolls_used OR NEW.revision<OLD.revision
  OR (NEW IS DISTINCT FROM OLD AND NEW.revision<=OLD.revision) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='daily quest identity and consumed allowances are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER daily_quest_history BEFORE UPDATE OR DELETE ON app.daily_quest_state
 FOR EACH ROW EXECUTE FUNCTION app.guard_daily_quest_history();

-- Selection is an input from the trusted assignment policy. This does not pick
-- or activate a shared-vs-personal distribution policy.
CREATE FUNCTION app.generate_daily_quest(p_actor uuid,p_definition uuid,p_day text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; daily app.daily_quest_state; assignment_id uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'quest_generate',p_actor::text,
  jsonb_build_object('definition',p_definition,'day',p_day));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF app.valid_utc_day(p_day) IS DISTINCT FROM true OR p_day<>to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('day','not current UTC day')); END IF;
 SELECT * INTO daily FROM app.daily_quest_state WHERE account_id=p_actor AND utc_day=p_day FOR NO KEY UPDATE;
 IF daily.generated_assignment_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',daily.generated_assignment_id)); END IF;
 PERFORM id FROM app.quest_definition_revision WHERE id=p_definition;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('definition','unavailable')); END IF;
 IF (SELECT count(*) FROM app.quest_assignment WHERE account_id=p_actor AND completed_at IS NULL AND replaced_by_id IS NULL)>=3 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('bank','full')); END IF;
 INSERT INTO app.quest_assignment(account_id,definition_id,source_day,assignment_ref,progress)
 VALUES(p_actor,p_definition,p_day,'daily:'||p_day,0) RETURNING id INTO assignment_id;
 INSERT INTO app.daily_quest_state(account_id,utc_day,generated_assignment_id,rerolls_used,revision)
 VALUES(p_actor,p_day,assignment_id,0,0)
 ON CONFLICT(account_id,utc_day) DO UPDATE SET generated_assignment_id=EXCLUDED.generated_assignment_id,revision=app.daily_quest_state.revision+1;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',assignment_id));
END $$;

CREATE FUNCTION app.reroll_daily_quest(p_actor uuid,p_assignment uuid,p_definition uuid,p_day text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; daily app.daily_quest_state; previous app.quest_assignment; replacement uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'quest_reroll',p_assignment::text,
  jsonb_build_object('definition',p_definition,'day',p_day));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF app.valid_utc_day(p_day) IS DISTINCT FROM true OR p_day<>to_char(clock_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('day','not current UTC day')); END IF;
 SELECT * INTO previous FROM app.quest_assignment WHERE id=p_assignment AND account_id=p_actor FOR NO KEY UPDATE;
 IF previous.id IS NULL OR previous.completed_at IS NOT NULL OR previous.replaced_by_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('assignment','unavailable')); END IF;
 IF p_definition IS NOT DISTINCT FROM previous.definition_id OR NOT EXISTS(SELECT 1 FROM app.quest_definition_revision WHERE id=p_definition) THEN
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
REVOKE ALL ON FUNCTION app.check_quest_bank_capacity(),app.guard_daily_quest_history(),app.generate_daily_quest(uuid,uuid,text,uuid),app.reroll_daily_quest(uuid,uuid,uuid,text,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
