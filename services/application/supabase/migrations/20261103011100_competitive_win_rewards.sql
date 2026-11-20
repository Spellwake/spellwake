BEGIN;
-- Initial Spellwake daily-win policy, fixed for this version. Coin-free win
-- slots remain wins; later versions must not rewrite accepted v1 receipts.
CREATE FUNCTION app.daily_win_cumulative_v1(p_wins numeric) RETURNS numeric
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT CASE WHEN p_wins=0 THEN 0 WHEN p_wins=1 THEN 250 WHEN p_wins=2 THEN 350
 WHEN p_wins=3 THEN 450 WHEN p_wins<6 THEN 550 WHEN p_wins<8 THEN 600
 WHEN p_wins<10 THEN 650 WHEN p_wins<12 THEN 700 WHEN p_wins<14 THEN 725 ELSE 750 END::numeric
$$;
ALTER TABLE app.daily_win_bucket ADD CONSTRAINT daily_win_issued_schedule_v1 CHECK(issued_coins=app.daily_win_cumulative_v1(wins));
CREATE FUNCTION app.guard_daily_win_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.wins<>0 OR NEW.issued_coins<>0 OR NEW.revision<>0 THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='daily win history requires its original empty bucket'; END IF;
  RETURN NEW;
 END IF;
 IF TG_OP='DELETE' OR (NEW.id,NEW.account_id,NEW.utc_day) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.utc_day)
 OR (NEW IS DISTINCT FROM OLD AND (NEW.wins<>OLD.wins+1 OR NEW.revision<>OLD.revision+1)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='daily win history cannot be erased or rewritten'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER daily_win_history BEFORE INSERT OR UPDATE OR DELETE ON app.daily_win_bucket
 FOR EACH ROW EXECUTE FUNCTION app.guard_daily_win_history();

CREATE FUNCTION app.consume_competitive_win_v1(p_credit uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE credit app.competitive_result; receipt app.command_receipt; bucket app.daily_win_bucket;
 owner uuid; coin uuid; inventory uuid; operation uuid; reward numeric; day_key text; fingerprint text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='win reward consumption requires read committed retry'; END IF;
 SELECT * INTO credit FROM app.competitive_result WHERE id=p_credit;
 IF credit.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='competitive credit is missing'; END IF;
 day_key:=to_char(credit.completed_at AT TIME ZONE 'UTC','YYYY-MM-DD');
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('credit',credit.id,'account',credit.account_id,'day',day_key,'win',credit.win,'policy','daily-win-v1')::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('rewards:'||credit.account_id::text,credit.id::text,'competitive_win_v1',credit.id::text,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='rewards:'||credit.account_id::text AND scope='competitive_win_v1' AND request_ref=credit.id::text FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>credit.id::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive reward source terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='competitive reward disposition is pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 IF NOT credit.win THEN
  RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',credit.id,'counted','false','coins','0','day',day_key)); END IF;
 PERFORM id FROM app.account WHERE id=credit.account_id AND state IN ('active','deletion_pending') FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reward account cannot receive committed game proceeds'; END IF;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=credit.account_id AND state='active';
 PERFORM app.lock_asset_account_for_write(owner);
 INSERT INTO app.daily_win_bucket(account_id,utc_day,wins,issued_coins,revision) VALUES(credit.account_id,day_key,0,0,0)
 ON CONFLICT(account_id,utc_day) DO NOTHING;
 SELECT * INTO STRICT bucket FROM app.daily_win_bucket WHERE account_id=credit.account_id AND utc_day=day_key FOR NO KEY UPDATE;
 reward:=app.daily_win_cumulative_v1(bucket.wins+1)-bucket.issued_coins;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('competitive_win',credit.id::text,'competitive_win_v1',credit.account_id,clock_timestamp(),'pending') RETURNING id INTO operation;
 IF reward>0 THEN
  SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';
  inventory:=app.ensure_inventory(owner,coin);
  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
  VALUES(operation,inventory,'credit',reward,'competitive_win_v1',credit.id::text);
 END IF;
 UPDATE app.daily_win_bucket SET wins=wins+1,issued_coins=app.daily_win_cumulative_v1(wins+1),revision=revision+1 WHERE id=bucket.id;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',credit.id,'counted','true','coins',reward::text,'day',day_key,'winIndex',(bucket.wins+1)::text,'operationId',operation));
END $$;

CREATE FUNCTION app.guard_competitive_human_roster() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_TABLE_NAME='match' THEN
  IF TG_OP='UPDATE' AND NEW.competitive IS NOT DISTINCT FROM OLD.competitive THEN RETURN NULL; END IF;
  targets:=ARRAY[NEW.id];
 ELSE
  IF TG_OP='UPDATE' AND (NEW.match_id,NEW.controller_kind) IS NOT DISTINCT FROM (OLD.match_id,OLD.controller_kind) THEN RETURN NULL; END IF;
  targets:=ARRAY[NEW.match_id];
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='competitive roster changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(targets) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id WHERE m.id=ANY(targets) AND m.competitive AND seat.controller_kind<>'human') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='competitive matches require human participants'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER competitive_human_match AFTER INSERT OR UPDATE ON app.match
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_competitive_human_roster();
CREATE CONSTRAINT TRIGGER competitive_human_seat AFTER INSERT OR UPDATE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_competitive_human_roster();
REVOKE ALL ON FUNCTION app.daily_win_cumulative_v1(numeric),app.guard_daily_win_history(),app.consume_competitive_win_v1(uuid),app.guard_competitive_human_roster()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
