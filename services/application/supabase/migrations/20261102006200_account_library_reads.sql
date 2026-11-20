BEGIN;

-- Expose only owner-facing account columns, never the Auth/provider identity.
GRANT SELECT(id,username,state,appearance_revision) ON app.account TO authenticated;
CREATE POLICY own_account_read ON app.account FOR SELECT TO authenticated USING(id=app.current_account_id());
GRANT SELECT(id,key,name) ON app.format TO authenticated;
CREATE POLICY format_catalogue_read ON app.format FOR SELECT TO authenticated USING(true);

CREATE FUNCTION app.read_my_account() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT jsonb_build_object('accountId',a.id,'username',a.username,'state',a.state,'appearanceRevision',a.appearance_revision::text,
  'defaults',jsonb_build_object('backId',p.default_back_id,'sleeveId',p.default_sleeve_id,'diceId',p.default_dice_id,'playmatId',p.default_playmat_id),
  'wallet',(SELECT jsonb_build_object('owned',coalesce(sum(v.owned),0)::text,'held',coalesce(sum(v.held),0)::text,'available',coalesce(sum(v.available),0)::text)
   FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id
   JOIN app.asset_definition d ON d.id=i.asset_definition_id JOIN app.asset_availability v ON v.id=i.id
   WHERE o.account_id=actor AND d.kind='coin' AND d.currency_key='coin')) INTO result
 FROM app.account a JOIN app.profile p ON p.account_id=a.id WHERE a.id=actor;
 IF result IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account profile is unavailable'; END IF;
 RETURN jsonb_build_object('status','success','data',result);
END $$;

CREATE FUNCTION app.read_my_deck_library(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));
 END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT d.id,jsonb_build_object('id',d.id,'name',d.name,'mode',d.mode,'currentRevisionId',d.current_revision_id,
   'format',jsonb_build_object('id',f.id,'key',f.key,'name',f.name),
   'appearance',jsonb_build_object('backId',d.back_id,'sleeveId',d.sleeve_id,'diceId',d.dice_id,'playmatId',d.playmat_id)) entry
  FROM app.deck d JOIN app.format f ON f.id=d.format_id
  WHERE d.account_id=actor AND d.source_kind='account' AND (p_after IS NULL OR d.id>p_after)
  ORDER BY d.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;

REVOKE EXECUTE ON FUNCTION app.read_my_account(),app.read_my_deck_library(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_account(),app.read_my_deck_library(uuid,integer) TO authenticated;

COMMIT;
