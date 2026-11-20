BEGIN;

ALTER TABLE app.booster_product ADD COLUMN current_pricing_policy_id uuid REFERENCES app.supply_pricing_policy(id);
ALTER TABLE app.booster_product ADD COLUMN pricing_revision numeric NOT NULL DEFAULT 0
 CHECK (pricing_revision>=0 AND pricing_revision<power(10::numeric,78) AND scale(pricing_revision)=0);
ALTER TABLE app.supply_pricing_policy ADD COLUMN modifier_numerator numeric NOT NULL
 CHECK (modifier_numerator>0 AND modifier_numerator<power(10::numeric,78) AND scale(modifier_numerator)=0);
ALTER TABLE app.supply_pricing_policy ADD COLUMN modifier_denominator numeric NOT NULL
 CHECK (modifier_denominator>0 AND modifier_denominator<power(10::numeric,78) AND scale(modifier_denominator)=0);
ALTER TABLE app.supply_pricing_policy ADD COLUMN publication_payload jsonb NOT NULL;
ALTER TABLE app.supply_pricing_policy ADD CHECK (base_price_coins=1000 AND formula_ref='scarcity-depletion-v1');
ALTER TABLE app.supply_stock ADD COLUMN allocation_ref text NOT NULL CHECK (allocation_ref ~ '^sha256:[0-9a-f]{64}$');
ALTER TABLE app.supply_stock ADD COLUMN coverage_ref text;
ALTER TABLE app.supply_stock ADD CHECK (starting_quantity>0 AND origin IN ('baseline','quarterly','introduction'));
ALTER TABLE app.supply_stock ADD CHECK ((origin='quarterly')=(drop_id IS NOT NULL));
ALTER TABLE app.supply_stock ADD CHECK ((origin='introduction' AND coverage_ref IS NOT NULL AND btrim(coverage_ref)<>'')
 OR (origin<>'introduction' AND coverage_ref IS NULL));
CREATE UNIQUE INDEX baseline_once ON app.supply_stock(recipe_id) WHERE origin='baseline';
CREATE UNIQUE INDEX quarterly_product_once ON app.supply_stock(recipe_id,drop_id) WHERE origin='quarterly';
CREATE UNIQUE INDEX introduction_coverage_once ON app.supply_stock(recipe_id,coverage_ref) WHERE origin='introduction';
CREATE INDEX pricing_stock ON app.supply_stock(pricing_policy_id,id);

CREATE FUNCTION app.guard_product_pricing() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.set_id,NEW.external_product_ref) IS DISTINCT FROM (OLD.id,OLD.set_id,OLD.external_product_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='product identity is immutable';
 END IF;
 IF TG_OP='UPDATE' AND NEW.pricing_revision<OLD.pricing_revision THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pricing revision cannot go backwards';
 END IF;
 IF NEW.current_pricing_policy_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.supply_pricing_policy
 WHERE id=NEW.current_pricing_policy_id AND product_id=NEW.id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pricing window belongs to another product';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER product_pricing BEFORE INSERT OR UPDATE ON app.booster_product
 FOR EACH ROW EXECUTE FUNCTION app.guard_product_pricing();
CREATE TRIGGER pricing_policy_history BEFORE UPDATE OR DELETE ON app.supply_pricing_policy
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.guard_supply_stock() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE product app.booster_product; recipe app.booster_recipe;
BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stock provenance cannot be deleted'; END IF;
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=NEW.recipe_id;
 SELECT * INTO STRICT product FROM app.booster_product WHERE id=recipe.product_id FOR NO KEY UPDATE;
 IF recipe.qualified_at IS NULL OR NOT EXISTS(SELECT 1 FROM app.supply_pricing_policy WHERE id=NEW.pricing_policy_id AND product_id=product.id)
 OR (NEW.remaining_quantity>0 AND NEW.pricing_policy_id IS DISTINCT FROM product.current_pricing_policy_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stock requires its qualified current product window';
 END IF;
 IF NEW.drop_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.supply_drop WHERE id=NEW.drop_id AND set_id=product.set_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='drop does not supply this product set';
 END IF;
 IF TG_OP='INSERT' AND NEW.remaining_quantity<>NEW.starting_quantity THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new allocation must begin at its full authored quantity';
 END IF;
 IF TG_OP='UPDATE' AND (
 (NEW.id,NEW.recipe_id,NEW.drop_id,NEW.origin,NEW.starting_quantity,NEW.allocation_ref,NEW.coverage_ref)
 IS DISTINCT FROM (OLD.id,OLD.recipe_id,OLD.drop_id,OLD.origin,OLD.starting_quantity,OLD.allocation_ref,OLD.coverage_ref)
 OR NEW.remaining_quantity>OLD.remaining_quantity
 OR (NEW.pricing_policy_id IS DISTINCT FROM OLD.pricing_policy_id AND NEW.remaining_quantity<>OLD.remaining_quantity)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stock source or window rebind cannot change its allocation';
 END IF;
 NEW.revision:=CASE WHEN TG_OP='INSERT' THEN 0 ELSE OLD.revision+1 END;
 UPDATE app.booster_product SET pricing_revision=pricing_revision+1 WHERE id=product.id;
 RETURN NEW;
END $$;
CREATE TRIGGER stock_source BEFORE INSERT OR UPDATE OR DELETE ON app.supply_stock
 FOR EACH ROW EXECUTE FUNCTION app.guard_supply_stock();

CREATE FUNCTION app.check_stock_window() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE v_product_id uuid; current_policy uuid; starting numeric; remaining numeric;
BEGIN
 IF TG_TABLE_NAME='booster_product' THEN v_product_id:=NEW.id;
 ELSE SELECT r.product_id INTO v_product_id FROM app.booster_recipe r WHERE id=NEW.recipe_id; END IF;
 SELECT p.current_pricing_policy_id,w.window_starting_quantity INTO current_policy,starting
 FROM app.booster_product p LEFT JOIN app.supply_pricing_policy w ON w.id=p.current_pricing_policy_id WHERE p.id=v_product_id;
 IF EXISTS(SELECT 1 FROM app.supply_stock s JOIN app.booster_recipe r ON r.id=s.recipe_id
 WHERE r.product_id=v_product_id AND s.remaining_quantity>0 AND s.pricing_policy_id IS DISTINCT FROM current_policy) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='positive product stock cannot span current pricing windows';
 END IF;
 SELECT coalesce(sum(remaining_quantity),0) INTO remaining FROM app.supply_stock WHERE pricing_policy_id=current_policy;
 IF current_policy IS NOT NULL AND remaining>starting THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='product stock exceeds its published window';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER stock_window AFTER INSERT OR UPDATE ON app.supply_stock
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_stock_window();
CREATE CONSTRAINT TRIGGER product_window AFTER INSERT OR UPDATE ON app.booster_product
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_stock_window();

CREATE FUNCTION app.publish_supply_window(p_recipe uuid,p_quantity numeric,p_normal numeric,p_modifier_n numeric,p_modifier_d numeric,
 p_modifier_ref text,p_revision text,p_origin text,p_drop uuid,p_allocation_ref text,p_coverage text)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE recipe app.booster_recipe; product app.booster_product; existing app.supply_pricing_policy;
 payload jsonb; total numeric; created uuid;
BEGIN
 IF p_quantity IS NULL OR p_normal IS NULL OR NOT(p_quantity>0 AND p_quantity<power(10::numeric,78) AND scale(p_quantity)=0)
 OR NOT(p_normal>0 AND p_normal<power(10::numeric,78) AND scale(p_normal)=0)
 OR p_modifier_n IS NULL OR p_modifier_d IS NULL OR NOT(p_modifier_n>0 AND p_modifier_n<power(10::numeric,78) AND scale(p_modifier_n)=0)
 OR NOT(p_modifier_d>0 AND p_modifier_d<power(10::numeric,78) AND scale(p_modifier_d)=0)
 OR p_modifier_ref IS NULL OR p_modifier_ref !~ '^sha256:[0-9a-f]{64}$' OR p_allocation_ref IS NULL OR p_allocation_ref !~ '^sha256:[0-9a-f]{64}$'
 OR p_revision IS NULL OR length(p_revision) NOT BETWEEN 1 AND 128 OR p_origin IS NULL OR p_origin NOT IN ('baseline','quarterly','introduction') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid qualified allocation or price inputs';
 END IF;
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=p_recipe;
 IF recipe.qualified_at IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe not qualified'; END IF;
 SELECT * INTO STRICT product FROM app.booster_product WHERE id=recipe.product_id FOR NO KEY UPDATE;
 payload:=jsonb_build_object('recipe',p_recipe,'quantity',p_quantity::text,'normal',p_normal::text,'modifierN',p_modifier_n::text,
 'modifierD',p_modifier_d::text,'modifierRef',p_modifier_ref,'origin',p_origin,'drop',p_drop,'allocation',p_allocation_ref,'coverage',p_coverage);
 SELECT * INTO existing FROM app.supply_pricing_policy WHERE revision_ref=p_revision;
 IF FOUND THEN
  IF existing.product_id<>product.id OR existing.publication_payload<>payload THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='publication identity reused for different allocation';
  END IF;
  RETURN existing.id;
 END IF;
 SELECT coalesce(sum(s.remaining_quantity),0)+p_quantity INTO total FROM app.supply_stock s WHERE s.recipe_id=p_recipe;
 INSERT INTO app.supply_pricing_policy(product_id,revision_ref,base_price_coins,product_modifier_ref,window_starting_quantity,
 normal_quantity,published_at,formula_ref,modifier_numerator,modifier_denominator,publication_payload)
 VALUES(product.id,p_revision,1000,p_modifier_ref,total,p_normal,clock_timestamp(),'scarcity-depletion-v1',p_modifier_n,p_modifier_d,payload) RETURNING id INTO created;
 UPDATE app.booster_product SET current_pricing_policy_id=created,pricing_revision=pricing_revision+1 WHERE id=product.id;
 UPDATE app.supply_stock SET pricing_policy_id=created WHERE recipe_id=p_recipe AND remaining_quantity>0;
 INSERT INTO app.supply_stock(recipe_id,drop_id,origin,pricing_policy_id,starting_quantity,remaining_quantity,revision,allocation_ref,coverage_ref)
 VALUES(p_recipe,p_drop,p_origin,created,p_quantity,p_quantity,0,p_allocation_ref,p_coverage);
 RETURN created;
END $$;
GRANT EXECUTE ON FUNCTION app.publish_supply_window(uuid,numeric,numeric,numeric,numeric,text,text,text,uuid,text,text) TO service_role;

CREATE FUNCTION app.window_unit_price(p_policy app.supply_pricing_policy,p_remaining numeric) RETURNS numeric
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path='' AS $$
DECLARE n numeric; d numeric; result numeric;
BEGIN
 IF p_remaining<0 OR p_remaining>p_policy.window_starting_quantity OR scale(p_remaining)<>0 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid pricing remainder';
 END IF;
 n:=p_policy.base_price_coins*p_policy.modifier_numerator*greatest(p_policy.window_starting_quantity,p_policy.normal_quantity)
 *(2*p_policy.window_starting_quantity-p_remaining);
 d:=p_policy.modifier_denominator*p_policy.window_starting_quantity*p_policy.window_starting_quantity;
 result:=div(n+d-1,d);
 IF result>=power(10::numeric,78) THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='price exceeds application amount precision'; END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.quote_packs(p_recipe uuid,p_quantity integer) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE recipe app.booster_recipe; product app.booster_product; policy app.supply_pricing_policy;
 remaining numeric; total numeric:=0; price numeric; index integer;
BEGIN
 IF p_quantity IS NULL OR p_quantity NOT BETWEEN 1 AND 1000 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='pack quantity must be between 1 and 1000'; END IF;
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=p_recipe;
 IF recipe.qualified_at IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe not qualified'; END IF;
 SELECT * INTO STRICT product FROM app.booster_product WHERE id=recipe.product_id FOR NO KEY UPDATE;
 IF product.current_pricing_policy_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='product has no published pricing basis'; END IF;
 SELECT * INTO STRICT policy FROM app.supply_pricing_policy WHERE id=product.current_pricing_policy_id;
 SELECT coalesce(sum(remaining_quantity),0) INTO remaining FROM app.supply_stock WHERE pricing_policy_id=policy.id;
 price:=app.window_unit_price(policy,remaining);
 IF remaining>=p_quantity THEN
  FOR index IN 0..p_quantity-1 LOOP total:=total+app.window_unit_price(policy,remaining-index); END LOOP;
 ELSE total:=NULL;
 END IF;
 IF total>=power(10::numeric,78) THEN RAISE EXCEPTION USING ERRCODE='22003',MESSAGE='purchase total exceeds application amount precision'; END IF;
 RETURN jsonb_build_object('recipeId',recipe.id,'productId',product.id,'policyId',policy.id,'revision',product.pricing_revision::text,
 'remaining',remaining::text,'unitPrice',price::text,'quantity',p_quantity::text,'total',total::text,
 'purpose',CASE WHEN total IS NULL THEN 'valuation_only' ELSE 'purchase' END);
END $$;
GRANT EXECUTE ON FUNCTION app.quote_packs(uuid,integer) TO service_role;

REVOKE INSERT,UPDATE,DELETE ON app.supply_stock,app.supply_pricing_policy FROM service_role;

COMMIT;
