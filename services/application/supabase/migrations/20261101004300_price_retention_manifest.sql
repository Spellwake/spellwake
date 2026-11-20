BEGIN;

-- Core transport/time validation remains one private primitive. Every newly
-- accepted artifact must additionally bind the retained original index bytes.
ALTER FUNCTION app.validate_price_artifact(date,jsonb,timestamptz) RENAME TO validate_price_artifact_core;
CREATE FUNCTION app.validate_price_artifact(p_slot date,p_artifact jsonb,p_clock timestamptz) RETURNS jsonb
LANGUAGE plpgsql STABLE SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE validated jsonb;
BEGIN
 IF jsonb_typeof(p_artifact->'recordCount') IS DISTINCT FROM 'string'
 OR NOT coalesce(p_artifact->>'metadataHash' ~ '^sha256:[0-9a-f]{64}$',false)
 OR NOT coalesce(p_artifact->>'metadataRef'='prices/scryfall/metadata/'||substring(p_artifact->>'metadataHash' FROM 8)||'.json',false) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='retained original source metadata identity required';
 END IF;
 validated:=app.validate_price_artifact_core(p_slot,p_artifact-'metadataHash'-'metadataRef',p_clock);
 IF (validated->>'recordCount')::numeric>5000000 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='price artifact exceeds validated worker record bound';
 END IF;
 RETURN validated||jsonb_build_object('metadataHash',p_artifact->>'metadataHash','metadataRef',p_artifact->>'metadataRef');
END $$;

CREATE FUNCTION app.read_established_price_slot(p_slot date) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE receipt uuid;
BEGIN
 IF p_slot IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='price slot required'; END IF;
 SELECT id INTO receipt FROM app.command_receipt WHERE principal_key='collector:scryfall-v1'
 AND scope='price_slot' AND request_ref=p_slot::text;
 IF NOT FOUND THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('id',receipt,'manifest',app.read_price_manifest(receipt));
END $$;
GRANT EXECUTE ON FUNCTION app.read_established_price_slot(date) TO service_role;

COMMIT;
