BEGIN;

ALTER TABLE app.ante_commitment ADD CONSTRAINT ante_match_scope UNIQUE(id,match_id);
ALTER TABLE app.ai_loss_exposure ADD CONSTRAINT exposure_commitment_match
 FOREIGN KEY(commitment_id,match_id) REFERENCES app.ante_commitment(id,match_id)
 DEFERRABLE INITIALLY IMMEDIATE;
ALTER TABLE app.ai_loss_exposure ADD CONSTRAINT exposure_state CHECK(state IN ('reserved','settled','released'));

CREATE FUNCTION app.guard_ai_loss_bucket() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.lost_coins<>0 OR NEW.revision<>0 THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI allowance starts without historical losses'; END IF;
  RETURN NEW;
 END IF;
 IF TG_OP='DELETE' THEN
  IF OLD.lost_coins<>0 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure WHERE bucket_id=OLD.id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI allowance history cannot be erased'; END IF;
  RETURN OLD;
 END IF;
 IF (NEW.id,NEW.sponsor_id,NEW.utc_day,NEW.limit_coins) IS DISTINCT FROM (OLD.id,OLD.sponsor_id,OLD.utc_day,OLD.limit_coins)
 OR NEW.lost_coins<OLD.lost_coins OR NEW.revision<OLD.revision
 OR (NEW.lost_coins<>OLD.lost_coins AND NEW.revision<=OLD.revision) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI allowance identity and losses are monotonic'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER ai_bucket_identity BEFORE INSERT OR UPDATE OR DELETE ON app.ai_loss_bucket
 FOR EACH ROW EXECUTE FUNCTION app.guard_ai_loss_bucket();

CREATE FUNCTION app.guard_ai_loss_exposure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE sponsor uuid; lifecycle text; terms_state text;
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI exposure history cannot be erased'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='AI exposure changes require read committed retry'; END IF;
 IF TG_OP='UPDATE' AND ((NEW.id,NEW.bucket_id,NEW.match_id,NEW.commitment_id) IS DISTINCT FROM (OLD.id,OLD.bucket_id,OLD.match_id,OLD.commitment_id)
  OR (OLD.state<>'reserved' AND NEW IS DISTINCT FROM OLD)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI exposure identity and terminal history are immutable'; END IF;
 IF TG_OP='INSERT' AND NEW.state<>'reserved' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI exposure requires an original reservation'; END IF;
 SELECT sponsor_id INTO STRICT sponsor FROM app.ai_loss_bucket WHERE id=NEW.bucket_id;
 SELECT state INTO STRICT lifecycle FROM app.account WHERE id=sponsor FOR NO KEY UPDATE;
 IF lifecycle='deleted' OR (NEW.state='reserved' AND lifecycle<>'active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot reserve AI allowance'; END IF;
 PERFORM id FROM app.match WHERE id=NEW.match_id FOR NO KEY UPDATE;
 SELECT state INTO terms_state FROM app.ante_commitment WHERE id=NEW.commitment_id AND match_id=NEW.match_id FOR SHARE;
 IF terms_state IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI exposure must bind its ante match'; END IF;
 IF TG_OP='INSERT' AND terms_state<>'offered' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI reservations must precede ante commitment'; END IF;
 IF TG_OP='UPDATE' AND NEW.reserved_coins<>OLD.reserved_coins AND terms_state<>'offered' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed AI stakes cannot be topped up'; END IF;
 -- Persist a real shared-row change so a competing stale snapshot cannot reserve against old totals.
 UPDATE app.ai_loss_bucket SET revision=revision+1 WHERE id=NEW.bucket_id;
 RETURN NEW;
END $$;
CREATE TRIGGER ai_exposure_identity BEFORE INSERT OR UPDATE OR DELETE ON app.ai_loss_exposure
 FOR EACH ROW EXECUTE FUNCTION app.guard_ai_loss_exposure();

CREATE FUNCTION app.check_ai_allowance() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE bucket uuid; target_match uuid;
BEGIN
 IF TG_TABLE_NAME='ai_loss_bucket' THEN bucket:=NEW.id;
 ELSE bucket:=NEW.bucket_id; target_match:=NEW.match_id; END IF;
 IF EXISTS(SELECT 1 FROM app.ai_loss_bucket b WHERE b.id=bucket AND b.lost_coins+
  coalesce((SELECT sum(reserved_coins) FROM app.ai_loss_exposure WHERE bucket_id=b.id AND state='reserved'),0)>b.limit_coins) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI daily allowance is already spent or reserved'; END IF;
 IF target_match IS NOT NULL AND EXISTS(
  SELECT b.sponsor_id FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id
   WHERE x.match_id=target_match AND x.state='reserved' GROUP BY b.sponsor_id HAVING sum(x.reserved_coins)>300) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='combined sponsor AI stakes exceed the match allowance'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ai_daily_allowance AFTER INSERT OR UPDATE ON app.ai_loss_bucket
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_ai_allowance();
CREATE CONSTRAINT TRIGGER ai_match_allowance AFTER INSERT OR UPDATE ON app.ai_loss_exposure
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.check_ai_allowance();

REVOKE ALL ON FUNCTION app.guard_ai_loss_bucket(),app.guard_ai_loss_exposure(),app.check_ai_allowance()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
