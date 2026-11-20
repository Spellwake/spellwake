BEGIN;

ALTER TABLE app.cosmetic ADD COLUMN is_free boolean NOT NULL DEFAULT false;
ALTER TABLE app.cosmetic ADD CONSTRAINT cosmetic_kind CHECK (kind IN ('back','sleeve','dice','playmat'));
ALTER TABLE app.cosmetic ADD CONSTRAINT cosmetic_state CHECK (state IN ('active','inactive'));
ALTER TABLE app.cosmetic ADD CONSTRAINT no_starter_playmat CHECK (NOT is_free OR kind<>'playmat');
ALTER TABLE app.cosmetic_grant ADD CONSTRAINT cosmetic_grant_state CHECK (state IN ('active','revoked'));
ALTER TABLE app.patreon_payment ADD CONSTRAINT patreon_payment_state CHECK (state IN ('paid','refunded','reversed'));
ALTER TABLE app.patreon_membership ADD CONSTRAINT membership_active_tier CHECK (active=(active_tier_id IS NOT NULL));
CREATE UNIQUE INDEX patreon_external_member ON app.patreon_membership(patreon_member_ref);
ALTER TABLE app.account ADD COLUMN appearance_revision numeric NOT NULL DEFAULT 0
 CHECK (appearance_revision>=0 AND appearance_revision<power(10::numeric,78) AND scale(appearance_revision)=0);
CREATE INDEX entitlement_lookup ON app.cosmetic_grant(account_id,cosmetic_id) WHERE state='active';

-- Cosmetic kind is identity: a saved sleeve must never become a different slot.
CREATE FUNCTION app.guard_cosmetic_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.kind,NEW.is_free) IS DISTINCT FROM (OLD.id,OLD.kind,OLD.is_free) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cosmetic slot and free eligibility are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER cosmetic_identity BEFORE UPDATE ON app.cosmetic
 FOR EACH ROW EXECUTE FUNCTION app.guard_cosmetic_identity();

CREATE FUNCTION app.free_cosmetic(p_actor uuid,p_kind text) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT c.id FROM app.cosmetic c JOIN app.cosmetic_grant g ON g.cosmetic_id=c.id
 WHERE c.kind=p_kind AND c.is_free AND c.state='active' AND g.account_id=p_actor
 AND g.source_kind='free' AND g.state='active' ORDER BY c.id LIMIT 1
$$;

CREATE FUNCTION app.resolve_cosmetic(p_actor uuid,p_selected uuid,p_kind text) RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE replacement uuid;
BEGIN
 IF p_kind NOT IN ('back','sleeve','dice','playmat') OR p_kind IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid appearance slot';
 END IF;
 IF app.has_cosmetic(p_actor,p_selected,p_kind) THEN RETURN p_selected; END IF;
 replacement:=app.free_cosmetic(p_actor,p_kind);
 IF replacement IS NULL AND p_kind<>'playmat' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='free cosmetic is not configured for account';
 END IF;
 RETURN replacement;
END $$;

-- The trusted host will supply accepted appearance IDs, independent of gameplay
-- snapshots. Every next projection resolves current access, never a cached grant.
CREATE FUNCTION app.resolve_account_appearance(p_actor uuid,p_back uuid,p_sleeve uuid,p_dice uuid,p_playmat uuid)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='appearance account unavailable';
 END IF;
 RETURN jsonb_build_object('backId',app.resolve_cosmetic(p_actor,p_back,'back'),
  'sleeveId',app.resolve_cosmetic(p_actor,p_sleeve,'sleeve'),'diceId',app.resolve_cosmetic(p_actor,p_dice,'dice'),
  'playmatId',app.resolve_cosmetic(p_actor,p_playmat,'playmat'));
END $$;
GRANT EXECUTE ON FUNCTION app.resolve_account_appearance(uuid,uuid,uuid,uuid,uuid) TO service_role;

CREATE FUNCTION app.guard_cosmetic_grant() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lifecycle text;
BEGIN
 IF TG_OP='DELETE' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cosmetic grant provenance cannot be deleted';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF (NEW.id,NEW.account_id,NEW.cosmetic_id,NEW.source_kind,NEW.purchase_id,NEW.patreon_payment_id)
  IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.cosmetic_id,OLD.source_kind,OLD.purchase_id,OLD.patreon_payment_id)
  OR (OLD.state='revoked' AND NEW.state<>'revoked') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cosmetic grant provenance or revocation is immutable';
  END IF;
 END IF;
 SELECT state INTO STRICT lifecycle FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 -- Real row changes invalidate a stale REPEATABLE READ entitlement snapshot.
 UPDATE app.account SET appearance_revision=appearance_revision+1 WHERE id=NEW.account_id;
 IF NEW.state='active' AND lifecycle='deleted' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deleted account cannot receive cosmetics';
 END IF;
 IF NEW.state='revoked' AND lifecycle<>'deleted' AND (
 (NEW.source_kind='stripe_purchase' AND NOT EXISTS(SELECT 1 FROM app.purchase WHERE id=NEW.purchase_id AND state IN ('refunded','reversed')))
 OR (NEW.source_kind='patreon_payment' AND NOT EXISTS(SELECT 1 FROM app.patreon_payment WHERE id=NEW.patreon_payment_id AND state IN ('refunded','reversed')))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='revocation requires reversed source payment';
 END IF;
 IF NEW.source_kind='free' THEN
  IF NEW.state<>'active' AND lifecycle<>'deleted' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='free entitlement cannot be revoked';
  END IF;
  IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=NEW.cosmetic_id AND is_free) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cosmetic is not a configured free choice';
  END IF;
 ELSIF NEW.state='active' AND NEW.source_kind='stripe_purchase' THEN
  IF NOT EXISTS(SELECT 1 FROM app.purchase WHERE id=NEW.purchase_id AND account_id=NEW.account_id
   AND cosmetic_id=NEW.cosmetic_id AND state='paid') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant requires matching paid purchase';
  END IF;
 ELSIF NEW.state='active' AND NEW.source_kind='patreon_payment' THEN
  IF NOT EXISTS(SELECT 1 FROM app.patreon_payment p JOIN app.patreon_membership m ON m.id=p.membership_id
   JOIN app.tier_cosmetic t ON t.tier_id=p.tier_id AND t.cosmetic_id=NEW.cosmetic_id
   WHERE p.id=NEW.patreon_payment_id AND m.account_id=NEW.account_id AND p.state='paid') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='grant requires matching paid tier';
  END IF;
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER cosmetic_grant_source BEFORE INSERT OR UPDATE OR DELETE ON app.cosmetic_grant
 FOR EACH ROW EXECUTE FUNCTION app.guard_cosmetic_grant();

CREATE FUNCTION app.repair_revoked_cosmetic() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE slot text; replacement uuid;
BEGIN
 IF NEW.state<>'revoked' OR OLD.state='revoked' OR
 EXISTS(SELECT 1 FROM app.cosmetic_grant WHERE account_id=NEW.account_id AND cosmetic_id=NEW.cosmetic_id AND state='active') THEN
  RETURN NULL;
 END IF;
 IF EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='deleted') THEN RETURN NULL; END IF;
 SELECT kind INTO STRICT slot FROM app.cosmetic WHERE id=NEW.cosmetic_id;
 -- Resolve only when saved state uses this item; the host resolves stale accepted
 -- IDs independently on its next projection, without modifying rules snapshots.
 IF slot='back' THEN
  IF EXISTS(SELECT 1 FROM app.profile WHERE account_id=NEW.account_id AND default_back_id=NEW.cosmetic_id)
  OR EXISTS(SELECT 1 FROM app.deck WHERE account_id=NEW.account_id AND back_id=NEW.cosmetic_id) THEN
   replacement:=app.resolve_cosmetic(NEW.account_id,NEW.cosmetic_id,slot);
   UPDATE app.profile SET default_back_id=replacement WHERE account_id=NEW.account_id AND default_back_id=NEW.cosmetic_id;
   UPDATE app.deck SET back_id=replacement WHERE account_id=NEW.account_id AND back_id=NEW.cosmetic_id;
  END IF;
 ELSIF slot='sleeve' THEN
  IF EXISTS(SELECT 1 FROM app.profile WHERE account_id=NEW.account_id AND default_sleeve_id=NEW.cosmetic_id)
  OR EXISTS(SELECT 1 FROM app.deck WHERE account_id=NEW.account_id AND sleeve_id=NEW.cosmetic_id) THEN
   replacement:=app.resolve_cosmetic(NEW.account_id,NEW.cosmetic_id,slot);
   UPDATE app.profile SET default_sleeve_id=replacement WHERE account_id=NEW.account_id AND default_sleeve_id=NEW.cosmetic_id;
   UPDATE app.deck SET sleeve_id=replacement WHERE account_id=NEW.account_id AND sleeve_id=NEW.cosmetic_id;
  END IF;
 ELSIF slot='dice' THEN
  IF EXISTS(SELECT 1 FROM app.profile WHERE account_id=NEW.account_id AND default_dice_id=NEW.cosmetic_id)
  OR EXISTS(SELECT 1 FROM app.deck WHERE account_id=NEW.account_id AND dice_id=NEW.cosmetic_id) THEN
   replacement:=app.resolve_cosmetic(NEW.account_id,NEW.cosmetic_id,slot);
   UPDATE app.profile SET default_dice_id=replacement WHERE account_id=NEW.account_id AND default_dice_id=NEW.cosmetic_id;
   UPDATE app.deck SET dice_id=replacement WHERE account_id=NEW.account_id AND dice_id=NEW.cosmetic_id;
  END IF;
 ELSE
  UPDATE app.profile SET default_playmat_id=NULL WHERE account_id=NEW.account_id AND default_playmat_id=NEW.cosmetic_id;
  UPDATE app.deck SET playmat_id=NULL WHERE account_id=NEW.account_id AND playmat_id=NEW.cosmetic_id;
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER cosmetic_grant_replacement AFTER UPDATE ON app.cosmetic_grant
 FOR EACH ROW EXECUTE FUNCTION app.repair_revoked_cosmetic();

-- These are normalized provider proofs. Actual signature verification, event
-- collection and provider reconciliation must precede writing them in the API.
CREATE FUNCTION app.check_provider_source(p_event uuid,p_account uuid,p_provider text) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.provider_event WHERE id=p_event AND account_id=p_account
 AND provider=p_provider AND processing_state IN ('verified','processed')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='verified provider source does not match account';
 END IF;
END $$;

CREATE FUNCTION app.guard_purchase_proof() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase proof cannot be deleted'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF TG_OP='UPDATE' THEN
  IF (NEW.id,NEW.account_id,NEW.cosmetic_id,NEW.provider_request_ref,NEW.requested_at)
  IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.cosmetic_id,OLD.provider_request_ref,OLD.requested_at)
  OR (OLD.stripe_payment_ref IS NOT NULL AND NEW.stripe_payment_ref IS DISTINCT FROM OLD.stripe_payment_ref)
  OR (OLD.stripe_checkout_ref IS NOT NULL AND NEW.stripe_checkout_ref IS DISTINCT FROM OLD.stripe_checkout_ref)
  OR (OLD.state IN ('refunded','reversed') AND NEW.state IS DISTINCT FROM OLD.state)
  OR (OLD.state='paid' AND NEW.state NOT IN ('paid','refunded','reversed')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase source or terminal disposition cannot change';
  END IF;
 END IF;
 IF NEW.state IN ('paid','refunded','reversed') THEN
  PERFORM app.check_provider_source(NEW.provider_event_id,NEW.account_id,'stripe');
 END IF;
 IF NEW.state IN ('creating','pending') AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot start a purchase';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER purchase_proof BEFORE INSERT OR UPDATE OR DELETE ON app.purchase
 FOR EACH ROW EXECUTE FUNCTION app.guard_purchase_proof();

CREATE FUNCTION app.apply_purchase_cosmetics() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NEW.state='paid' AND (TG_OP='INSERT' OR OLD.state<>'paid') AND EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state<>'deleted') THEN
  INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,purchase_id,state)
  VALUES(NEW.account_id,NEW.cosmetic_id,'stripe_purchase',NEW.id,'active') ON CONFLICT DO NOTHING;
 ELSIF NEW.state IN ('refunded','reversed') THEN
  UPDATE app.cosmetic_grant SET state='revoked' WHERE purchase_id=NEW.id AND state='active';
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER purchase_entitlement AFTER INSERT OR UPDATE ON app.purchase
 FOR EACH ROW EXECUTE FUNCTION app.apply_purchase_cosmetics();

CREATE FUNCTION app.guard_membership_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.account_id,NEW.patreon_member_ref) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.patreon_member_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='membership association is immutable';
 END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(NEW.source_event_id,NEW.account_id,'patreon');
 IF NEW.active AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot activate a membership';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER membership_source BEFORE INSERT OR UPDATE ON app.patreon_membership
 FOR EACH ROW EXECUTE FUNCTION app.guard_membership_source();

CREATE FUNCTION app.guard_patreon_payment() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE owner_account uuid;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment proof cannot be deleted'; END IF;
 SELECT account_id INTO STRICT owner_account FROM app.patreon_membership WHERE id=NEW.membership_id;
 PERFORM id FROM app.account WHERE id=owner_account FOR NO KEY UPDATE;
 IF TG_OP='UPDATE' THEN
  IF (NEW.id,NEW.membership_id,NEW.tier_id,NEW.payment_ref,NEW.paid_at)
  IS DISTINCT FROM (OLD.id,OLD.membership_id,OLD.tier_id,OLD.payment_ref,OLD.paid_at)
  OR (OLD.state IN ('refunded','reversed') AND NEW.state IS DISTINCT FROM OLD.state) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon payment source or reversal cannot change';
  END IF;
 END IF;
 PERFORM app.check_provider_source(NEW.provider_event_id,owner_account,'patreon');
 RETURN NEW;
END $$;
CREATE TRIGGER patreon_payment_proof BEFORE INSERT OR UPDATE OR DELETE ON app.patreon_payment
 FOR EACH ROW EXECUTE FUNCTION app.guard_patreon_payment();

CREATE FUNCTION app.apply_patreon_cosmetics() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE owner_account uuid;
BEGIN
 SELECT account_id INTO STRICT owner_account FROM app.patreon_membership WHERE id=NEW.membership_id;
 IF NEW.state='paid' AND (TG_OP='INSERT' OR OLD.state<>'paid') AND EXISTS(SELECT 1 FROM app.account WHERE id=owner_account AND state<>'deleted') THEN
  INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,patreon_payment_id,state)
  SELECT owner_account,cosmetic_id,'patreon_payment',NEW.id,'active' FROM app.tier_cosmetic WHERE tier_id=NEW.tier_id
  ON CONFLICT DO NOTHING;
 ELSIF NEW.state IN ('refunded','reversed') THEN
  UPDATE app.cosmetic_grant SET state='revoked' WHERE patreon_payment_id=NEW.id AND state='active';
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER patreon_entitlement AFTER INSERT OR UPDATE ON app.patreon_payment
 FOR EACH ROW EXECUTE FUNCTION app.apply_patreon_cosmetics();

CREATE FUNCTION app.active_supporter_tier(p_account uuid) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT m.active_tier_id FROM app.patreon_membership m JOIN app.account a ON a.id=m.account_id
 WHERE m.account_id=p_account AND m.active AND a.state='active'
$$;
GRANT EXECUTE ON FUNCTION app.active_supporter_tier(uuid) TO service_role;

-- Explicit deck cosmetics do not mutate the immutable saved card contents.
CREATE FUNCTION app.change_deck_appearance(p_actor uuid,p_deck uuid,p_back uuid,p_sleeve uuid,p_dice uuid,p_playmat uuid,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'deck_appearance',p_deck::text,
  jsonb_build_object('back',p_back,'sleeve',p_sleeve,'dice',p_dice,'playmat',p_playmat));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF NOT app.has_cosmetic(p_actor,p_back,'back') OR NOT app.has_cosmetic(p_actor,p_sleeve,'sleeve')
 OR NOT app.has_cosmetic(p_actor,p_dice,'dice') OR (p_playmat IS NOT NULL AND NOT app.has_cosmetic(p_actor,p_playmat,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','cosmetic selection unavailable'));
 END IF;
 UPDATE app.deck SET back_id=p_back,sleeve_id=p_sleeve,dice_id=p_dice,playmat_id=p_playmat
 WHERE id=p_deck AND account_id=p_actor AND source_kind='account';
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','deck unavailable')); END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_deck));
END $$;
GRANT EXECUTE ON FUNCTION app.change_deck_appearance(uuid,uuid,uuid,uuid,uuid,uuid,uuid) TO service_role;
GRANT SELECT ON app.purchase,app.patreon_membership,app.patreon_payment,app.supporter_tier,app.tier_cosmetic TO authenticated;
CREATE POLICY own_purchase_read ON app.purchase FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY own_membership_read ON app.patreon_membership FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY own_payment_read ON app.patreon_payment FOR SELECT TO authenticated USING
 (EXISTS(SELECT 1 FROM app.patreon_membership m WHERE m.id=membership_id AND m.account_id=app.current_account_id()));
CREATE POLICY tier_catalogue_read ON app.supporter_tier FOR SELECT TO authenticated USING (true);
CREATE POLICY tier_cosmetic_catalogue_read ON app.tier_cosmetic FOR SELECT TO authenticated USING (true);

COMMIT;
