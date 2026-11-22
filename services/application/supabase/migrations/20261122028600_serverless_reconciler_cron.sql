-- Zero-cost serverless background reconciliation via Supabase pg_cron:
-- 1. Prediction market maintenance runs directly inside Postgres (never wakes Fly).
-- 2. Stripe, Patreon, and account deletion work is checked via cheap SQL queries;
--    Fly's scale-to-zero spellwake-api is woken via net.http_post ONLY when real
--    pending rows exist, authenticated via Supabase Vault secret
--    'spellwake_reconcile_secret'.
BEGIN;

CREATE OR REPLACE FUNCTION app.sweep_prediction_maintenance(p_limit integer DEFAULT 50) RETURNS integer
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  page jsonb;
  market_id uuid;
  processed integer := 0;
BEGIN
  IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RETURN 0;
  END IF;
  page := app.read_prediction_maintenance_page(NULL, p_limit);
  IF page IS NULL OR jsonb_typeof(page->'items') <> 'array' THEN
    RETURN 0;
  END IF;
  FOR market_id IN
    SELECT value::text::uuid
    FROM jsonb_array_elements_text(page->'items') AS t(value)
  LOOP
    PERFORM app.maintain_prediction_market(market_id);
    processed := processed + 1;
  END LOOP;
  RETURN processed;
END $$;

CREATE OR REPLACE FUNCTION app.enqueue_background_reconcile_wake() RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  due_domains text[] := ARRAY[]::text[];
  secret_value text;
BEGIN
  -- Always run prediction maintenance directly inside Postgres (zero external wake).
  PERFORM app.sweep_prediction_maintenance(50);

  IF jsonb_array_length(app.read_stripe_work('purchase', NULL, 1, 'spellwake-payment-evidence')) > 0
     OR jsonb_array_length(app.read_stripe_work('event', NULL, 1, 'spellwake-payment-evidence')) > 0
     OR jsonb_array_length(app.read_stripe_work('evidence', NULL, 1, 'spellwake-payment-evidence')) > 0 THEN
    due_domains := array_append(due_domains, 'stripe');
  END IF;

  IF jsonb_array_length(app.read_patreon_work('evidence', NULL, 1, 'spellwake-patreon-evidence')) > 0 THEN
    due_domains := array_append(due_domains, 'patreon');
  END IF;

  IF jsonb_array_length(COALESCE(app.read_deletion_account_page(NULL, 1)->'items', '[]'::jsonb)) > 0 THEN
    due_domains := array_append(due_domains, 'deletion');
  END IF;

  -- When zero external work is queued, do NOT wake the scale-to-zero Fly app.
  IF cardinality(due_domains) = 0 THEN
    RETURN NULL;
  END IF;

  SELECT decrypted_secret INTO secret_value
  FROM vault.decrypted_secrets
  WHERE name = 'spellwake_reconcile_secret'
  LIMIT 1;

  IF secret_value IS NULL OR length(secret_value) < 32 THEN
    RETURN NULL;
  END IF;

  RETURN net.http_post(
    url := 'https://spellwake-api.fly.dev/v1/internal/reconcile',
    headers := jsonb_build_object(
      'Content-Type', 'application/json',
      'x-spellwake-reconcile', secret_value
    ),
    body := jsonb_build_object('domains', to_jsonb(due_domains)),
    timeout_milliseconds := 25000
  );
END $$;

REVOKE ALL ON FUNCTION
  app.sweep_prediction_maintenance(integer),
  app.enqueue_background_reconcile_wake()
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

DO $schedule_reconcile$
BEGIN
  IF to_regclass('cron.job') IS NOT NULL AND to_regprocedure('cron.schedule(text,text,text)') IS NOT NULL THEN
    PERFORM cron.schedule(
      'spellwake-background-reconcile',
      '30 seconds',
      $cmd$SELECT app.enqueue_background_reconcile_wake();$cmd$
    );
  END IF;
END $schedule_reconcile$;

COMMIT;
