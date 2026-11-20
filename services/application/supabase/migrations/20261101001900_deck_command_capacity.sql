BEGIN;

-- Up to 1000 canonical deck entries (including 78-digit quantities and exact
-- printing IDs) fit this bounded shared command envelope. Client bodies remain
-- bounded before parsing at the future HTTP boundary.
CREATE OR REPLACE FUNCTION app.begin_account_command(p_actor uuid,p_request uuid,p_scope text,p_target text,p_payload jsonb)
RETURNS app.command_receipt LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result app.command_receipt; fingerprint text;
BEGIN
 IF p_actor IS NULL OR p_request IS NULL OR p_scope IS NULL OR p_target IS NULL OR p_payload IS NULL
 OR length(p_scope)>64 OR length(p_target)>128 OR octet_length(p_payload::text)>262144 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid command identity or payload bounds';
 END IF;
 fingerprint:=encode(sha256(convert_to(p_payload::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(account_id,principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES(p_actor,'human:'||p_actor::text,p_request::text,p_scope,p_target,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO result;
 IF FOUND THEN RETURN result; END IF;
 SELECT * INTO STRICT result FROM app.command_receipt
 WHERE principal_key='human:'||p_actor::text AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF result.target_ref<>p_target OR result.payload_hash<>fingerprint THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='request identity was used for different command terms';
 END IF;
 IF result.state='pending' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='command disposition is not yet terminal';
 END IF;
 RETURN result;
END $$;

COMMIT;
