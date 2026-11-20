BEGIN;
CREATE FUNCTION app.check_quarterly_stock_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE drop_row app.supply_drop; receipt app.command_receipt; opening app.command_receipt; candidate jsonb; product jsonb;
 total_weight numeric; assigned numeric; ordinal bigint; expected numeric;
BEGIN
 IF NEW.drop_id IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO STRICT drop_row FROM app.supply_drop WHERE id=NEW.drop_id;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='supply-season:'||drop_row.season_id::text AND scope='supply_ballot_result_v1' AND state='accepted';
 IF receipt.id IS NULL OR NEW.origin<>'quarterly' OR NEW.allocation_ref IS DISTINCT FROM drop_row.population_snapshot_ref THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stock requires its accepted quarterly allocation source'; END IF;
 SELECT * INTO opening FROM app.command_receipt WHERE id=(receipt.normalized_source->>'openingReceiptId')::uuid AND state='accepted' AND scope='supply_ballot_open_v1';
 SELECT value INTO candidate FROM jsonb_array_elements(opening.normalized_source->'candidates') WHERE value->>'setId'=drop_row.set_id::text;
 SELECT sum((p->>'weight')::numeric) INTO total_weight FROM jsonb_array_elements(candidate->'products') p;
 SELECT sum(div(drop_row.quantity*(p->>'weight')::numeric,total_weight)) INTO assigned FROM jsonb_array_elements(candidate->'products') p;
 SELECT value,index_value-1 INTO product,ordinal FROM(
  SELECT p value,row_number() OVER(ORDER BY r.product_id) index_value FROM jsonb_array_elements(candidate->'products') p JOIN app.booster_recipe r ON r.id=(p->>'recipeId')::uuid) products WHERE value->>'recipeId'=NEW.recipe_id::text;
 IF product IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quarterly stock recipe is outside frozen product configuration'; END IF;
 expected:=div(drop_row.quantity*(product->>'weight')::numeric,total_weight)+CASE WHEN ordinal<drop_row.quantity-assigned THEN 1 ELSE 0 END;
 IF expected<=0 OR NEW.starting_quantity<>expected OR (SELECT coalesce(sum(starting_quantity),0) FROM app.supply_stock WHERE drop_id=drop_row.id)<>drop_row.quantity
 OR EXISTS(SELECT 1 FROM app.supply_stock WHERE drop_id=drop_row.id AND (origin<>'quarterly' OR allocation_ref IS DISTINCT FROM drop_row.population_snapshot_ref)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='quarterly product weights and stock must conserve the immutable set budget'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER quarterly_stock_source AFTER INSERT OR UPDATE ON app.supply_stock DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_quarterly_stock_source();
REVOKE ALL ON FUNCTION app.check_quarterly_stock_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
