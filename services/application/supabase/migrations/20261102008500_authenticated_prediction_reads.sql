-- Original-caller projections over canonical anonymous depth and owner-only commitments.
BEGIN;
CREATE FUNCTION app.read_my_prediction_market(p_market uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; item jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 BEGIN item:=app.read_prediction_market(actor,p_market);
 EXCEPTION WHEN insufficient_privilege THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('market','unavailable')); END;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'market',jsonb_build_object('terms',item-'book','book',item->'book')));
END $$;
CREATE FUNCTION app.read_my_prediction_markets(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; page jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 page:=app.read_prediction_market_page(actor,p_after,p_limit);
 RETURN jsonb_build_object('status','success','data',page||jsonb_build_object('accountId',actor));
END $$;
CREATE FUNCTION app.read_my_prediction_orders(p_market uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; page jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid
 OR p_market='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 page:=app.read_prediction_orders(actor,p_market,p_after,p_limit);
 RETURN jsonb_build_object('status','success','data',page||jsonb_build_object('accountId',actor));
END $$;
CREATE FUNCTION app.read_my_prediction_positions(p_market uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; page jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid
 OR p_market='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 page:=app.read_prediction_positions(actor,p_market,p_after,p_limit);
 RETURN jsonb_build_object('status','success','data',page||jsonb_build_object('accountId',actor));
END $$;
REVOKE ALL ON FUNCTION app.read_my_prediction_market(uuid),app.read_my_prediction_markets(uuid,integer),
 app.read_my_prediction_orders(uuid,uuid,integer),app.read_my_prediction_positions(uuid,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_prediction_market(uuid),app.read_my_prediction_markets(uuid,integer),
 app.read_my_prediction_orders(uuid,uuid,integer),app.read_my_prediction_positions(uuid,uuid,integer) TO authenticated;
REVOKE EXECUTE ON FUNCTION app.read_prediction_market(uuid,uuid),app.read_prediction_market_page(uuid,uuid,integer),
 app.read_prediction_orders(uuid,uuid,uuid,integer),app.read_prediction_positions(uuid,uuid,uuid,integer) FROM service_role;
COMMIT;
