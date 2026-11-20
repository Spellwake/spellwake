BEGIN;

-- Source task receipts reuse the approved durable record; no asset operation is
-- fabricated for ingestion. Only normalized trusted collector entry points write it.
REVOKE INSERT,UPDATE,DELETE ON app.command_receipt,app.price_observation FROM service_role;
CREATE TRIGGER price_observation_history BEFORE UPDATE OR DELETE ON app.price_observation
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();
ALTER TABLE app.price_observation ADD COLUMN manifest_id uuid NOT NULL REFERENCES app.command_receipt(id);

CREATE FUNCTION app.price_slot_start(p_slot date) RETURNS timestamptz
LANGUAGE sql IMMUTABLE STRICT SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
 SELECT (p_slot::timestamp+interval '12 hours') AT TIME ZONE 'UTC'
$$;
CREATE FUNCTION app.prediction_slots(p_market app.prediction_market)
RETURNS TABLE(slot date,starts_at timestamptz,closes_at timestamptz)
LANGUAGE sql IMMUTABLE STRICT SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
 SELECT day::date,app.price_slot_start(day::date),app.price_slot_start(day::date)+interval '1 hour'
 FROM generate_series((p_market.observation_starts_at AT TIME ZONE 'UTC')::date::timestamp,
 (p_market.deadline AT TIME ZONE 'UTC')::date::timestamp,interval '1 day') day
 WHERE app.price_slot_start(day::date)>=p_market.observation_starts_at
 AND app.price_slot_start(day::date)+interval '1 hour'<=p_market.deadline
$$;
CREATE FUNCTION app.price_decimal_valid(p_value text) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
 SELECT coalesce(length(p_value)<=80 AND p_value ~ '^(0|[1-9][0-9]*)(\.[0-9]+)?$',false)
$$;

CREATE FUNCTION app.validate_price_artifact(p_slot date,p_artifact jsonb,p_clock timestamptz) RETURNS jsonb
LANGUAGE plpgsql STABLE SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE started timestamptz; acquired timestamptz; updated timestamptz; manifest jsonb; v_clock timestamptz:=p_clock;
BEGIN
 IF p_slot IS NULL OR p_artifact IS NULL OR p_clock IS NULL OR NOT isfinite(p_clock) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='artifact validation basis required';
 END IF;
 started:=app.price_slot_start(p_slot);
  IF jsonb_typeof(p_artifact)<>'object' OR octet_length(p_artifact::text)>4096
  OR p_artifact-'hash'-'artifactRef'-'sourceUpdatedAt'-'acquiredAt'-'recordCount'-'downloadUri'<>'{}'::jsonb
  OR NOT coalesce(p_artifact->>'hash' ~ '^sha256:[0-9a-f]{64}$',false)
  OR NOT coalesce(p_artifact->>'artifactRef'='prices/scryfall/'||substring(p_artifact->>'hash' FROM 8)||'.jsonl.gz',false)
  OR NOT coalesce(p_artifact->>'recordCount' ~ '^[1-9][0-9]{0,9}$',false)
  OR NOT coalesce(p_artifact->>'downloadUri' ~ '^https://data\.scryfall\.io/all-cards/all-cards-[0-9]{14}\.jsonl\.gz$',false) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized Scryfall artifact';
  END IF;
  acquired:=(p_artifact->>'acquiredAt')::timestamptz; updated:=(p_artifact->>'sourceUpdatedAt')::timestamptz;
  IF acquired IS NULL OR updated IS NULL OR NOT isfinite(acquired) OR NOT isfinite(updated)
  OR v_clock<started OR v_clock>=started+interval '1 hour'
  OR acquired<started OR acquired>v_clock OR acquired>=started+interval '1 hour'
  OR updated>acquired OR updated<acquired-interval '24 hours' THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='artifact is outside its acquisition/freshness window';
  END IF;
  manifest:=p_artifact||jsonb_build_object('version','scryfall-slot-v1','slot',p_slot::text,'state','valid','source','scryfall:all_cards');

 RETURN manifest;
END $$;

CREATE FUNCTION app.commit_price_slot(p_slot date,p_artifact jsonb) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE existing app.command_receipt; result uuid; started timestamptz;
 manifest jsonb; digest text; v_clock timestamptz:=clock_timestamp();
BEGIN
 IF p_slot IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='price slot required'; END IF;
 started:=app.price_slot_start(p_slot);
 PERFORM pg_advisory_xact_lock(hashtextextended('scryfall-slot:'||p_slot::text,0));
 SELECT * INTO existing FROM app.command_receipt WHERE principal_key='collector:scryfall-v1' AND scope='price_slot' AND request_ref=p_slot::text;
 IF FOUND THEN
  IF existing.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid price slot disposition'; END IF;
  -- The first complete manifest wins; subsequent workers receive its identity.
  RETURN existing.id;
 END IF;
 v_clock:=clock_timestamp();
 IF p_artifact IS NULL THEN
  IF v_clock<started+interval '1 hour' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='slot acceptance is not over'; END IF;
  manifest:=jsonb_build_object('version','scryfall-slot-v1','slot',p_slot::text,'state','missing');
 ELSE
  manifest:=app.validate_price_artifact(p_slot,p_artifact,v_clock);
 END IF;
 digest:=encode(sha256(convert_to(manifest::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('collector:scryfall-v1',p_slot::text,'price_slot',p_slot::text,digest,'accepted',manifest::text) RETURNING id INTO result;
 RETURN result;
END $$;

CREATE FUNCTION app.read_price_manifest(p_manifest uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE receipt app.command_receipt; manifest jsonb;
BEGIN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE id=p_manifest;
 manifest:=receipt.result_ref::jsonb;
 IF receipt.principal_key<>'collector:scryfall-v1' OR receipt.scope<>'price_slot' OR receipt.state<>'accepted'
 OR receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL OR receipt.operation_id IS NOT NULL
 OR receipt.request_ref<>receipt.target_ref OR manifest->>'slot'<>receipt.target_ref
 OR manifest->>'version'<>'scryfall-slot-v1' OR manifest->>'state' NOT IN ('valid','missing')
 OR receipt.payload_hash<>encode(sha256(convert_to(manifest::text,'UTF8')),'hex') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid established price manifest';
 END IF;
 RETURN manifest;
END $$;

-- A collector extracts this exact card from the retained artifact. The DB checks
-- normalized identity/finish/field consistency; raw-file verification belongs to it.
CREATE FUNCTION app.record_prediction_observation(p_market uuid,p_manifest uuid,p_card jsonb) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE market app.prediction_market; manifest jsonb; date_slot date; field text; required_finish text;
 printing app.printing; value text; disposition text; evidence text; result uuid; existing app.price_observation;
BEGIN
 SELECT * INTO STRICT market FROM app.prediction_market WHERE id=p_market;
 manifest:=app.read_price_manifest(p_manifest); date_slot:=(manifest->>'slot')::date;
 IF NOT EXISTS(SELECT 1 FROM app.prediction_slots(market) s WHERE s.slot=date_slot)
 OR clock_timestamp()<app.price_slot_start(date_slot)+interval '1 hour' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='observation slot is not final or in market terms';
 END IF;
 SELECT * INTO STRICT printing FROM app.printing WHERE id=market.printing_id;
 field:=split_part(market.price_metric_ref,':',2);
 required_finish:=CASE field WHEN 'usd' THEN 'nonfoil' WHEN 'usd_foil' THEN 'foil' WHEN 'usd_etched' THEN 'etched' END;
 IF market.price_metric_ref NOT IN ('scryfall:usd','scryfall:usd_foil','scryfall:usd_etched') OR required_finish IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='market has no qualified price metric';
 END IF;
 disposition:='missing'; value:=NULL;
 IF manifest->>'state'='valid' AND p_card IS NOT NULL THEN
  disposition:='invalid';
  IF jsonb_typeof(p_card)='object' AND octet_length(p_card::text)<=262144
  AND p_card->>'object'='card' AND p_card->>'id'=printing.external_printing_ref
  AND printing.finish=required_finish AND jsonb_typeof(p_card->'finishes')='array'
  AND p_card->'finishes' ? required_finish AND jsonb_typeof(p_card->'prices'->field)='string'
  AND app.price_decimal_valid(p_card->'prices'->>field) THEN
   disposition:='valid'; value:=p_card->'prices'->>field;
  END IF;
 END IF;
 evidence:=encode(sha256(convert_to(jsonb_build_object('manifestId',p_manifest,'printingId',market.printing_id,
 'field',field,'state',disposition,'price',value)::text,'UTF8')),'hex');
 SELECT * INTO existing FROM app.price_observation WHERE market_id=p_market AND scheduled_slot_ref=date_slot::text;
 IF FOUND THEN
  IF existing.manifest_id IS DISTINCT FROM p_manifest OR existing.evidence_ref<>evidence THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='observation cannot be replaced';
  END IF;
  RETURN existing.id;
 END IF;
 INSERT INTO app.price_observation(market_id,printing_id,source_ref,observed_at,scheduled_slot_ref,source_artifact_updated_at,
 price_field,price_usd_decimal,evidence_ref,state,manifest_id)
 VALUES(p_market,market.printing_id,'scryfall:all_cards',CASE WHEN manifest->>'state'='valid' THEN (manifest->>'acquiredAt')::timestamptz ELSE app.price_slot_start(date_slot)+interval '1 hour' END,
 date_slot::text,(manifest->>'sourceUpdatedAt')::timestamptz,field,value,evidence,disposition,p_manifest)
 ON CONFLICT(market_id,scheduled_slot_ref) DO NOTHING RETURNING id INTO result;
 IF NOT FOUND THEN
  SELECT * INTO STRICT existing FROM app.price_observation WHERE market_id=p_market AND scheduled_slot_ref=date_slot::text;
  IF existing.manifest_id IS DISTINCT FROM p_manifest OR existing.evidence_ref<>evidence THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='observation cannot be replaced';
  END IF;
  RETURN existing.id;
 END IF;
 RETURN result;
END $$;
CREATE FUNCTION app.check_price_slot_commit_window() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE manifest jsonb; started timestamptz;
BEGIN
 IF NEW.principal_key='collector:scryfall-v1' AND NEW.scope='price_slot' THEN
  manifest:=NEW.result_ref::jsonb;
  IF manifest->>'state'='valid' THEN
   started:=app.price_slot_start((manifest->>'slot')::date);
   IF clock_timestamp()<started OR clock_timestamp()>=started+interval '1 hour' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='artifact acceptance window passed before commit';
   END IF;
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER price_slot_commit_window AFTER INSERT ON app.command_receipt
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_price_slot_commit_window();
GRANT EXECUTE ON FUNCTION app.commit_price_slot(date,jsonb),app.record_prediction_observation(uuid,uuid,jsonb) TO service_role;

COMMIT;
