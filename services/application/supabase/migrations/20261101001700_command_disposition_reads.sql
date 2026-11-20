BEGIN;

CREATE FUNCTION app.read_account_command(p_actor uuid,p_scope text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 IF p_scope IS NULL OR length(p_scope) NOT BETWEEN 1 AND 64 OR p_request IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid command query';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending')) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='account unavailable';
 END IF;
 SELECT * INTO receipt FROM app.command_receipt
 WHERE principal_key='human:'||p_actor::text AND scope=p_scope AND request_ref=p_request::text;
 IF NOT FOUND THEN RETURN jsonb_build_object('state','unknown','response',NULL); END IF;
 RETURN jsonb_build_object('state',receipt.state,'response',receipt.response_payload);
END $$;
GRANT EXECUTE ON FUNCTION app.read_account_command(uuid,text,uuid) TO service_role;

COMMIT;
