BEGIN;

ALTER TABLE app.account ADD CONSTRAINT supported_username CHECK (username ~ '^[A-Za-z0-9_]{3,24}$');
ALTER TABLE app.profile ADD CONSTRAINT complete_art_crop CHECK (
 (printing_id IS NULL AND crop_x IS NULL AND crop_y IS NULL AND crop_scale IS NULL)
 OR (printing_id IS NOT NULL AND crop_x IS NOT NULL AND crop_y IS NOT NULL AND crop_scale IS NOT NULL
 AND crop_x BETWEEN 0 AND 1 AND crop_y BETWEEN 0 AND 1 AND crop_scale>0));

-- Service-only registration: subject/method must come from the verified Auth adapter,
-- and free cosmetic IDs from qualified server configuration, not the user's payload.
CREATE FUNCTION app.register_account(p_subject uuid,p_username text,p_method text,p_back uuid,p_sleeve uuid,p_dice uuid)
RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result uuid; existing_method text; existing_state text; owner uuid; coin uuid; inventory uuid; operation uuid;
BEGIN
 PERFORM pg_advisory_xact_lock(hashtextextended('signup:'||p_subject::text,0));
 SELECT id,sign_in_method,state INTO result,existing_method,existing_state FROM app.account WHERE auth_subject=p_subject;
 IF FOUND THEN
  IF existing_method<>p_method OR existing_state<>'active' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account sign-in method or lifecycle does not permit registration';
  END IF;
  RETURN result;
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_back AND kind='back' AND state='active')
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_sleeve AND kind='sleeve' AND state='active')
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_dice AND kind='dice' AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified free cosmetics are not configured';
 END IF;
 INSERT INTO app.account(auth_subject,username,sign_in_method,created_at,state)
 VALUES(p_subject,p_username,p_method,clock_timestamp(),'active') RETURNING id INTO result;
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
 SELECT result,cosmetic,'free','active' FROM unnest(ARRAY[p_back,p_sleeve,p_dice]) AS cosmetic;
 RETURN result;
END $$;
REVOKE EXECUTE ON FUNCTION app.register_account(uuid,text,text,uuid,uuid,uuid) FROM PUBLIC,anon,authenticated;
GRANT EXECUTE ON FUNCTION app.register_account(uuid,text,text,uuid,uuid,uuid) TO service_role;

COMMIT;
