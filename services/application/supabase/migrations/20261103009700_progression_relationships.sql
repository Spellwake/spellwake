BEGIN;

CREATE UNIQUE INDEX match_human_member ON app.match_seat(match_id,account_id)
 WHERE controller_kind='human' AND account_id IS NOT NULL;
CREATE UNIQUE INDEX competitive_account_match ON app.competitive_result(account_id,match_result_id);
ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_account_scope UNIQUE(id,account_id);
ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_generation_scope UNIQUE(id,account_id,source_day);
ALTER TABLE app.game_activity_fact ADD CONSTRAINT activity_account_scope UNIQUE(id,account_id);
ALTER TABLE app.match_result ADD CONSTRAINT ranked_result_scope UNIQUE(id,ranking_format_id,committed_at);

ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_replacement_account
 FOREIGN KEY(replaced_by_id,account_id) REFERENCES app.quest_assignment(id,account_id) DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_reward_account
 FOREIGN KEY(reward_source_fact_id,account_id) REFERENCES app.game_activity_fact(id,account_id) DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.daily_quest_state ADD CONSTRAINT generated_quest_account_day
 FOREIGN KEY(generated_assignment_id,account_id,utc_day) REFERENCES app.quest_assignment(id,account_id,source_day) DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.competitive_result ADD CONSTRAINT competitive_result_source
 FOREIGN KEY(match_result_id,format_id,completed_at) REFERENCES app.match_result(id,ranking_format_id,committed_at) DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_replacement_state
 CHECK (replaced_by_id IS NULL OR (replaced_by_id<>id AND completed_at IS NULL));
ALTER TABLE app.quest_definition_revision ADD CONSTRAINT quest_positive_target CHECK(target>0);

CREATE FUNCTION app.valid_utc_day(p_value text) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF (p_value ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$') IS DISTINCT FROM true THEN RETURN false; END IF;
 RETURN to_char(p_value::date,'YYYY-MM-DD')=p_value;
EXCEPTION WHEN datetime_field_overflow OR invalid_datetime_format THEN RETURN false;
END $$;
ALTER TABLE app.quest_assignment ADD CONSTRAINT quest_source_day CHECK(app.valid_utc_day(source_day));
ALTER TABLE app.daily_quest_state ADD CONSTRAINT quest_utc_day CHECK(app.valid_utc_day(utc_day));
ALTER TABLE app.daily_win_bucket ADD CONSTRAINT wins_utc_day CHECK(app.valid_utc_day(utc_day));
ALTER TABLE app.ai_loss_bucket ADD CONSTRAINT ai_allowance_utc_day CHECK(app.valid_utc_day(utc_day));
ALTER TABLE app.monthly_account_activity ADD CONSTRAINT activity_utc_month
 CHECK (utc_month ~ '^[0-9]{4}-[0-9]{2}$' AND app.valid_utc_day(utc_month||'-01'));

CREATE FUNCTION app.guard_quest_assignment() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR
 (NEW.id,NEW.account_id,NEW.definition_id,NEW.source_day,NEW.assignment_ref)
  IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.definition_id,OLD.source_day,OLD.assignment_ref)
 OR (OLD.replaced_by_id IS NOT NULL AND NEW IS DISTINCT FROM OLD)
 OR (OLD.completed_at IS NOT NULL AND NEW IS DISTINCT FROM OLD) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest assignment identity and completed history are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER quest_assignment_history BEFORE UPDATE OR DELETE ON app.quest_assignment
 FOR EACH ROW EXECUTE FUNCTION app.guard_quest_assignment();
CREATE TRIGGER quest_definition_history BEFORE UPDATE OR DELETE ON app.quest_definition_revision
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
CREATE TRIGGER activity_fact_history BEFORE UPDATE OR DELETE ON app.game_activity_fact
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.check_quest_replacement_graph() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NEW.replaced_by_id IS NULL THEN RETURN NULL; END IF;
 -- Serialize same-account graph edits. A stale snapshot cannot qualify a competing edge.
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest replacement requires read committed retry'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF EXISTS(WITH RECURSIVE chain(id,next_id,path,cycle) AS (
  SELECT id,replaced_by_id,ARRAY[id],false FROM app.quest_assignment WHERE id=NEW.id
  UNION ALL
  SELECT q.id,q.replaced_by_id,c.path||q.id,q.id=ANY(c.path)
   FROM chain c JOIN app.quest_assignment q ON q.id=c.next_id WHERE NOT c.cycle
 ) SELECT 1 FROM chain WHERE cycle) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest replacement cycle'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER quest_replacement_graph AFTER INSERT OR UPDATE ON app.quest_assignment
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_quest_replacement_graph();

REVOKE ALL ON FUNCTION app.valid_utc_day(text),app.guard_quest_assignment(),app.check_quest_replacement_graph()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
