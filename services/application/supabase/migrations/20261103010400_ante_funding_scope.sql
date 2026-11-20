BEGIN;

-- These are relational reservation guards. Accepted terms, valuation evidence and
-- terminal allocation still belong to the canonical ante transactions.
CREATE FUNCTION app.check_ante_funding_scope(p_matches uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante funding changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(p_matches) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(
  SELECT 1 FROM app.ante_stake s
  JOIN app.ante_commitment c ON c.id=s.commitment_id
  JOIN app.match_seat seat ON seat.id=s.match_seat_id
  JOIN app.match m ON m.id=c.match_id
  LEFT JOIN app.asset_hold h ON h.id=s.hold_id
  LEFT JOIN app.asset_owner owner ON owner.id=h.owner_id
  LEFT JOIN app.ai_loss_exposure x ON x.id=s.ai_exposure_id
  LEFT JOIN app.ai_loss_bucket b ON b.id=x.bucket_id
  WHERE (c.match_id=ANY(p_matches) OR seat.match_id=ANY(p_matches)) AND
   (seat.match_id<>c.match_id OR m.mode<>'collection' OR s.estimated_value_coins<=0
    OR (s.funding_kind='owned_assets' AND
      (seat.controller_kind<>'human' OR h.id IS NULL OR owner.account_id IS DISTINCT FROM seat.account_id
       OR h.purpose<>'ante' OR h.commitment_ref<>c.id::text))
    OR (s.funding_kind='ai_loss_allowance' AND
      (seat.controller_kind<>'ai' OR x.id IS NULL OR x.commitment_id<>c.id
       OR x.match_id<>c.match_id OR b.sponsor_id IS DISTINCT FROM seat.sponsor_id)))
 ) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante stake must bind its match seat and funding source'; END IF;
 IF EXISTS(
  SELECT 1 FROM app.ante_stake s JOIN app.ai_loss_exposure x ON x.id=s.ai_exposure_id
  WHERE x.match_id=ANY(p_matches)
  GROUP BY x.id,x.reserved_coins HAVING sum(s.estimated_value_coins)>x.reserved_coins
 ) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI seat stakes exceed their shared reserved exposure'; END IF;
END $$;

CREATE FUNCTION app.guard_ante_funding_scope() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[]; current_stake app.ante_stake; commitment_match uuid; seat_match uuid;
BEGIN
 IF TG_TABLE_NAME='ante_stake' THEN
  SELECT * INTO current_stake FROM app.ante_stake WHERE id=NEW.id;
  IF NOT FOUND THEN RETURN NULL; END IF;
  SELECT match_id INTO commitment_match FROM app.ante_commitment WHERE id=current_stake.commitment_id FOR SHARE;
  SELECT match_id INTO seat_match FROM app.match_seat WHERE id=current_stake.match_seat_id FOR SHARE;
  IF commitment_match IS NULL OR seat_match IS NULL THEN
   RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='ante funding parent is missing'; END IF;
  IF current_stake.hold_id IS NOT NULL THEN
   PERFORM id FROM app.asset_hold WHERE id=current_stake.hold_id FOR SHARE;
  ELSE
   PERFORM id FROM app.ai_loss_exposure WHERE id=current_stake.ai_exposure_id FOR SHARE;
  END IF;
  targets:=ARRAY[commitment_match,seat_match];
 ELSIF TG_TABLE_NAME='match_seat' THEN
  IF (NEW.match_id,NEW.account_id,NEW.sponsor_id,NEW.controller_kind) IS NOT DISTINCT FROM
     (OLD.match_id,OLD.account_id,OLD.sponsor_id,OLD.controller_kind) THEN RETURN NULL; END IF;
  targets:=ARRAY[OLD.match_id,NEW.match_id];
 ELSIF TG_TABLE_NAME='match' THEN
  IF NEW.mode IS NOT DISTINCT FROM OLD.mode THEN RETURN NULL; END IF;
  targets:=ARRAY[NEW.id];
 ELSIF TG_TABLE_NAME='ante_commitment' THEN
  targets:=ARRAY[OLD.match_id,NEW.match_id];
 ELSE targets:=ARRAY[NEW.match_id];
 END IF;
 PERFORM app.check_ante_funding_scope(targets);
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ante_stake_funding_scope AFTER INSERT OR UPDATE ON app.ante_stake
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_ante_funding_scope();
CREATE CONSTRAINT TRIGGER ante_seat_funding_scope AFTER UPDATE ON app.match_seat
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_ante_funding_scope();
CREATE CONSTRAINT TRIGGER ante_terms_funding_scope AFTER UPDATE ON app.ante_commitment
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_ante_funding_scope();
CREATE CONSTRAINT TRIGGER ante_match_funding_scope AFTER UPDATE ON app.match
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_ante_funding_scope();
CREATE CONSTRAINT TRIGGER ante_exposure_funding_scope AFTER UPDATE ON app.ai_loss_exposure
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_ante_funding_scope();

REVOKE ALL ON FUNCTION app.check_ante_funding_scope(uuid[]),app.guard_ante_funding_scope()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
