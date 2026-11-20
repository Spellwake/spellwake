BEGIN;
-- An exact accepted restoration remains recoverable after later checkpoints or
-- host changes. A missing original still requires reconstruction of paused history.
CREATE FUNCTION app.read_native_restore_original(p_game uuid,p_request uuid,p_terms jsonb)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='recovery:'||p_game::text
  AND scope='game_restoration_v1' AND target_ref=p_game::text AND request_ref=p_request::text;
 IF NOT FOUND THEN RETURN NULL; END IF;
 IF receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(p_terms)
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restoration request conflict'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='restoration pending'; END IF;
 RETURN receipt.response_payload;
END $$;
REVOKE ALL ON FUNCTION app.read_native_restore_original(uuid,uuid,jsonb) FROM PUBLIC,anon,authenticated,supabase_auth_admin,service_role;
GRANT EXECUTE ON FUNCTION app.read_native_restore_original(uuid,uuid,jsonb) TO service_role;
COMMIT;
