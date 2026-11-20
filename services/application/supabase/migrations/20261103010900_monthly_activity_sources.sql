BEGIN;
CREATE FUNCTION app.check_monthly_activity_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE participation app.game_participation;
BEGIN
 IF NEW.activity_policy_ref<>'monthly-real-game-v1'
 OR NEW.source_ref !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='monthly activity requires the selected real-game source policy'; END IF;
 SELECT * INTO participation FROM app.game_participation WHERE id=NEW.source_ref::uuid FOR SHARE;
 IF participation.id IS NULL OR participation.account_id<>NEW.account_id OR NOT participation.real_game
 OR participation.own_turns<3 OR NEW.utc_month<>to_char(participation.completed_at AT TIME ZONE 'UTC','YYYY-MM') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='monthly activity must bind its account and real-game completion month'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER monthly_activity_source BEFORE INSERT ON app.monthly_account_activity
 FOR EACH ROW EXECUTE FUNCTION app.check_monthly_activity_source();
CREATE TRIGGER monthly_activity_history BEFORE UPDATE OR DELETE ON app.monthly_account_activity
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

-- Idempotent normalized participation consumer; host fact production and the
-- cutoff-consistent seasonal population snapshot remain separate obligations.
CREATE FUNCTION app.record_monthly_game_activity(p_participation uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE participation app.game_participation; result_id uuid; month_key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='monthly activity recording requires read committed retry'; END IF;
 SELECT * INTO participation FROM app.game_participation WHERE id=p_participation FOR SHARE;
 IF participation.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='monthly activity participation is missing'; END IF;
 IF NOT participation.real_game OR participation.own_turns<3 THEN RETURN NULL; END IF;
 month_key:=to_char(participation.completed_at AT TIME ZONE 'UTC','YYYY-MM');
 INSERT INTO app.monthly_account_activity(account_id,utc_month,activity_policy_ref,source_ref)
 VALUES(participation.account_id,month_key,'monthly-real-game-v1',participation.id::text)
 ON CONFLICT(account_id,utc_month,activity_policy_ref) DO NOTHING;
 SELECT id INTO STRICT result_id FROM app.monthly_account_activity
 WHERE account_id=participation.account_id AND utc_month=month_key AND activity_policy_ref='monthly-real-game-v1';
 RETURN result_id;
END $$;
REVOKE ALL ON FUNCTION app.check_monthly_activity_source(),app.record_monthly_game_activity(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
