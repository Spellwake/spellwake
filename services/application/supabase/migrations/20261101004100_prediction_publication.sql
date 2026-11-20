BEGIN;

ALTER TABLE app.prediction_market ADD COLUMN publication_probe_id uuid REFERENCES app.command_receipt(id);

CREATE FUNCTION app.record_price_probe(p_manifest uuid,p_card jsonb) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE manifest jsonb; projection jsonb; digest text; result uuid; reference uuid;
BEGIN
 manifest:=app.read_price_manifest(p_manifest);
 IF manifest->>'state'<>'valid' OR (manifest->>'acquiredAt')::timestamptz<clock_timestamp()-interval '24 hours'
 OR jsonb_typeof(p_card)<>'object' OR p_card->>'object'<>'card' OR octet_length(p_card::text)>262144
 OR jsonb_typeof(p_card->'finishes') IS DISTINCT FROM 'array' OR jsonb_typeof(p_card->'prices') IS DISTINCT FROM 'object' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='usable current price evidence required';
 END IF;
 reference:=(p_card->>'id')::uuid;
 IF reference IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='printing identity required'; END IF;
 projection:=jsonb_build_object('manifestId',p_manifest,'printingRef',reference,'finishes',p_card->'finishes',
 'prices',jsonb_build_object('usd',p_card->'prices'->'usd','usd_foil',p_card->'prices'->'usd_foil','usd_etched',p_card->'prices'->'usd_etched'));
 digest:=encode(sha256(convert_to(projection::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('collector:scryfall-v1',p_manifest::text||':'||reference::text,'price_probe',reference::text,digest,'accepted',projection::text)
 ON CONFLICT DO NOTHING RETURNING id INTO result;
 IF FOUND THEN RETURN result; END IF;
 SELECT id INTO STRICT result FROM app.command_receipt WHERE principal_key='collector:scryfall-v1'
 AND scope='price_probe' AND request_ref=p_manifest::text||':'||reference::text AND payload_hash=digest;
 RETURN result;
END $$;

CREATE FUNCTION app.publish_prediction(p_actor uuid,p_printing uuid,p_target text,p_condition text,p_close timestamptz,
 p_start timestamptz,p_deadline timestamptz,p_probe uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE receipt app.command_receipt; probe app.command_receipt; evidence jsonb; manifest jsonb; printing app.printing;
 field text; market app.prediction_market; slots integer; terminal date; result uuid:=gen_random_uuid();
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'prediction_publish',p_printing::text,jsonb_build_object(
 'target',p_target,'condition',p_condition,'tradingClosesAt',p_close,'observationStartsAt',p_start,'deadline',p_deadline,'probeId',p_probe));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));
 END IF;
 IF NOT app.price_decimal_valid(p_target) OR p_target::numeric<=0 OR p_condition IS NULL OR p_condition NOT IN ('reached_by','at_deadline')
 OR p_close IS NULL OR p_start IS NULL OR p_deadline IS NULL OR NOT isfinite(p_close) OR NOT isfinite(p_start) OR NOT isfinite(p_deadline)
 OR p_close<=clock_timestamp() OR p_close>=p_start OR p_start>p_deadline OR p_deadline>p_start+interval '366 days' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','invalid future sampled-price terms'));
 END IF;
 SELECT * INTO printing FROM app.printing WHERE id=p_printing;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('printing','printing unavailable')); END IF;
 field:=CASE printing.finish WHEN 'nonfoil' THEN 'usd' WHEN 'foil' THEN 'usd_foil' WHEN 'etched' THEN 'usd_etched' END;
 SELECT * INTO probe FROM app.command_receipt WHERE id=p_probe AND principal_key='collector:scryfall-v1' AND scope='price_probe' AND state='accepted';
 IF NOT FOUND OR field IS NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('price','qualified printing price unavailable'));
 END IF;
 evidence:=probe.result_ref::jsonb;
 manifest:=app.read_price_manifest((evidence->>'manifestId')::uuid);
 IF probe.target_ref<>printing.external_printing_ref OR evidence->>'printingRef'<>printing.external_printing_ref
 OR probe.payload_hash<>encode(sha256(convert_to(evidence::text,'UTF8')),'hex')
 OR manifest->>'state'<>'valid' OR (manifest->>'acquiredAt')::timestamptz>clock_timestamp()
 OR (manifest->>'acquiredAt')::timestamptz<clock_timestamp()-interval '24 hours'
 OR NOT (evidence->'finishes' ? printing.finish) OR jsonb_typeof(evidence->'prices'->field)<>'string'
 OR NOT app.price_decimal_valid(evidence->'prices'->>field) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('price','usable current exact-printing price required'));
 END IF;
 market.observation_starts_at:=p_start; market.deadline:=p_deadline;
 SELECT count(*),max(s.slot) INTO slots,terminal FROM app.prediction_slots(market) s;
 IF slots=0 OR slots>366 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','at least one complete daily acceptance window required'));
 END IF;
 INSERT INTO app.prediction_market(id,creator_id,printing_id,price_metric_ref,target_usd_decimal,condition,deadline,trading_closes_at,
 observation_starts_at,sampling_policy_ref,terminal_slot_ref,rules_revision,payout_coins,next_order_sequence,state,publication_probe_id)
 VALUES(result,p_actor,p_printing,'scryfall:'||field,p_target,p_condition,p_deadline,p_close,p_start,'scryfall-daily-12-13-utc-v1',terminal::text,
 'scryfall-sampled-gte-v1',100,1,'open',p_probe);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',result));
END $$;

CREATE FUNCTION app.guard_prediction_publication_source() RETURNS trigger
LANGUAGE plpgsql SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
BEGIN
 IF NEW.publication_probe_id IS DISTINCT FROM OLD.publication_probe_id THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='prediction publication evidence is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER prediction_publication_source BEFORE UPDATE ON app.prediction_market
 FOR EACH ROW EXECUTE FUNCTION app.guard_prediction_publication_source();

-- Terms accepted by the normal function must still be future when its transaction
-- commits. A deferred check is not a scheduler or a client timestamp assertion.
CREATE FUNCTION app.check_prediction_publication_cutoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE evidence jsonb; manifest jsonb;
BEGIN
 IF NEW.publication_probe_id IS NOT NULL AND clock_timestamp()>=NEW.trading_closes_at THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='publication trading cutoff passed before commit';
 END IF;
 IF NEW.publication_probe_id IS NOT NULL THEN
  SELECT result_ref::jsonb INTO STRICT evidence FROM app.command_receipt WHERE id=NEW.publication_probe_id;
  manifest:=app.read_price_manifest((evidence->>'manifestId')::uuid);
  IF (manifest->>'acquiredAt')::timestamptz<clock_timestamp()-interval '24 hours' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='publication evidence became stale before commit';
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER prediction_publication_cutoff AFTER INSERT ON app.prediction_market
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_prediction_publication_cutoff();
GRANT EXECUTE ON FUNCTION app.record_price_probe(uuid,jsonb),
 app.publish_prediction(uuid,uuid,text,text,timestamptz,timestamptz,timestamptz,uuid,uuid) TO service_role;

COMMIT;
