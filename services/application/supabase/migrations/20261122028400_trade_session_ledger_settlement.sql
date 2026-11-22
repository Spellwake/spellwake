-- Atomic double-entry ledger settlement for bilateral live trade sessions,
-- live balance/ownership validation on offer updates, and pausing finished
-- temporary pilot wake cron jobs.
BEGIN;

ALTER TABLE app.trade_session
  ADD COLUMN IF NOT EXISTS settlement_operation_id uuid REFERENCES app.ledger_operation(id),
  ADD COLUMN IF NOT EXISTS assessment jsonb;

-- Extend exchange_conservation to permit both accepted app.trade_offer and
-- completed app.trade_session as canonical trade_settlement domain sources,
-- while continuing to enforce strict zero-sum asset conservation.
CREATE OR REPLACE FUNCTION app.check_exchange_conservation()
 RETURNS trigger
 LANGUAGE plpgsql
 SET search_path TO ''
AS $function$
DECLARE kind text; state text;
BEGIN
 SELECT o.kind, o.state INTO kind, state FROM app.ledger_operation o WHERE id = NEW.id;
 IF state = 'committed' AND (
   (kind = 'trade_settlement' AND NOT EXISTS (
     SELECT 1 FROM app.trade_offer t JOIN app.ledger_operation o ON o.id = t.settlement_operation_id
     WHERE o.id = NEW.id AND t.state = 'accepted' AND o.source_scope = 'trade_settlement'
       AND o.source_ref = t.id::text AND o.actor_account_id = t.recipient_id
   ) AND NOT EXISTS (
     SELECT 1 FROM app.trade_session s JOIN app.ledger_operation o ON o.id = s.settlement_operation_id
     WHERE o.id = NEW.id AND s.state = 'completed' AND o.source_scope = 'trade_session_settlement'
       AND o.source_ref = s.id::text AND o.actor_account_id IN (s.initiator_id, s.counterparty_id)
   ))
   OR (kind = 'market_purchase' AND NOT EXISTS (
     SELECT 1 FROM app.market_listing l JOIN app.ledger_operation o ON o.id = l.settlement_operation_id
     WHERE o.id = NEW.id AND l.state = 'sold' AND o.source_scope = 'market_purchase'
       AND o.source_ref = l.id::text AND o.actor_account_id = l.buyer_id
   ))
 ) THEN
  RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'exchange operation requires its accepted domain source';
 END IF;
 IF kind IN ('trade_settlement', 'market_purchase') AND state = 'committed' AND (
   NOT EXISTS (SELECT 1 FROM app.ledger_posting WHERE operation_id = NEW.id)
   OR EXISTS (
     SELECT 1 FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id = p.inventory_asset_id
     WHERE p.operation_id = NEW.id GROUP BY i.asset_definition_id
     HAVING sum(CASE WHEN p.direction = 'debit' THEN -p.quantity ELSE p.quantity END) <> 0
   )
 ) THEN
  RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'exchange must conserve every asset definition';
 END IF;
 RETURN NULL;
END $function$;

CREATE OR REPLACE FUNCTION app.trade_session_bundle(p_session uuid, p_owner uuid, p_coins numeric) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  WITH coin_def AS (
    SELECT id FROM app.asset_definition WHERE kind = 'coin' AND currency_key = 'coin' LIMIT 1
  ),
  lines AS (
    SELECT i.asset_definition_id AS asset_id, i.quantity
    FROM app.trade_session_item i
    WHERE i.session_id = p_session AND i.owner_id = p_owner
    UNION ALL
    SELECT c.id AS asset_id, p_coins AS quantity
    FROM coin_def c
    WHERE p_coins IS NOT NULL AND p_coins > 0
  )
  SELECT COALESCE(
    jsonb_agg(jsonb_build_object('assetId', asset_id, 'quantity', quantity::text) ORDER BY asset_id),
    '[]'::jsonb
  )
  FROM lines
$$;

CREATE OR REPLACE FUNCTION app.trade_session_json(p_session app.trade_session) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT jsonb_build_object(
    'id', p_session.id,
    'state', p_session.state,
    'revision', p_session.revision::text,
    'initiator', app.account_identity_json(init_acc),
    'counterparty', app.account_identity_json(cp_acc),
    'initiatorReady', p_session.initiator_ready,
    'counterpartyReady', p_session.counterparty_ready,
    'initiatorConfirmed', p_session.initiator_confirmed,
    'counterpartyConfirmed', p_session.counterparty_confirmed,
    'initiatorCoins', p_session.initiator_coins::text,
    'counterpartyCoins', p_session.counterparty_coins::text,
    'initiatorItems', COALESCE((
      SELECT jsonb_agg(jsonb_build_object(
        'assetId', d.id, 'quantity', i.quantity::text, 'asset', app.asset_identity_json(d)
      ) ORDER BY d.id)
      FROM app.trade_session_item i
      JOIN app.asset_definition d ON d.id = i.asset_definition_id
      WHERE i.session_id = p_session.id AND i.owner_id = p_session.initiator_id
    ), '[]'::jsonb),
    'counterpartyItems', COALESCE((
      SELECT jsonb_agg(jsonb_build_object(
        'assetId', d.id, 'quantity', i.quantity::text, 'asset', app.asset_identity_json(d)
      ) ORDER BY d.id)
      FROM app.trade_session_item i
      JOIN app.asset_definition d ON d.id = i.asset_definition_id
      WHERE i.session_id = p_session.id AND i.owner_id = p_session.counterparty_id
    ), '[]'::jsonb),
    'settlementOperationId', p_session.settlement_operation_id,
    'createdAt', p_session.created_at,
    'updatedAt', p_session.updated_at
  )
  FROM app.account init_acc
  JOIN app.account cp_acc ON cp_acc.id = p_session.counterparty_id
  WHERE init_acc.id = p_session.initiator_id
$$;

CREATE OR REPLACE FUNCTION app.update_trade_session_offer(
  p_session uuid,
  p_expected_revision bigint,
  p_coins numeric,
  p_items jsonb
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
  owner_id uuid;
  coin_id uuid;
  item jsonb;
  a_id uuid;
  qty numeric;
  proposed_bundle jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  IF p_coins IS NULL OR p_coins < 0 OR p_coins <> trunc(p_coins)
     OR jsonb_typeof(COALESCE(p_items, '[]'::jsonb)) <> 'array'
     OR jsonb_array_length(COALESCE(p_items, '[]'::jsonb)) > 100 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('offer', 'invalid'));
  END IF;
  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id) FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  IF sess.revision <> p_expected_revision THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('revision', 'conflict'));
  END IF;

  SELECT id INTO coin_id FROM app.asset_definition WHERE kind = 'coin' AND currency_key = 'coin' LIMIT 1;
  SELECT id INTO owner_id FROM app.asset_owner WHERE account_id = actor AND state = 'active';
  IF owner_id IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  DELETE FROM app.trade_session_item WHERE session_id = p_session AND owner_id = actor;
  FOR item IN SELECT value FROM jsonb_array_elements(COALESCE(p_items, '[]'::jsonb)) LOOP
    a_id := (item->>'assetId')::uuid;
    qty := (item->>'quantity')::numeric;
    IF a_id IS NULL OR qty IS NULL OR qty <= 0 OR qty <> trunc(qty) OR a_id = coin_id THEN
      RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid trade item';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM app.asset_definition WHERE id = a_id) THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('assets', 'unavailable'));
    END IF;
    INSERT INTO app.trade_session_item(session_id, owner_id, asset_definition_id, quantity)
    VALUES (p_session, actor, a_id, qty);
  END LOOP;

  proposed_bundle := app.trade_session_bundle(p_session, actor, p_coins);
  IF jsonb_array_length(proposed_bundle) > 0 AND NOT app.bundle_available(owner_id, proposed_bundle) THEN
    RAISE EXCEPTION USING ERRCODE = 'P0001', MESSAGE = 'insufficient available trade assets';
  END IF;

  -- Any edit resets BOTH sides' readiness and confirmation to prevent bait-and-switch.
  UPDATE app.trade_session
  SET
    initiator_coins = CASE WHEN actor = initiator_id THEN p_coins ELSE initiator_coins END,
    counterparty_coins = CASE WHEN actor = counterparty_id THEN p_coins ELSE counterparty_coins END,
    initiator_ready = false,
    counterparty_ready = false,
    initiator_confirmed = false,
    counterparty_confirmed = false,
    revision = revision + 1,
    updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
EXCEPTION
  WHEN raise_exception THEN
    IF SQLERRM = 'insufficient available trade assets' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('assets', 'not enough available offered assets'));
    END IF;
    RAISE;
  WHEN unique_violation OR invalid_parameter_value OR invalid_text_representation THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('offer', 'invalid'));
END $$;

CREATE OR REPLACE FUNCTION app.confirm_trade_session(
  p_session uuid,
  p_expected_revision bigint
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  sess app.trade_session;
  peer uuid;
  next_init_conf boolean;
  next_cp_conf boolean;
  init_bundle jsonb;
  cp_bundle jsonb;
  init_owner uuid;
  cp_owner uuid;
  all_assets uuid[];
  v_assessment jsonb;
  init_restricted boolean;
  cp_restricted boolean;
  operation uuid;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  SELECT * INTO sess FROM app.trade_session WHERE id = p_session AND actor IN (initiator_id, counterparty_id);
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;

  peer := CASE WHEN sess.initiator_id = actor THEN sess.counterparty_id ELSE sess.initiator_id END;
  IF NOT app.lock_contact_pair(actor, peer, true, false) OR app.contact_blocked(actor, peer) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('contact', 'trade contact unavailable'));
  END IF;

  SELECT * INTO sess FROM app.trade_session WHERE id = p_session FOR UPDATE;
  IF sess.id IS NULL OR sess.state <> 'active' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unavailable'));
  END IF;
  IF sess.revision <> p_expected_revision THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('revision', 'conflict'));
  END IF;
  IF NOT (sess.initiator_ready AND sess.counterparty_ready) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('ready', 'both_parties_required'));
  END IF;

  next_init_conf := sess.initiator_confirmed OR (actor = sess.initiator_id);
  next_cp_conf := sess.counterparty_confirmed OR (actor = sess.counterparty_id);

  IF NOT (next_init_conf AND next_cp_conf) THEN
    UPDATE app.trade_session
    SET
      initiator_confirmed = next_init_conf,
      counterparty_confirmed = next_cp_conf,
      revision = revision + 1,
      updated_at = statement_timestamp()
    WHERE id = p_session
    RETURNING * INTO sess;
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
  END IF;

  -- Final confirmation: settle atomically through the canonical double-entry ledger.
  init_bundle := app.trade_session_bundle(p_session, sess.initiator_id, sess.initiator_coins);
  cp_bundle := app.trade_session_bundle(p_session, sess.counterparty_id, sess.counterparty_coins);
  IF jsonb_array_length(init_bundle) = 0 AND jsonb_array_length(cp_bundle) = 0 THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('offer', 'empty'));
  END IF;

  v_assessment := app.assess_exchange(init_bundle, cp_bundle);
  init_restricted := app.account_trade_restricted(sess.initiator_id);
  cp_restricted := app.account_trade_restricted(sess.counterparty_id);
  IF (init_restricted AND NOT (v_assessment->>'outgoingAllowed')::boolean)
     OR (cp_restricted AND NOT (v_assessment->>'incomingAllowed')::boolean) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('value', 'restricted outgoing value exceeds the allowed limit'));
  END IF;

  SELECT id INTO init_owner FROM app.asset_owner WHERE account_id = sess.initiator_id AND state = 'active';
  SELECT id INTO cp_owner FROM app.asset_owner WHERE account_id = sess.counterparty_id AND state = 'active';
  IF init_owner IS NULL OR cp_owner IS NULL THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  all_assets := app.bundle_asset_ids(init_bundle) || app.bundle_asset_ids(cp_bundle);
  PERFORM app.prepare_exchange_inventory(init_owner, cp_owner, all_assets);
  IF (jsonb_array_length(init_bundle) > 0 AND NOT app.bundle_available(init_owner, init_bundle))
     OR (jsonb_array_length(cp_bundle) > 0 AND NOT app.bundle_available(cp_owner, cp_bundle)) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('assets', 'trade assets no longer available'));
  END IF;

  INSERT INTO app.ledger_operation(kind, source_ref, source_scope, actor_account_id, created_at, state)
  VALUES ('trade_settlement', p_session::text, 'trade_session_settlement', actor, clock_timestamp(), 'pending')
  RETURNING id INTO operation;

  PERFORM app.apply_exchange(init_owner, cp_owner, init_bundle, cp_bundle, operation, p_session::text);

  UPDATE app.ledger_operation SET state = 'committed' WHERE id = operation;
  UPDATE app.trade_session
  SET
    initiator_confirmed = true,
    counterparty_confirmed = true,
    state = 'completed',
    settlement_operation_id = operation,
    assessment = v_assessment || jsonb_build_object(
      'initiatorRestricted', init_restricted,
      'counterpartyRestricted', cp_restricted
    ),
    revision = revision + 1,
    updated_at = statement_timestamp()
  WHERE id = p_session
  RETURNING * INTO sess;

  INSERT INTO app.notification(recipient_id, source_kind, source_ref, body)
  VALUES
    (sess.initiator_id, 'trade_accepted', p_session::text, 'Trade completed'),
    (sess.counterparty_id, 'trade_accepted', p_session::text, 'Trade completed')
  ON CONFLICT DO NOTHING;

  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('session', app.trade_session_json(sess)));
END $$;

REVOKE ALL ON FUNCTION app.trade_session_bundle(uuid, uuid, numeric)
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Pause completed temporary pilot wake jobs to eliminate ~34k/day idle cron log churn.
DO $pause_pilots$
DECLARE
  rec record;
BEGIN
  IF to_regclass('cron.job') IS NOT NULL AND to_regprocedure('cron.alter_job(bigint,text,text,text,text,boolean)') IS NOT NULL THEN
    FOR rec IN
      SELECT jobid FROM cron.job
      WHERE active AND jobname IN (
        'spellwake-commander-worker-wake',
        'spellwake-eight-format-pilot-worker-wake'
      )
    LOOP
      PERFORM cron.alter_job(rec.jobid, active := false);
    END LOOP;
  END IF;
END $pause_pilots$;

COMMIT;
