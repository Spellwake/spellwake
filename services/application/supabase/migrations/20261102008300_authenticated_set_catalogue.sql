-- All accepted catalogue sets, including sets without a qualified purchasable product.
BEGIN;
CREATE FUNCTION app.read_my_sets(p_query text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; items jsonb;
BEGIN
 IF p_query IS NULL OR length(p_query)>1024 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT coalesce(jsonb_agg(to_jsonb(page) ORDER BY page.id),'[]') INTO items FROM (
  SELECT s.id,s.name,s.code FROM app.card_set s
  WHERE (strpos(lower(s.name),lower(btrim(p_query)))>0 OR strpos(lower(s.code),lower(btrim(p_query)))>0)
   AND (p_after IS NULL OR s.id>p_after) ORDER BY s.id LIMIT p_limit) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'id' END));
END $$;
REVOKE ALL ON FUNCTION app.read_my_sets(text,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_sets(text,uuid,integer) TO authenticated;
COMMIT;
