BEGIN;

CREATE FUNCTION app.prediction_outcome(p_market uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE market app.prediction_market; scheduled record; observation app.price_observation; manifest jsonb;
 manifest_id uuid; evidence jsonb:='[]'; unavailable boolean:=false; pending boolean:=false;
 positive boolean:=false; count_slots integer:=0; terminal date; finish_at timestamptz;
BEGIN
 SELECT * INTO STRICT market FROM app.prediction_market WHERE id=p_market;
 IF market.rules_revision<>'scryfall-sampled-gte-v1' OR market.sampling_policy_ref<>'scryfall-daily-12-13-utc-v1'
 OR NOT app.price_decimal_valid(market.target_usd_decimal) OR market.target_usd_decimal::numeric<=0
 OR market.deadline>market.observation_starts_at+interval '366 days' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unqualified prediction outcome terms';
 END IF;
 SELECT max(s.slot),max(s.closes_at),count(*) INTO terminal,finish_at,count_slots FROM app.prediction_slots(market) s;
 IF count_slots NOT BETWEEN 1 AND 366 OR market.terminal_slot_ref<>terminal::text THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid fixed prediction sample calendar';
 END IF;
 FOR scheduled IN SELECT * FROM app.prediction_slots(market) s
 WHERE market.condition='reached_by' OR s.slot=terminal ORDER BY s.slot LOOP
  IF clock_timestamp()<scheduled.closes_at THEN pending:=true; CONTINUE; END IF;
  SELECT * INTO observation FROM app.price_observation WHERE market_id=p_market AND scheduled_slot_ref=scheduled.slot::text;
  IF NOT FOUND THEN
   -- Absence of market processing is not absence of source data. A valid shared
   -- artifact waits for extraction; an actually missed slot can be finalized.
   manifest_id:=app.commit_price_slot(scheduled.slot,NULL);
   manifest:=app.read_price_manifest(manifest_id);
   IF manifest->>'state'='valid' THEN pending:=true; CONTINUE; END IF;
   PERFORM app.record_prediction_observation(p_market,manifest_id,NULL);
   SELECT * INTO STRICT observation FROM app.price_observation WHERE market_id=p_market AND scheduled_slot_ref=scheduled.slot::text;
  END IF;
  manifest:=app.read_price_manifest(observation.manifest_id);
  IF observation.printing_id<>market.printing_id OR observation.source_ref<>'scryfall:all_cards'
  OR observation.price_field<>split_part(market.price_metric_ref,':',2) OR manifest->>'slot'<>scheduled.slot::text
  OR (observation.state='valid' AND (manifest->>'state'<>'valid' OR NOT app.price_decimal_valid(observation.price_usd_decimal))) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction observation source is inconsistent';
  END IF;
  evidence:=evidence||jsonb_build_array(jsonb_build_object('observationId',observation.id,'evidence',observation.evidence_ref));
  IF observation.state<>'valid' THEN unavailable:=true;
  ELSIF observation.price_usd_decimal::numeric>=market.target_usd_decimal::numeric THEN positive:=true; END IF;
 END LOOP;
 IF positive THEN RETURN jsonb_build_object('state','final','outcome','yes','evidence',evidence); END IF;
 IF pending OR clock_timestamp()<finish_at THEN RETURN jsonb_build_object('state','pending'); END IF;
 RETURN jsonb_build_object('state','final','outcome',CASE WHEN unavailable THEN 'void' ELSE 'no' END,'evidence',evidence);
END $$;

-- Settlement states require the market's committed terminal source. Void
-- releases availability because actual costs were held, never debited at fill.
CREATE OR REPLACE FUNCTION app.validate_prediction_order(p_order uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE ticket app.prediction_order; hold app.asset_hold; filled numeric; cost numeric; backing numeric; line_quantity numeric;
 terminal app.prediction_market;
BEGIN
 SELECT * INTO STRICT ticket FROM app.prediction_order WHERE id=p_order;
 SELECT * INTO STRICT hold FROM app.asset_hold WHERE id=ticket.hold_id;
 SELECT coalesce(sum(quantity),0) INTO filled FROM app.prediction_pair WHERE yes_order_id=p_order OR no_order_id=p_order;
 cost:=app.prediction_filled_cost(p_order);
 backing:=cost+CASE WHEN ticket.state='open' THEN (ticket.quantity-ticket.filled_quantity)*ticket.limit_cost_coins ELSE 0 END;
 SELECT l.quantity INTO line_quantity FROM app.hold_line l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id
 JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE l.hold_id=hold.id AND d.kind='coin' AND d.currency_key='coin';
 IF ticket.filled_quantity<>filled OR hold.purpose<>'prediction' OR hold.commitment_ref<>ticket.id::text
 OR NOT EXISTS(SELECT 1 FROM app.asset_owner WHERE id=hold.owner_id AND account_id=ticket.account_id)
 OR (SELECT count(*) FROM app.hold_line WHERE hold_id=hold.id)<>1 OR line_quantity IS NULL
 OR (ticket.state<>'settled' AND ((backing>0 AND (hold.state<>'held' OR line_quantity<>backing)) OR (backing=0 AND hold.state<>'released')))
 OR NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=hold.operation_id AND kind='prediction_order' AND source_scope='prediction_order'
 AND source_ref=ticket.id::text AND actor_account_id=ticket.account_id AND state='committed') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction fill/backing/source commitment is inconsistent';
 END IF;
 IF ticket.state='settled' THEN
  SELECT * INTO STRICT terminal FROM app.prediction_market WHERE id=ticket.market_id;
  IF terminal.state<>'resolved' OR hold.state<>(CASE WHEN cost>0 AND terminal.outcome<>'void' THEN 'consumed' ELSE 'released' END)
  OR (cost>0 AND line_quantity<>cost)
  OR NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=terminal.settlement_operation_id AND kind='prediction_settlement'
   AND source_scope='prediction_settlement' AND source_ref=terminal.id::text AND state='committed') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='settled order requires its terminal market source';
  END IF;
 END IF;
END $$;

CREATE FUNCTION app.resolve_prediction(p_market uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE market app.prediction_market; decision jsonb; operation uuid; ticket app.prediction_order; cost numeric; item record;
 evidence text;
BEGIN
 market:=app.lock_prediction_market(p_market,NULL);
 IF market.state='resolved' THEN RETURN jsonb_build_object('state','final','outcome',market.outcome,'operationId',market.settlement_operation_id); END IF;
 decision:=app.prediction_outcome(p_market);
 IF decision->>'state'<>'final' THEN RETURN decision; END IF;
 PERFORM app.close_prediction_trading(p_market);
 SELECT * INTO STRICT market FROM app.prediction_market WHERE id=p_market;
 IF market.state<>'closed' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction must close before settlement'; END IF;
 -- Accounts are already locked. Take every touched coin inventory in one stable
 -- order before changing holds or posting any debit/credit.
 PERFORM i.id FROM app.inventory_asset i WHERE EXISTS(SELECT 1 FROM app.prediction_order o JOIN app.hold_line l ON l.hold_id=o.hold_id
 WHERE o.market_id=p_market AND l.inventory_asset_id=i.id) ORDER BY i.id FOR NO KEY UPDATE;
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,created_at,state)
 VALUES('prediction_settlement',p_market::text,'prediction_settlement',clock_timestamp(),'pending') RETURNING id INTO operation;
 FOR ticket IN SELECT * FROM app.prediction_order WHERE market_id=p_market ORDER BY id LOOP
  cost:=app.prediction_filled_cost(ticket.id);
  UPDATE app.asset_hold SET state=CASE WHEN cost>0 AND decision->>'outcome'<>'void' THEN 'consumed' ELSE 'released' END
  WHERE id=ticket.hold_id AND state='held';
  UPDATE app.prediction_order SET state='settled' WHERE id=ticket.id;
 END LOOP;
 IF decision->>'outcome'<>'void' THEN
  FOR item IN SELECT l.inventory_asset_id,sum(app.prediction_filled_cost(o.id)) AS quantity
  FROM app.prediction_order o JOIN app.hold_line l ON l.hold_id=o.hold_id WHERE o.market_id=p_market GROUP BY l.inventory_asset_id LOOP
   IF item.quantity>0 THEN
    INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
    VALUES(operation,item.inventory_asset_id,'debit',item.quantity,'prediction',p_market::text);
   END IF;
  END LOOP;
  FOR item IN SELECT l.inventory_asset_id,sum(o.filled_quantity*100) AS quantity
  FROM app.prediction_order o JOIN app.hold_line l ON l.hold_id=o.hold_id
  WHERE o.market_id=p_market AND o.side=decision->>'outcome' GROUP BY l.inventory_asset_id LOOP
   IF item.quantity>0 THEN
    INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
    VALUES(operation,item.inventory_asset_id,'credit',item.quantity,'prediction',p_market::text);
   END IF;
  END LOOP;
 END IF;
 evidence:=decision::text;
 UPDATE app.prediction_market SET state='resolved',outcome=decision->>'outcome',resolution_evidence_ref=evidence,
 resolution_rules_revision=rules_revision,settlement_operation_id=operation,resolved_at=clock_timestamp(),orders_revision=orders_revision+1 WHERE id=p_market;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 RETURN jsonb_build_object('state','final','outcome',decision->>'outcome','operationId',operation);
END $$;

CREATE FUNCTION app.check_prediction_settlement() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE market app.prediction_market; operation app.ledger_operation; decision jsonb;
BEGIN
 IF TG_TABLE_NAME='prediction_market' THEN
  SELECT * INTO STRICT market FROM app.prediction_market WHERE id=NEW.id;
  IF market.state<>'resolved' THEN RETURN NULL; END IF;
  SELECT * INTO STRICT operation FROM app.ledger_operation WHERE id=market.settlement_operation_id;
 ELSE
  SELECT * INTO STRICT operation FROM app.ledger_operation WHERE id=NEW.id;
  IF operation.kind<>'prediction_settlement' OR operation.state<>'committed' THEN RETURN NULL; END IF;
  SELECT * INTO STRICT market FROM app.prediction_market WHERE settlement_operation_id=operation.id;
 END IF;
 IF operation.kind<>'prediction_settlement' OR operation.state<>'committed' OR operation.actor_account_id IS NOT NULL
 OR operation.source_scope<>'prediction_settlement' OR operation.source_ref<>market.id::text
 OR market.state<>'resolved' OR market.resolution_rules_revision<>market.rules_revision
 OR EXISTS(SELECT 1 FROM app.prediction_order WHERE market_id=market.id AND state<>'settled') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction settlement source or orders inconsistent';
 END IF;
 decision:=market.resolution_evidence_ref::jsonb;
 IF decision->>'state'<>'final' OR decision->>'outcome'<>market.outcome OR jsonb_typeof(decision->'evidence')<>'array' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction resolution evidence is inconsistent';
 END IF;
 IF EXISTS(SELECT 1 FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id
 JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE p.operation_id=operation.id
 AND (d.currency_key IS DISTINCT FROM 'coin' OR d.kind<>'coin' OR p.source_kind<>'prediction' OR p.source_ref<>market.id::text)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction settlement contains an unrelated posting';
 END IF;
 IF market.outcome='void' THEN
  IF EXISTS(SELECT 1 FROM app.ledger_posting WHERE operation_id=operation.id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='void releases backing without issuing or debiting coins';
  END IF;
 ELSE
  IF EXISTS(WITH expected AS (
   SELECT l.inventory_asset_id,'debit'::text AS direction,sum(app.prediction_filled_cost(o.id)) AS quantity
   FROM app.prediction_order o JOIN app.hold_line l ON l.hold_id=o.hold_id WHERE o.market_id=market.id GROUP BY l.inventory_asset_id
   HAVING sum(app.prediction_filled_cost(o.id))>0
   UNION ALL
   SELECT l.inventory_asset_id,'credit',sum(o.filled_quantity*100)
   FROM app.prediction_order o JOIN app.hold_line l ON l.hold_id=o.hold_id WHERE o.market_id=market.id AND o.side=market.outcome
   GROUP BY l.inventory_asset_id HAVING sum(o.filled_quantity)>0), actual AS (
   SELECT p.inventory_asset_id,p.direction,sum(p.quantity) AS quantity FROM app.ledger_posting p WHERE p.operation_id=operation.id GROUP BY p.inventory_asset_id,p.direction)
   SELECT 1 FROM expected e FULL JOIN actual a USING(inventory_asset_id,direction) WHERE e.quantity IS DISTINCT FROM a.quantity) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction settlement does not match original costs and fixed payouts';
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER prediction_market_settlement AFTER INSERT OR UPDATE ON app.prediction_market
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_settlement();
CREATE CONSTRAINT TRIGGER prediction_operation_settlement AFTER INSERT OR UPDATE ON app.ledger_operation
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_settlement();
GRANT EXECUTE ON FUNCTION app.resolve_prediction(uuid) TO service_role;

COMMIT;
