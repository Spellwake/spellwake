-- Persist Stripe Customer ID on app.account for Stripe Billing Portal sessions.
BEGIN;

ALTER TABLE app.account
  ADD COLUMN IF NOT EXISTS stripe_customer_id text
    CHECK (stripe_customer_id IS NULL OR stripe_customer_id ~ '^cus_[A-Za-z0-9]{4,128}$');

CREATE OR REPLACE FUNCTION app.read_my_stripe_customer() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  cus text;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT stripe_customer_id INTO cus FROM app.account WHERE id = actor;
  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object('accountId', actor, 'stripeCustomerId', cus)
  );
END $$;

CREATE OR REPLACE FUNCTION app.record_account_stripe_customer(
  p_account uuid,
  p_customer_id text
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
BEGIN
  IF p_account IS NULL OR p_customer_id IS NULL OR p_customer_id !~ '^cus_[A-Za-z0-9]{4,128}$' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('customer', 'invalid'));
  END IF;
  UPDATE app.account
  SET stripe_customer_id = p_customer_id
  WHERE id = p_account AND state = 'active' AND stripe_customer_id IS DISTINCT FROM p_customer_id;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('accountId', p_account, 'stripeCustomerId', p_customer_id));
END $$;

REVOKE ALL ON FUNCTION
  app.read_my_stripe_customer(),
  app.record_account_stripe_customer(uuid, text)
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

GRANT EXECUTE ON FUNCTION app.read_my_stripe_customer() TO authenticated;
GRANT EXECUTE ON FUNCTION app.record_account_stripe_customer(uuid, text) TO service_role;

COMMIT;
