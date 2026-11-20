-- Seed-free owner projection over the existing immutable pack-opening receipt.
BEGIN;
CREATE FUNCTION app.read_my_pack_open(p_open uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; outcome jsonb;
BEGIN
 IF p_open IS NULL OR p_open='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('pack','unavailable')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 BEGIN
  outcome:=app.read_pack_open(actor,p_open);
 EXCEPTION WHEN insufficient_privilege THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('pack','unavailable'));
 END;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'outcome',outcome));
END $$;
REVOKE ALL ON FUNCTION app.read_my_pack_open(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_pack_open(uuid) TO authenticated;
REVOKE EXECUTE ON FUNCTION app.read_pack_open(uuid,uuid) FROM service_role;
COMMIT;
