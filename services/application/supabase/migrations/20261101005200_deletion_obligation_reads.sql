BEGIN;

-- This projection reports authoritative stored work, not permission to finalize.
-- Full host/payment producers and final transactional fencing remain required.
CREATE FUNCTION app.read_account_deletion_status(p_actor uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; uncommitted bigint; predictions bigint; purchases bigint;
 provider_events bigint; matches bigint; holds bigint; commands bigint;
BEGIN
 SELECT * INTO player FROM app.account WHERE id=p_actor AND state='deletion_pending';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion account unavailable'; END IF;
 SELECT count(*) INTO uncommitted FROM (
  SELECT id FROM app.trade_offer WHERE state='offered' AND p_actor IN (sender_id,recipient_id)
  UNION ALL SELECT id FROM app.market_listing WHERE state='listed' AND seller_id=p_actor
  UNION ALL SELECT id FROM app.prediction_order WHERE state='open' AND account_id=p_actor
 ) work;
 SELECT count(DISTINCT m.id) INTO predictions FROM app.prediction_market m JOIN app.prediction_pair p ON p.market_id=m.id
 JOIN app.prediction_order o ON o.id IN (p.yes_order_id,p.no_order_id) WHERE o.account_id=p_actor AND m.state<>'resolved';
 SELECT count(*) INTO purchases FROM app.purchase WHERE account_id=p_actor AND state IN ('creating','pending');
 SELECT count(*) INTO provider_events FROM app.provider_event WHERE account_id=p_actor AND provider='patreon' AND processing_state='verified';
 SELECT count(DISTINCT m.id) INTO matches FROM app.match m JOIN app.match_seat s ON s.match_id=m.id
 WHERE p_actor IN (s.account_id,s.sponsor_id) AND NOT EXISTS(SELECT 1 FROM app.match_result r WHERE r.match_id=m.id);
 SELECT count(*) INTO holds FROM app.asset_hold h JOIN app.asset_owner o ON o.id=h.owner_id
 WHERE o.account_id=p_actor AND h.state='held' AND EXISTS(SELECT 1 FROM app.hold_line l WHERE l.hold_id=h.id AND l.quantity>0);
 SELECT count(*) INTO commands FROM app.command_receipt WHERE account_id=p_actor AND state='pending';
 RETURN jsonb_build_object('state','deletion_pending','requestedAt',
 to_char(player.deletion_requested_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
 'obligations',jsonb_build_object('uncommittedWork',uncommitted::text,'unsettledPredictionMarkets',predictions::text,
 'purchasesInFlight',purchases::text,'verifiedPatreonEvents',provider_events::text,'matchesWithoutResult',matches::text,
 'heldCommitments',holds::text,'pendingCommands',commands::text));
END $$;
GRANT EXECUTE ON FUNCTION app.read_account_deletion_status(uuid) TO service_role;

COMMIT;
