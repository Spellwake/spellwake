BEGIN;
ALTER TABLE app.quest_definition_revision ADD COLUMN fact_kind text;
ALTER TABLE app.quest_definition_revision ADD COLUMN fact_schema_revision text;
ALTER TABLE app.quest_definition_revision ADD COLUMN objective_qualification_ref text;
ALTER TABLE app.quest_definition_revision ADD CONSTRAINT quest_objective_tuple CHECK(
 (fact_kind IS NULL AND fact_schema_revision IS NULL AND objective_qualification_ref IS NULL)
 OR (fact_kind IS NOT NULL AND length(btrim(fact_kind)) BETWEEN 1 AND 128 AND fact_schema_revision IS NOT NULL AND length(btrim(fact_schema_revision)) BETWEEN 1 AND 128 AND objective_qualification_ref IS NOT NULL AND length(btrim(objective_qualification_ref)) BETWEEN 1 AND 2048 AND target>0));
ALTER TABLE app.quest_assignment ADD COLUMN assigned_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(assigned_at));
ALTER TABLE app.game_event ADD COLUMN accepted_at timestamptz NOT NULL DEFAULT clock_timestamp() CHECK(isfinite(accepted_at));
ALTER TABLE app.activity_consumer_cursor ADD COLUMN initialized boolean NOT NULL DEFAULT false;
CREATE OR REPLACE FUNCTION app.guard_quest_assignment() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR (NEW.id,NEW.account_id,NEW.definition_id,NEW.source_day,NEW.assignment_ref,NEW.assigned_at) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.definition_id,OLD.source_day,OLD.assignment_ref,OLD.assigned_at)
 OR NEW.progress<OLD.progress OR (OLD.replaced_by_id IS NOT NULL AND NEW IS DISTINCT FROM OLD) OR (OLD.completed_at IS NOT NULL AND NEW IS DISTINCT FROM OLD) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest identity, counted progress and completed history are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION app.guard_activity_cursor_history() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR (NEW.id,NEW.game_id,NEW.consumer_key) IS DISTINCT FROM (OLD.id,OLD.game_id,OLD.consumer_key) OR NEW.last_sequence<OLD.last_sequence OR (OLD.initialized AND NOT NEW.initialized) OR NEW.revision<>OLD.revision+1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='activity cursor identity and consumed source frontier cannot rewind'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER activity_cursor_history BEFORE UPDATE OR DELETE ON app.activity_consumer_cursor FOR EACH ROW EXECUTE FUNCTION app.guard_activity_cursor_history();
CREATE FUNCTION app.check_quest_reward_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE q app.quest_assignment; definition app.quest_definition_revision; fact app.game_activity_fact; source_time timestamptz; source_mode text; operation app.ledger_operation; credited numeric;
BEGIN
 SELECT * INTO STRICT q FROM app.quest_assignment WHERE id=NEW.id;
 SELECT * INTO STRICT definition FROM app.quest_definition_revision WHERE id=q.definition_id;
 IF definition.fact_kind IS NULL THEN RETURN NULL; END IF;
 IF q.progress>definition.target OR ((q.completed_at IS NOT NULL)<>(q.progress=definition.target)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified quest completion must match its capped objective progress'; END IF;
 IF q.completed_at IS NULL THEN
 IF q.reward_operation_id IS NOT NULL OR q.reward_source_fact_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unfinished quest cannot have a reward'; END IF;
 RETURN NULL;
 END IF;
 SELECT * INTO fact FROM app.game_activity_fact WHERE id=q.reward_source_fact_id;
 SELECT e.accepted_at,m.mode INTO source_time,source_mode FROM app.game_event e JOIN app.game g ON g.id=e.game_id JOIN app.match m ON m.id=g.match_id WHERE e.id=fact.source_event_id;
 SELECT * INTO operation FROM app.ledger_operation WHERE id=q.reward_operation_id;
 IF fact.id IS NULL OR fact.account_id<>q.account_id OR fact.kind<>definition.fact_kind OR fact.fact_revision<>definition.fact_schema_revision OR source_mode<>'collection' OR source_time<q.assigned_at OR q.completed_at IS DISTINCT FROM source_time
 OR operation.id IS NULL OR operation.kind<>'quest_reward' OR operation.source_scope<>'quest_reward_v1' OR operation.source_ref<>q.id::text OR operation.actor_account_id IS DISTINCT FROM q.account_id OR operation.state<>'committed'
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt receipt WHERE receipt.principal_key='quests:'||q.account_id::text AND receipt.scope='quest_reward_v1' AND receipt.target_ref=q.id::text AND receipt.operation_id=q.reward_operation_id AND receipt.state='accepted') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest reward requires its objective, collection fact and accepted accounting source'; END IF;
 SELECT coalesce(sum(p.quantity),0) INTO credited FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id JOIN app.asset_owner owner ON owner.id=i.owner_id JOIN app.asset_definition asset ON asset.id=i.asset_definition_id WHERE p.operation_id=operation.id AND p.direction='credit' AND owner.account_id=q.account_id AND asset.currency_key='coin';
 IF credited<>definition.reward_coins OR EXISTS(SELECT 1 FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id JOIN app.asset_owner owner ON owner.id=i.owner_id JOIN app.asset_definition asset ON asset.id=i.asset_definition_id WHERE p.operation_id=operation.id AND (p.direction<>'credit' OR owner.account_id IS DISTINCT FROM q.account_id OR asset.currency_key IS DISTINCT FROM 'coin')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quest coin posting must equal its pinned reward'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER quest_reward_source AFTER INSERT OR UPDATE ON app.quest_assignment DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_quest_reward_source();
REVOKE ALL ON FUNCTION app.guard_activity_cursor_history(),app.check_quest_reward_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
