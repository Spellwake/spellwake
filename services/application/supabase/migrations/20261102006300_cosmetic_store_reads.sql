BEGIN;

-- Names are curated catalogue metadata, not derived from private artwork references.
ALTER TABLE app.cosmetic ADD COLUMN display_name text
 CHECK(display_name IS NULL OR (char_length(display_name) BETWEEN 1 AND 120 AND btrim(display_name)=display_name));

CREATE FUNCTION app.read_my_cosmetic_catalogue(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));
 END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized')); END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT c.id,jsonb_build_object('id',c.id,'name',c.display_name,'kind',c.kind,'isFree',c.is_free,
   'isOwned',c.is_free OR EXISTS(SELECT 1 FROM app.cosmetic_grant g WHERE g.account_id=actor AND g.cosmetic_id=c.id AND g.state='active'),
   'includedWithPatreon',EXISTS(SELECT 1 FROM app.tier_cosmetic t WHERE t.cosmetic_id=c.id)) entry
  FROM app.cosmetic c WHERE c.state='active' AND c.display_name IS NOT NULL AND (p_after IS NULL OR c.id>p_after)
  ORDER BY c.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;

CREATE FUNCTION app.read_my_purchases(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));
 END IF;
 IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized')); END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT p.id,app.read_my_purchase(p.id)->'data' entry FROM app.purchase p
  WHERE p.account_id=actor AND (p_after IS NULL OR p.id>p_after) ORDER BY p.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'purchaseId' ELSE NULL END));
END $$;

REVOKE EXECUTE ON FUNCTION app.read_my_cosmetic_catalogue(uuid,integer),app.read_my_purchases(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_cosmetic_catalogue(uuid,integer),app.read_my_purchases(uuid,integer) TO authenticated;

COMMIT;
