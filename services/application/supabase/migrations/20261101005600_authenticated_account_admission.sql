BEGIN;

-- Read-only in effect, but VOLATILE deliberately refreshes lifecycle snapshots
-- after registration has waited for a concurrent account/retirement writer.
CREATE FUNCTION app.read_auth_session_authority(p_subject uuid,p_session uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE principal text:='auth:'||app.auth_uuid_hash(p_subject); binding app.command_receipt;
 grant_row app.command_receipt; method text; initial_class text;
BEGIN
 IF p_subject IS NULL OR p_session IS NULL OR EXISTS(SELECT 1 FROM app.command_receipt
  WHERE principal_key=principal AND scope='auth_retirement_v1' AND request_ref='primary') THEN RETURN NULL; END IF;
 SELECT * INTO binding FROM app.command_receipt WHERE principal_key=principal AND scope='auth_binding_v1' AND request_ref='primary';
 IF NOT FOUND OR binding.state<>'accepted' THEN RETURN NULL; END IF;
 PERFORM app.check_auth_receipt_shape(binding);
 SELECT * INTO grant_row FROM app.command_receipt WHERE principal_key=principal
  AND scope='auth_session_v1' AND request_ref=app.auth_uuid_hash(p_session);
 IF NOT FOUND OR grant_row.state<>'accepted' OR grant_row.target_ref<>binding.id::text THEN RETURN NULL; END IF;
 PERFORM app.check_auth_receipt_shape(grant_row);
 method:=(binding.result_ref::jsonb)->>'method';
 initial_class:=(grant_row.result_ref::jsonb)->>'initialClass';
 IF NOT app.auth_session_is_live(p_subject,p_session,initial_class) THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('version',1,'method',method,'bindingId',binding.id::text,'sessionGrantId',grant_row.id::text);
END $$;
REVOKE EXECUTE ON FUNCTION app.read_auth_session_authority(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.current_account_id() RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE claims jsonb; subject uuid; session_id uuid; authority jsonb; result uuid;
BEGIN
 claims:=nullif(current_setting('request.jwt.claims',true),'')::jsonb;
 IF jsonb_typeof(claims) IS DISTINCT FROM 'object' OR claims->>'role' IS DISTINCT FROM 'authenticated'
 OR (claims->'aud' IS DISTINCT FROM to_jsonb('authenticated'::text)
  AND claims->'aud' IS DISTINCT FROM jsonb_build_array('authenticated'))
 OR jsonb_typeof(claims->'exp') IS DISTINCT FROM 'number' THEN RETURN NULL; END IF;
 IF (claims->>'exp')::numeric<=extract(epoch FROM statement_timestamp()) THEN RETURN NULL; END IF;
 subject:=(claims->>'sub')::uuid; session_id:=(claims->>'session_id')::uuid;
 authority:=app.read_auth_session_authority(subject,session_id);
 IF authority IS NULL OR claims->'spellwake_auth' IS DISTINCT FROM authority THEN RETURN NULL; END IF;
 SELECT id INTO result FROM app.account WHERE auth_subject=subject AND state IN ('active','deletion_pending')
  AND sign_in_method=authority->>'method';
 RETURN result;
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range OR check_violation THEN
 RETURN NULL;
END $$;

-- Fresh system: the old caller-selected method signature is removed completely.
DROP FUNCTION app.register_account(uuid,text,text,uuid,uuid,uuid);
CREATE FUNCTION app.register_account(p_subject uuid,p_session uuid,p_username text,p_back uuid,p_sleeve uuid,p_dice uuid)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result uuid; existing_method text; existing_state text; authority jsonb;
 owner uuid; coin uuid; inventory uuid; operation uuid;
BEGIN
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='Auth subject unavailable'; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('signup:'||p_subject::text,0));
 SELECT id,sign_in_method,state INTO result,existing_method,existing_state FROM app.account
  WHERE auth_subject=p_subject FOR UPDATE;
 -- Recheck after account-lock waits, including a finalizer clearing the subject.
 authority:=app.read_auth_session_authority(p_subject,p_session);
 IF authority IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live Auth authority required'; END IF;
 IF result IS NOT NULL THEN
  IF existing_method IS DISTINCT FROM authority->>'method' OR existing_state<>'active' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account method or lifecycle does not permit registration';
  END IF;
  RETURN result;
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_back AND kind='back' AND state='active' AND is_free)
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_sleeve AND kind='sleeve' AND state='active' AND is_free)
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_dice AND kind='dice' AND state='active' AND is_free) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified free cosmetics are not configured';
 END IF;
 INSERT INTO app.account(auth_subject,username,sign_in_method,created_at,state)
 VALUES(p_subject,p_username,authority->>'method',clock_timestamp(),'active') RETURNING id INTO result;
 INSERT INTO app.profile(account_id,default_back_id,default_sleeve_id,default_dice_id)
 VALUES(result,p_back,p_sleeve,p_dice);
 INSERT INTO app.asset_owner(account_id,state) VALUES(result,'active') RETURNING id INTO owner;
 INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING;
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE kind='coin' AND currency_key='coin';
 inventory:=app.ensure_inventory(owner,coin);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES('signup',result::text,'signup',result,clock_timestamp(),'pending') RETURNING id INTO operation;
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
 VALUES(operation,inventory,'credit',14500,'signup',result::text);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 INSERT INTO app.cosmetic_grant(account_id,cosmetic_id,source_kind,state)
 SELECT result,id,'free','active' FROM app.cosmetic WHERE is_free AND state='active';
 RETURN result;
END $$;
REVOKE EXECUTE ON FUNCTION app.register_account(uuid,uuid,text,uuid,uuid,uuid) FROM PUBLIC,anon,authenticated,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.register_account(uuid,uuid,text,uuid,uuid,uuid) TO service_role;

COMMIT;
