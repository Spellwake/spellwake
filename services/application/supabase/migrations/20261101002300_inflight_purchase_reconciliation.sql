BEGIN;

-- New purchases require active accounts. Already-created unknown/pending
-- checkouts must still persist learned provider refs while deletion waits.
CREATE OR REPLACE FUNCTION app.guard_purchase_proof() RETURNS trigger
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
  OR (OLD.state='paid' AND NEW.state NOT IN ('paid','refunded','reversed'))
  OR (OLD.state='canceled' AND NEW.state NOT IN ('canceled','paid','refunded','reversed')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase source or terminal disposition cannot change';
  END IF;
 END IF;
 IF NEW.state IN ('paid','refunded','reversed') THEN
  PERFORM app.check_provider_source(NEW.provider_event_id,NEW.account_id,'stripe');
 END IF;
 IF TG_OP='INSERT' AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account cannot start a purchase';
 END IF;
 RETURN NEW;
END $$;
COMMIT;
