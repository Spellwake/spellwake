BEGIN;

-- Idempotent qualification/publication retries retain their epoch as well as
-- their source allocation. Only a new accepted catalogue mutation advances it.
CREATE OR REPLACE FUNCTION app.lock_valuation_catalogue(p_write boolean) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE policy uuid;
BEGIN
 IF p_write THEN
  SELECT id INTO policy FROM app.valuation_policy_revision WHERE state='active' FOR NO KEY UPDATE;
 ELSE
  SELECT id INTO policy FROM app.valuation_policy_revision WHERE state='active' FOR SHARE;
 END IF;
 IF policy IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='valuation policy is not configured'; END IF;
 RETURN policy;
END $$;


CREATE FUNCTION app.advance_valuation_catalogue() RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 UPDATE app.valuation_policy_revision SET catalogue_revision=catalogue_revision+1 WHERE state='active';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='valuation policy is not configured'; END IF;
END $$;

CREATE OR REPLACE FUNCTION app.qualify_booster_recipe(p_recipe uuid,p_manifest text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE existing text;
BEGIN
 PERFORM app.lock_valuation_catalogue(true);
 SELECT qualification_ref INTO existing FROM app.booster_recipe WHERE id=p_recipe FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='recipe unavailable'; END IF;
 IF existing IS NOT NULL THEN
  IF existing IS DISTINCT FROM p_manifest THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe qualification changed'; END IF;
  RETURN;
 END IF;
 PERFORM app.advance_valuation_catalogue();
 UPDATE app.booster_recipe SET qualified_at=clock_timestamp(),qualification_ref=p_manifest WHERE id=p_recipe;
END $$;


CREATE OR REPLACE FUNCTION app.publish_supply_window(p_recipe uuid,p_quantity numeric,p_normal numeric,p_modifier_n numeric,p_modifier_d numeric,
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
 PERFORM app.lock_valuation_catalogue(true);
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
 PERFORM app.advance_valuation_catalogue();
 INSERT INTO app.supply_pricing_policy(product_id,revision_ref,base_price_coins,product_modifier_ref,window_starting_quantity,
 normal_quantity,published_at,formula_ref,modifier_numerator,modifier_denominator,publication_payload)
 VALUES(product.id,p_revision,1000,p_modifier_ref,total,p_normal,clock_timestamp(),'scarcity-depletion-v1',p_modifier_n,p_modifier_d,payload) RETURNING id INTO created;
 UPDATE app.booster_product SET current_pricing_policy_id=created,pricing_revision=pricing_revision+1 WHERE id=product.id;
 UPDATE app.supply_stock SET pricing_policy_id=created WHERE recipe_id=p_recipe AND remaining_quantity>0;
 INSERT INTO app.supply_stock(recipe_id,drop_id,origin,pricing_policy_id,starting_quantity,remaining_quantity,revision,allocation_ref,coverage_ref)
 VALUES(p_recipe,p_drop,p_origin,created,p_quantity,p_quantity,0,p_allocation_ref,p_coverage);
 RETURN created;
END $$;


COMMIT;
