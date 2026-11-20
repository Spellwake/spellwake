BEGIN;

-- Supersedes the service-only actor-selected read. The caller's original JWT
-- now reaches an invoker function and the existing owner RLS policies.
DROP FUNCTION app.read_inventory_page(uuid,uuid,integer);
CREATE FUNCTION app.read_my_inventory_page(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE result jsonb; actor uuid; session_state jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('limit','must_be_between_1_and_100'));
 END IF;
 actor:=app.current_account_id();
 IF actor IS NULL THEN
  session_state:=app.read_current_session();
  IF session_state->>'status'='fail' THEN RETURN session_state; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT i.id,jsonb_build_object('id',i.id,'assetId',d.id,'revision',i.revision::text,
   'owned',a.owned::text,'held',a.held::text,'available',a.available::text,'asset',
   CASE d.kind WHEN 'coin' THEN jsonb_build_object('kind','coin','currencyKey',d.currency_key)
    WHEN 'card' THEN jsonb_build_object('kind','card','printingId',d.printing_id)
    WHEN 'pack' THEN jsonb_build_object('kind','pack','recipeId',d.recipe_id) END) AS entry
  FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id
  JOIN app.asset_definition d ON d.id=i.asset_definition_id JOIN app.asset_availability a ON a.id=i.id
  WHERE o.account_id=actor AND i.owned_quantity>0 AND (p_after IS NULL OR i.id>p_after)
  ORDER BY i.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;
REVOKE EXECUTE ON FUNCTION app.read_my_inventory_page(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_inventory_page(uuid,integer) TO authenticated;

-- Only the established writer creates account/owner/grants/ledger rows. This
-- facade translates known business failures without disclosing vendor text.
CREATE FUNCTION app.register_account_command(p_subject uuid,p_session uuid,p_username text,p_back uuid,p_sleeve uuid,p_dice uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result uuid; violated text; reason text;
BEGIN
 result:=app.register_account(p_subject,p_session,p_username,p_back,p_sleeve,p_dice);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',result));
EXCEPTION
 WHEN unique_violation THEN
  GET STACKED DIAGNOSTICS violated=CONSTRAINT_NAME;
  IF violated IN ('account_username_key','account_normalized_username') THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('username','unavailable'));
  END IF;
  RAISE;
 WHEN invalid_authorization_specification THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
 WHEN check_violation THEN
  GET STACKED DIAGNOSTICS reason=MESSAGE_TEXT;
  IF reason='account method or lifecycle does not permit registration' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
  END IF;
  RAISE;
END $$;
REVOKE EXECUTE ON FUNCTION app.register_account_command(uuid,uuid,text,uuid,uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.register_account_command(uuid,uuid,text,uuid,uuid,uuid) TO service_role;

COMMIT;
