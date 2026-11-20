BEGIN;

CREATE FUNCTION app.guard_asset_definition() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.kind,NEW.currency_key,NEW.printing_id,NEW.recipe_id) IS DISTINCT FROM
 (OLD.id,OLD.kind,OLD.currency_key,OLD.printing_id,OLD.recipe_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='tradable asset identity is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER asset_definition_identity BEFORE UPDATE ON app.asset_definition
 FOR EACH ROW EXECUTE FUNCTION app.guard_asset_definition();

CREATE FUNCTION app.lock_assessment_products(p_assets uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 -- Account/contact locks come first in settlement. Acquire every relevant product
 -- before assessing individual assets; input order cannot reverse product locks.
 PERFORM p.id FROM app.booster_product p WHERE EXISTS(
 SELECT 1 FROM app.asset_definition a WHERE a.id=ANY(p_assets) AND (
 (a.kind='pack' AND EXISTS(SELECT 1 FROM app.booster_recipe r WHERE r.id=a.recipe_id AND r.product_id=p.id)) OR
 (a.kind='card' AND EXISTS(SELECT 1 FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id
 JOIN app.booster_recipe r ON r.id=s.recipe_id WHERE c.printing_id=a.printing_id AND r.qualified_at IS NOT NULL AND r.product_id=p.id))))
 ORDER BY p.id FOR NO KEY UPDATE;
END $$;

CREATE FUNCTION app.assess_asset(p_asset uuid) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE asset app.asset_definition; recipe app.booster_recipe; candidate record; quote jsonb;
 expected app.exact_value; value app.exact_value; best app.exact_value; basis jsonb; count_outputs numeric;
BEGIN
 SELECT * INTO STRICT asset FROM app.asset_definition WHERE id=p_asset;
 IF asset.kind='coin' THEN
  IF asset.currency_key<>'coin' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unsupported coin definition'; END IF;
  RETURN jsonb_build_object('value',app.value_json(app.value_make(1,1)),'basis',jsonb_build_object('kind','coin'));
 ELSIF asset.kind='pack' THEN
  quote:=app.quote_packs(asset.recipe_id,1);
  RETURN jsonb_build_object('value',app.value_json(app.value_make((quote->>'unitPrice')::numeric,1)),'basis',quote);
 END IF;
 FOR recipe IN SELECT DISTINCT r.* FROM app.booster_recipe r JOIN app.booster_product p ON p.id=r.product_id
 JOIN app.booster_slot s ON s.recipe_id=r.id JOIN app.booster_slot_card c ON c.slot_id=s.id
 WHERE r.qualified_at IS NOT NULL AND p.current_pricing_policy_id IS NOT NULL AND c.printing_id=asset.printing_id
 ORDER BY r.product_id,r.id LOOP
  expected:=app.value_make(0,1);
  FOR candidate IN SELECT c.id FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id
   WHERE s.recipe_id=recipe.id AND c.printing_id=asset.printing_id ORDER BY c.id LOOP
   expected:=app.value_add(expected,app.slot_expected(candidate.id));
  END LOOP;
  SELECT count(DISTINCT c.printing_id) INTO count_outputs FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id WHERE s.recipe_id=recipe.id;
  quote:=app.quote_packs(recipe.id,1);
  value:=app.value_make((quote->>'unitPrice')::numeric*expected.denominator,count_outputs*expected.numerator);
  IF best IS NULL OR app.value_less(value,best) THEN
   best:=value;
   basis:=jsonb_build_object('quote',quote,'recipeQualification',recipe.qualification_ref,'candidateCount',count_outputs::text,'expectedCopies',app.value_json(expected));
  END IF;
 END LOOP;
 IF best IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='asset has no qualified pack valuation basis'; END IF;
 RETURN jsonb_build_object('value',app.value_json(best),'basis',basis);
END $$;

CREATE FUNCTION app.assess_bundle(p_lines jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE line jsonb; ids uuid[]:='{}'; asset uuid; quantity numeric; assessment jsonb;
 total app.exact_value:=app.value_make(0,1); weighted app.exact_value; lines jsonb:='[]';
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
GRANT EXECUTE ON FUNCTION app.assess_bundle(jsonb) TO service_role;

CREATE FUNCTION app.guard_supply_purchase() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted supply purchase is immutable'; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.ledger_operation WHERE id=NEW.operation_id AND actor_account_id=NEW.buyer_id AND state='pending' AND kind='pack_purchase')
 OR NOT EXISTS(SELECT 1 FROM app.supply_stock WHERE id=NEW.stock_id AND pricing_policy_id=NEW.pricing_policy_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='purchase allocation requires matching source operation and pricing window';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER supply_purchase_source BEFORE INSERT OR UPDATE OR DELETE ON app.supply_purchase
 FOR EACH ROW EXECUTE FUNCTION app.guard_supply_purchase();

CREATE FUNCTION app.buy_packs(p_actor uuid,p_recipe uuid,p_quantity integer,p_policy uuid,p_revision text,p_total text,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; quote jsonb; policy app.supply_pricing_policy; owner uuid;
 coin uuid; pack uuid; coin_inventory uuid; pack_inventory uuid; available numeric; operation uuid;
 stock app.supply_stock; needed integer:=p_quantity; take integer; sold integer:=0; index integer; line_price numeric; paid numeric:=0;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'pack_purchase',p_recipe::text,
  jsonb_build_object('quantity',p_quantity,'policy',p_policy,'revision',p_revision,'total',p_total));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF p_quantity IS NULL OR p_quantity NOT BETWEEN 1 AND 1000 OR p_policy IS NULL OR p_revision IS NULL
 OR p_revision !~ '^(0|[1-9][0-9]{0,77})$' OR p_total IS NULL OR p_total !~ '^[1-9][0-9]{0,77}$' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('quote','invalid purchase terms'));
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe r JOIN app.booster_product p ON p.id=r.product_id
 WHERE r.id=p_recipe AND r.qualified_at IS NOT NULL AND p.current_pricing_policy_id IS NOT NULL) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('product','product unavailable'));
 END IF;
 quote:=app.quote_packs(p_recipe,p_quantity);
 IF quote->>'purpose'<>'purchase' OR quote->>'policyId'<>p_policy::text OR quote->>'revision'<>p_revision OR quote->>'total'<>p_total THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('quote','stock or price changed; request a new quote'));
 END IF;
 SELECT * INTO STRICT policy FROM app.supply_pricing_policy WHERE id=p_policy;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=p_actor AND state='active';
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE kind='coin' AND currency_key='coin';
 coin_inventory:=app.ensure_inventory(owner,coin);
 PERFORM id FROM app.inventory_asset WHERE id=coin_inventory FOR NO KEY UPDATE;
 SELECT a.available INTO available FROM app.asset_availability a WHERE a.inventory_asset_id=coin_inventory;
 IF available<p_total::numeric THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('coins','not enough available coins'));
 END IF;
 INSERT INTO app.asset_definition(kind,recipe_id) VALUES('pack',p_recipe) ON CONFLICT(recipe_id) DO NOTHING;
 SELECT id INTO STRICT pack FROM app.asset_definition WHERE recipe_id=p_recipe;
 pack_inventory:=app.ensure_inventory(owner,pack);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('pack_purchase',receipt.id::text,'pack_purchase',p_actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 FOR stock IN SELECT * FROM app.supply_stock WHERE pricing_policy_id=p_policy AND remaining_quantity>0 ORDER BY id FOR NO KEY UPDATE LOOP
  EXIT WHEN needed=0;
  take:=least(stock.remaining_quantity,needed)::integer;
  line_price:=0;
  FOR index IN 0..take-1 LOOP line_price:=line_price+app.window_unit_price(policy,(quote->>'remaining')::numeric-sold-index); END LOOP;
  UPDATE app.supply_stock SET remaining_quantity=remaining_quantity-take WHERE id=stock.id;
  INSERT INTO app.supply_purchase(stock_id,pricing_policy_id,buyer_id,operation_id,quantity,paid_coins)
  VALUES(stock.id,p_policy,p_actor,operation,take,line_price);
  needed:=needed-take; sold:=sold+take; paid:=paid+line_price;
 END LOOP;
 IF needed<>0 OR paid<>p_total::numeric THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='purchase allocation changed'; END IF;
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
 VALUES(operation,coin_inventory,'debit',paid,'pack_purchase',receipt.id::text),
 (operation,pack_inventory,'credit',p_quantity,'pack_purchase',receipt.id::text);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',operation));
END $$;
GRANT EXECUTE ON FUNCTION app.buy_packs(uuid,uuid,integer,uuid,text,text,uuid) TO service_role;
REVOKE INSERT,UPDATE,DELETE ON app.supply_purchase FROM service_role;

COMMIT;
