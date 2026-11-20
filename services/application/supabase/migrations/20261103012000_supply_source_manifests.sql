BEGIN;
ALTER TABLE app.command_receipt ADD COLUMN normalized_source jsonb CHECK(normalized_source IS NULL OR jsonb_typeof(normalized_source)='object');
-- These private ports accept normalized reviewed evidence. They establish the
-- database conversion, not the truth of a physical manufacturer's disclosures.
CREATE FUNCTION app.publish_baseline_manifest(p_manifest jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; fingerprint text; forecast numeric; cutoff timestamptz;
 count_products bigint; quantity numeric; item jsonb; recipe uuid; product uuid; window_id uuid; windows jsonb:='[]';
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='baseline publication requires read committed retry'; END IF;
 IF jsonb_typeof(p_manifest) IS DISTINCT FROM 'object' OR octet_length(p_manifest::text)>1048576
 OR p_manifest->>'version' IS DISTINCT FROM 'launch-baseline-v1'
 OR jsonb_typeof(p_manifest->'forecast') IS DISTINCT FROM 'string' OR p_manifest->>'forecast' !~ '^(0|[1-9][0-9]{0,29})$'
 OR jsonb_typeof(p_manifest->'cutoff') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p_manifest->'configurationRef') IS DISTINCT FROM 'string' OR p_manifest->>'configurationRef' !~ '^sha256:[0-9a-f]{64}$'
 OR jsonb_typeof(p_manifest->'products') IS DISTINCT FROM 'array'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_manifest) k WHERE k NOT IN ('version','forecast','cutoff','configurationRef','products')) THEN
 RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid reviewed baseline manifest'; END IF;
 cutoff:=app.financial_instant(p_manifest->>'cutoff');
 IF cutoff>clock_timestamp() THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='baseline eligibility cutoff cannot be future'; END IF;
 count_products:=jsonb_array_length(p_manifest->'products');
 IF count_products NOT BETWEEN 1 AND 10000 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='baseline needs a bounded complete product list'; END IF;
 forecast:=(p_manifest->>'forecast')::numeric;
 quantity:=greatest(20,div(120*forecast+count_products-1,count_products));
 fingerprint:=app.auth_json_hash(p_manifest);
 PERFORM app.lock_valuation_catalogue(true);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('supply:baseline','baseline_source_v1','initial','initial',fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='supply:baseline' AND scope='baseline_source_v1' AND request_ref='initial' FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='launch baseline configuration is frozen'; END IF;
 IF receipt.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='baseline publication pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 IF (SELECT count(DISTINCT x->>'recipeId') FROM jsonb_array_elements(p_manifest->'products') x)<>count_products
 OR (SELECT count(DISTINCT x->>'productId') FROM jsonb_array_elements(p_manifest->'products') x)<>count_products THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='baseline products and recipes must be distinct'; END IF;
 FOR item IN SELECT x FROM jsonb_array_elements(p_manifest->'products') x ORDER BY x->>'productId' LOOP
 IF jsonb_typeof(item) IS DISTINCT FROM 'object'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['recipeId','productId','eligibleAt','evidenceRef','modifierN','modifierD','modifierRef']) k WHERE jsonb_typeof(item->k) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('recipeId','productId','eligibleAt','evidenceRef','modifierN','modifierD','modifierRef'))
 OR item->>'recipeId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR item->>'productId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR item->>'evidenceRef' !~ '^sha256:[0-9a-f]{64}$' OR item->>'modifierRef' !~ '^sha256:[0-9a-f]{64}$'
 OR item->>'modifierN' !~ '^[1-9][0-9]{0,29}$' OR item->>'modifierD' !~ '^[1-9][0-9]{0,29}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid baseline product evidence'; END IF;
 IF app.financial_instant(item->>'eligibleAt')>cutoff THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='product is outside frozen baseline cutoff'; END IF;
 recipe:=(item->>'recipeId')::uuid;product:=(item->>'productId')::uuid;
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe r WHERE r.id=recipe AND r.product_id=product AND r.qualified_at IS NOT NULL FOR SHARE) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='baseline recipe is not qualified for this product'; END IF;
 window_id:=app.publish_supply_window(recipe,quantity,quantity,(item->>'modifierN')::numeric,(item->>'modifierD')::numeric,item->>'modifierRef','baseline-v1:'||fingerprint||':'||recipe::text,'baseline',NULL,'sha256:'||fingerprint,NULL);
 windows:=windows||jsonb_build_array(jsonb_build_object('recipeId',recipe,'productId',product,'windowId',window_id,'quantity',quantity::text));
 END LOOP;
 UPDATE app.command_receipt SET normalized_source=p_manifest WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('quantityPerProduct',quantity::text,'windows',windows::text));
END $$;

CREATE FUNCTION app.publish_physical_introduction(p_manifest jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; fingerprint text; coverage text; recipe uuid; product uuid;
 quantity numeric; window_id uuid; disposition text; source_key text; eligible boolean;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='physical publication requires read committed retry'; END IF;
 IF jsonb_typeof(p_manifest) IS DISTINCT FROM 'object' OR octet_length(p_manifest::text)>65536
 OR p_manifest->>'version' IS DISTINCT FROM 'physical-introduction-v1'
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['recipeId','productId','runRef','language','territory','unit','qualification','modifierN','modifierD','modifierRef','normalQuantity']) k WHERE jsonb_typeof(p_manifest->k) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_manifest) k WHERE k NOT IN ('version','recipeId','productId','runRef','language','territory','unit','qualification','physicalQuantity','evidenceRef','modifierN','modifierD','modifierRef','normalQuantity'))
 OR p_manifest->>'recipeId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR p_manifest->>'productId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR length(p_manifest->>'runRef') NOT BETWEEN 1 AND 256 OR btrim(p_manifest->>'runRef')<>p_manifest->>'runRef'
 OR p_manifest->>'language' !~ '^[a-z]{2,3}$' OR p_manifest->>'territory' !~ '^([A-Z]{2}|GLOBAL)$'
 OR p_manifest->>'qualification' NOT IN ('exact','unavailable','bounded')
 OR NOT(p_manifest ? 'physicalQuantity') OR NOT(p_manifest ? 'evidenceRef')
 OR (p_manifest->>'qualification'='exact' AND (jsonb_typeof(p_manifest->'physicalQuantity') IS DISTINCT FROM 'string' OR p_manifest->>'physicalQuantity' !~ '^(0|[1-9][0-9]{0,29})$' OR jsonb_typeof(p_manifest->'evidenceRef') IS DISTINCT FROM 'string' OR p_manifest->>'evidenceRef' !~ '^sha256:[0-9a-f]{64}$'))
 OR (p_manifest->>'qualification'<>'exact' AND (jsonb_typeof(p_manifest->'physicalQuantity') NOT IN ('string','null') OR jsonb_typeof(p_manifest->'evidenceRef') NOT IN ('string','null')))
 OR p_manifest->>'modifierN' !~ '^[1-9][0-9]{0,29}$' OR p_manifest->>'modifierD' !~ '^[1-9][0-9]{0,29}$'
 OR p_manifest->>'normalQuantity' !~ '^[1-9][0-9]{0,29}$'
 OR p_manifest->>'modifierRef' !~ '^sha256:[0-9a-f]{64}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized physical allocation'; END IF;
 recipe:=(p_manifest->>'recipeId')::uuid;product:=(p_manifest->>'productId')::uuid;
 fingerprint:=app.auth_json_hash(p_manifest);
 coverage:=app.auth_json_hash(jsonb_build_object('productId',product,'runRef',p_manifest->>'runRef','language',p_manifest->>'language','territory',p_manifest->>'territory'));
 eligible:=p_manifest->>'qualification'='exact' AND p_manifest->>'unit'='physical_pack';
 source_key:=CASE WHEN eligible THEN coverage ELSE 'observation:'||fingerprint END;
 PERFORM app.lock_valuation_catalogue(true);
 IF eligible AND EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.principal_key='supply:introduction:'||product::text AND r.scope='physical_source_v1' AND r.state='accepted'
  AND r.normalized_source->>'qualification'='exact' AND r.normalized_source->>'unit'='physical_pack'
  AND (r.normalized_source->>'runRef' IS DISTINCT FROM p_manifest->>'runRef'
   OR (r.target_ref<>coverage AND r.normalized_source->>'language'=p_manifest->>'language' AND (r.normalized_source->>'territory'='GLOBAL' OR p_manifest->>'territory'='GLOBAL')))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='initial physical coverage overlaps or requires a separate reprint policy'; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe r WHERE r.id=recipe AND r.product_id=product AND r.qualified_at IS NOT NULL FOR SHARE) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='introduction requires the qualified product recipe'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('supply:introduction:'||product::text,'physical_source_v1',coverage,source_key,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='supply:introduction:'||product::text AND scope='physical_source_v1' AND request_ref=source_key FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='physical coverage evidence is immutable'; END IF;
 IF receipt.state<>'accepted' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='physical publication pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 IF p_manifest->>'qualification'<>'exact' OR p_manifest->>'unit'<>'physical_pack' THEN quantity:=0;disposition:='unavailable';
 ELSE quantity:=div((p_manifest->>'physicalQuantity')::numeric,100);disposition:=CASE WHEN quantity=0 THEN 'zero' ELSE 'published' END; END IF;
 IF quantity>0 THEN
 window_id:=app.publish_supply_window(recipe,quantity,(p_manifest->>'normalQuantity')::numeric,(p_manifest->>'modifierN')::numeric,(p_manifest->>'modifierD')::numeric,p_manifest->>'modifierRef','introduction-v1:'||fingerprint,'introduction',NULL,'sha256:'||fingerprint,coverage);
 END IF;
 UPDATE app.command_receipt SET normalized_source=p_manifest WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('coverage',coverage,'quantity',quantity::text,'windowId',coalesce(window_id::text,''),'disposition',disposition));
END $$;
REVOKE ALL ON FUNCTION app.publish_supply_window(uuid,numeric,numeric,numeric,numeric,text,text,text,uuid,text,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
REVOKE ALL ON FUNCTION app.publish_baseline_manifest(jsonb),app.publish_physical_introduction(jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
