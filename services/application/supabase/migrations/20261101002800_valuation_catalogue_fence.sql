BEGIN;

-- Reuse the approved policy table as the catalogue epoch/fence. Shared readers
-- exclude qualification/publication while assessing all producing products;
-- actual counter writes make stale REPEATABLE READ snapshots retry as well.
ALTER TABLE app.valuation_policy_revision ADD COLUMN catalogue_revision numeric NOT NULL DEFAULT 0
 CHECK (catalogue_revision>=0 AND catalogue_revision<power(10::numeric,78) AND scale(catalogue_revision)=0);
CREATE UNIQUE INDEX active_valuation_policy ON app.valuation_policy_revision(state) WHERE state='active';
ALTER TABLE app.valuation_policy_revision ADD CHECK (state<>'active' OR
 (formula_ref='pack-marginal-v1' AND price_anchor_ref='current-published-pack-window-v1'));
INSERT INTO app.valuation_policy_revision(revision_ref,formula_ref,price_anchor_ref,state)
 VALUES('pack-marginal-v1','pack-marginal-v1','current-published-pack-window-v1','active');
REVOKE INSERT,UPDATE,DELETE ON app.valuation_policy_revision FROM service_role;
REVOKE UPDATE,DELETE ON app.booster_recipe FROM service_role;

CREATE FUNCTION app.lock_valuation_catalogue(p_write boolean) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE policy uuid;
BEGIN
 IF p_write THEN
  SELECT id INTO policy FROM app.valuation_policy_revision WHERE state='active' FOR NO KEY UPDATE;
  IF FOUND THEN UPDATE app.valuation_policy_revision SET catalogue_revision=catalogue_revision+1 WHERE id=policy; END IF;
 ELSE
  SELECT id INTO policy FROM app.valuation_policy_revision WHERE state='active' FOR SHARE;
 END IF;
 IF policy IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='valuation policy is not configured'; END IF;
 RETURN policy;
END $$;

CREATE OR REPLACE FUNCTION app.guard_product_pricing() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.current_pricing_policy_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new product must begin unpublished'; END IF;
  NEW.pricing_revision:=0;
 END IF;
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
 INSERT INTO app.supply_pricing_policy(product_id,revision_ref,base_price_coins,product_modifier_ref,window_starting_quantity,
 normal_quantity,published_at,formula_ref,modifier_numerator,modifier_denominator,publication_payload)
 VALUES(product.id,p_revision,1000,p_modifier_ref,total,p_normal,clock_timestamp(),'scarcity-depletion-v1',p_modifier_n,p_modifier_d,payload) RETURNING id INTO created;
 UPDATE app.booster_product SET current_pricing_policy_id=created,pricing_revision=pricing_revision+1 WHERE id=product.id;
 UPDATE app.supply_stock SET pricing_policy_id=created WHERE recipe_id=p_recipe AND remaining_quantity>0;
 INSERT INTO app.supply_stock(recipe_id,drop_id,origin,pricing_policy_id,starting_quantity,remaining_quantity,revision,allocation_ref,coverage_ref)
 VALUES(p_recipe,p_drop,p_origin,created,p_quantity,p_quantity,0,p_allocation_ref,p_coverage);
 RETURN created;
END $$;

CREATE OR REPLACE FUNCTION app.lock_assessment_products(p_assets uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 PERFORM app.lock_valuation_catalogue(false);
 -- Account/contact locks come first in settlement. Acquire every relevant product
 -- before assessing individual assets; input order cannot reverse product locks.
 PERFORM p.id FROM app.booster_product p WHERE EXISTS(
 SELECT 1 FROM app.asset_definition a WHERE a.id=ANY(p_assets) AND (
 (a.kind='pack' AND EXISTS(SELECT 1 FROM app.booster_recipe r WHERE r.id=a.recipe_id AND r.product_id=p.id)) OR
 (a.kind='card' AND EXISTS(SELECT 1 FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id
 JOIN app.booster_recipe r ON r.id=s.recipe_id WHERE c.printing_id=a.printing_id AND r.qualified_at IS NOT NULL AND r.product_id=p.id))))
 ORDER BY p.id FOR NO KEY UPDATE;
END $$;


CREATE OR REPLACE FUNCTION app.assess_bundle(p_lines jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE line jsonb; ids uuid[]:='{}'; asset uuid; quantity numeric; assessment jsonb;
 total app.exact_value:=app.value_make(0,1); weighted app.exact_value; lines jsonb:='[]'; v_policy uuid; v_epoch numeric;
BEGIN
 ids:=app.bundle_asset_ids(p_lines);
 PERFORM app.lock_assessment_products(ids);
 SELECT id,catalogue_revision INTO STRICT v_policy,v_epoch FROM app.valuation_policy_revision WHERE state='active';
 FOR line IN SELECT v FROM jsonb_array_elements(p_lines) v LOOP
  asset:=(line->>'assetId')::uuid; quantity:=(line->>'quantity')::numeric;
  assessment:=app.assess_asset(asset);
  weighted:=app.value_make((assessment->'value'->>'numerator')::numeric*quantity,(assessment->'value'->>'denominator')::numeric);
  total:=app.value_add(total,weighted);
  lines:=lines||jsonb_build_array(jsonb_build_object('assetId',asset,'quantity',quantity::text,'unitAssessment',assessment));
 END LOOP;
 RETURN jsonb_build_object('formula','pack-marginal-v1','policyId',v_policy,'catalogueRevision',v_epoch::text,'total',app.value_json(total),'lines',lines);
END $$;


COMMIT;
