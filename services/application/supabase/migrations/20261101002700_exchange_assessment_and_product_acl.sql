BEGIN;

-- Catalogue writers can name products, but cannot discard or rewind a sold-out
-- terminal basis. Only the positive-allocation publisher controls its pointer.
REVOKE UPDATE,DELETE ON app.booster_product FROM service_role;
GRANT UPDATE(name) ON app.booster_product TO service_role;

CREATE FUNCTION app.bundle_asset_ids(p_lines jsonb) RETURNS uuid[]
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE line jsonb; ids uuid[]:='{}'; asset uuid;
BEGIN
 IF p_lines IS NULL OR jsonb_typeof(p_lines)<>'array' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='expected asset bundle'; END IF;
 IF jsonb_array_length(p_lines)>100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='too many bundle lines'; END IF;
 FOR line IN SELECT v FROM jsonb_array_elements(p_lines) v LOOP
  IF jsonb_typeof(line)<>'object' OR (line-ARRAY['assetId','quantity'])<>'{}'::jsonb
  OR jsonb_typeof(line->'assetId') IS DISTINCT FROM 'string' OR jsonb_typeof(line->'quantity') IS DISTINCT FROM 'string'
  OR (line->>'quantity') !~ '^[1-9][0-9]{0,77}$' THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid asset bundle line';
  END IF;
  asset:=(line->>'assetId')::uuid;
  IF asset=ANY(ids) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='duplicate asset bundle line'; END IF;
  ids:=array_append(ids,asset);
 END LOOP;
 RETURN ids;
END $$;

CREATE OR REPLACE FUNCTION app.assess_bundle(p_lines jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE line jsonb; ids uuid[]:='{}'; asset uuid; quantity numeric; assessment jsonb;
 total app.exact_value:=app.value_make(0,1); weighted app.exact_value; lines jsonb:='[]';
BEGIN
 ids:=app.bundle_asset_ids(p_lines);
 PERFORM app.lock_assessment_products(ids);
 FOR line IN SELECT v FROM jsonb_array_elements(p_lines) v LOOP
  asset:=(line->>'assetId')::uuid; quantity:=(line->>'quantity')::numeric;
  assessment:=app.assess_asset(asset);
  weighted:=app.value_make((assessment->'value'->>'numerator')::numeric*quantity,(assessment->'value'->>'denominator')::numeric);
  total:=app.value_add(total,weighted);
  lines:=lines||jsonb_build_array(jsonb_build_object('assetId',asset,'quantity',quantity::text,'unitAssessment',assessment));
 END LOOP;
 RETURN jsonb_build_object('formula','pack-marginal-v1','total',app.value_json(total),'lines',lines);
END $$;

CREATE FUNCTION app.assess_exchange(p_outgoing jsonb,p_incoming jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE outgoing jsonb; incoming jsonb; allowed boolean;
BEGIN
 PERFORM app.lock_assessment_products(app.bundle_asset_ids(p_outgoing)||app.bundle_asset_ids(p_incoming));
 outgoing:=app.assess_bundle(p_outgoing); incoming:=app.assess_bundle(p_incoming);
 allowed:=10*(outgoing->'total'->>'numerator')::numeric*(incoming->'total'->>'denominator')::numeric
 <=13*(incoming->'total'->>'numerator')::numeric*(outgoing->'total'->>'denominator')::numeric;
 RETURN jsonb_build_object('outgoing',outgoing,'incoming',incoming,'outgoingAllowed',allowed);
END $$;
GRANT EXECUTE ON FUNCTION app.assess_exchange(jsonb,jsonb) TO service_role;

COMMIT;
