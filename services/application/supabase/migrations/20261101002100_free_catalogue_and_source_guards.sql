BEGIN;

-- Defaults are explicit server configuration; all approved configured free
-- catalogue choices are granted, including unselected backs and sleeves.
CREATE OR REPLACE FUNCTION app.register_account(p_subject uuid,p_username text,p_method text,p_back uuid,p_sleeve uuid,p_dice uuid)
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
 IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_back AND kind='back' AND state='active' AND is_free)
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_sleeve AND kind='sleeve' AND state='active' AND is_free)
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=p_dice AND kind='dice' AND state='active' AND is_free) THEN
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
 SELECT result,id,'free','active' FROM app.cosmetic WHERE is_free AND state='active';
 RETURN result;
END $$;

CREATE FUNCTION app.guard_provider_event() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR
 (OLD.processing_state IN ('verified','processed') AND
  ((NEW.id,NEW.provider,NEW.external_event_ref,NEW.account_id,NEW.received_at,NEW.event_type,NEW.payload_ref)
  IS DISTINCT FROM (OLD.id,OLD.provider,OLD.external_event_ref,OLD.account_id,OLD.received_at,OLD.event_type,OLD.payload_ref)
  OR NEW.processing_state NOT IN ('verified','processed'))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='verified provider source cannot be rebound';
 END IF;
 RETURN NEW;
END $$;
ALTER TABLE app.provider_event ADD CHECK (provider IN ('stripe','patreon'));
ALTER TABLE app.provider_event ADD CHECK (processing_state IN ('received','verified','processed','rejected'));
CREATE TRIGGER provider_event_source BEFORE UPDATE OR DELETE ON app.provider_event
 FOR EACH ROW EXECUTE FUNCTION app.guard_provider_event();

CREATE FUNCTION app.check_saved_cosmetic_slots() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE back uuid; sleeve uuid; dice uuid; playmat uuid;
BEGIN
 IF TG_TABLE_NAME='profile' THEN
  back:=NEW.default_back_id; sleeve:=NEW.default_sleeve_id; dice:=NEW.default_dice_id; playmat:=NEW.default_playmat_id;
 ELSE
  back:=NEW.back_id; sleeve:=NEW.sleeve_id; dice:=NEW.dice_id; playmat:=NEW.playmat_id;
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=back AND kind='back')
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=sleeve AND kind='sleeve')
 OR NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=dice AND kind='dice')
 OR (playmat IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.cosmetic WHERE id=playmat AND kind='playmat')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='cosmetic selection belongs to another slot';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER profile_cosmetic_slots BEFORE INSERT OR UPDATE ON app.profile
 FOR EACH ROW EXECUTE FUNCTION app.check_saved_cosmetic_slots();
CREATE TRIGGER deck_cosmetic_slots BEFORE INSERT OR UPDATE ON app.deck
 FOR EACH ROW EXECUTE FUNCTION app.check_saved_cosmetic_slots();

COMMIT;
