-- Public product labels and current quotes, authorized by the original caller.
BEGIN;
CREATE FUNCTION app.pack_product_json(p_recipe app.booster_recipe) RETURNS jsonb
LANGUAGE sql STABLE SET search_path='' AS $$
 SELECT jsonb_build_object('id',p_recipe.id,'productId',p.id,'setId',s.id,
  'name',p.name,'setName',s.name,'revisionRef',p_recipe.revision_ref)
 FROM app.booster_product p JOIN app.card_set s ON s.id=p.set_id WHERE p.id=p_recipe.product_id
$$;
CREATE FUNCTION app.read_my_pack_products(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; items jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT coalesce(jsonb_agg(page.item ORDER BY page.id),'[]') INTO items FROM (
  SELECT r.id,app.pack_product_json(r) item FROM app.booster_recipe r JOIN app.booster_product p ON p.id=r.product_id
  WHERE r.qualified_at IS NOT NULL AND r.opening_qualified_at IS NOT NULL AND p.current_pricing_policy_id IS NOT NULL
   AND (p_after IS NULL OR r.id>p_after) ORDER BY r.id LIMIT p_limit) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'id' END));
END $$;
CREATE FUNCTION app.read_my_pack_quote(p_recipe uuid,p_quantity integer) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; quote jsonb;
BEGIN
 IF p_quantity IS NULL OR p_quantity NOT BETWEEN 1 AND 1000 THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('quantity','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe r JOIN app.booster_product p ON p.id=r.product_id
  WHERE r.id=p_recipe AND r.qualified_at IS NOT NULL AND r.opening_qualified_at IS NOT NULL AND p.current_pricing_policy_id IS NOT NULL) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('product','unavailable')); END IF;
 quote:=app.quote_packs(p_recipe,p_quantity);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'quote',quote));
END $$;
REVOKE ALL ON FUNCTION app.pack_product_json(app.booster_recipe),app.read_my_pack_products(uuid,integer),app.read_my_pack_quote(uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_pack_products(uuid,integer),app.read_my_pack_quote(uuid,integer) TO authenticated;
COMMIT;
