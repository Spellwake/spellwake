BEGIN;

ALTER TABLE app.deck ADD CONSTRAINT deck_provenance CHECK (
 (source_kind='account' AND account_id IS NOT NULL AND system_pool_ref IS NULL AND source_manifest_ref IS NULL)
 OR (source_kind='system_pool' AND account_id IS NULL AND system_pool_ref IS NOT NULL AND source_manifest_ref IS NOT NULL));
ALTER TABLE app.deck ADD CONSTRAINT deck_mode CHECK (mode IN ('open','collection'));
ALTER TABLE app.deck ADD CONSTRAINT deck_name CHECK (length(btrim(name)) BETWEEN 1 AND 120);
ALTER TABLE app.deck_revision ADD COLUMN sealed_at timestamptz;
ALTER TABLE app.deck_revision ADD CHECK (revision_number>0);
ALTER TABLE app.deck_entry ADD CHECK (quantity>0 AND section ~ '^[a-z][a-z0-9_]{0,31}$');
CREATE UNIQUE INDEX deck_entry_identity ON app.deck_entry(revision_id,definition_id,printing_id,section) NULLS NOT DISTINCT;
CREATE INDEX deck_library_owner ON app.deck(account_id,id);
CREATE INDEX deck_entries_revision ON app.deck_entry(revision_id,id);

CREATE FUNCTION app.guard_deck_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='DELETE' OR OLD.sealed_at IS NOT NULL OR
 (NEW.id,NEW.deck_id,NEW.revision_number,NEW.created_at) IS DISTINCT FROM
 (OLD.id,OLD.deck_id,OLD.revision_number,OLD.created_at) OR NEW.sealed_at IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='saved deck revision is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER deck_revision_history BEFORE UPDATE OR DELETE ON app.deck_revision
 FOR EACH ROW EXECUTE FUNCTION app.guard_deck_revision();

CREATE FUNCTION app.check_deck_revision_sealed() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM app.deck_revision WHERE id=NEW.id AND sealed_at IS NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deck revision must be sealed before commit';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER deck_revision_complete AFTER INSERT OR UPDATE ON app.deck_revision
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_deck_revision_sealed();

CREATE FUNCTION app.guard_printing_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.definition_id,NEW.set_id,NEW.external_printing_ref,NEW.finish) IS DISTINCT FROM
 (OLD.id,OLD.definition_id,OLD.set_id,OLD.external_printing_ref,OLD.finish) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='printing identity is immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER printing_identity BEFORE UPDATE ON app.printing
 FOR EACH ROW EXECUTE FUNCTION app.guard_printing_identity();

CREATE FUNCTION app.guard_deck_entry() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE sealed timestamptz;
BEGIN
 IF TG_OP<>'INSERT' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='saved deck entry is immutable';
 END IF;
 SELECT sealed_at INTO sealed FROM app.deck_revision WHERE id=NEW.revision_id FOR NO KEY UPDATE;
 IF NOT FOUND OR sealed IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deck revision does not accept entries';
 END IF;
 IF NEW.printing_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=NEW.printing_id AND definition_id=NEW.definition_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='printing does not belong to card definition';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER deck_entry_history BEFORE INSERT OR UPDATE OR DELETE ON app.deck_entry
 FOR EACH ROW EXECUTE FUNCTION app.guard_deck_entry();

CREATE FUNCTION app.guard_deck_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.account_id,NEW.source_kind,NEW.system_pool_ref,NEW.source_manifest_ref)
 IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.source_kind,OLD.system_pool_ref,OLD.source_manifest_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='deck owner and source are immutable';
 END IF;
 IF NEW.current_revision_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.deck_revision
 WHERE id=NEW.current_revision_id AND deck_id=NEW.id AND sealed_at IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='current deck revision must belong to this deck and be sealed';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER deck_identity BEFORE INSERT OR UPDATE ON app.deck
 FOR EACH ROW EXECUTE FUNCTION app.guard_deck_identity();

CREATE FUNCTION app.has_cosmetic(p_actor uuid,p_cosmetic uuid,p_kind text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.cosmetic c JOIN app.cosmetic_grant g ON g.cosmetic_id=c.id
 WHERE c.id=p_cosmetic AND c.kind=p_kind AND c.state='active' AND g.account_id=p_actor AND g.state='active')
$$;

CREATE FUNCTION app.change_profile_defaults(p_actor uuid,p_back uuid,p_sleeve uuid,p_dice uuid,p_playmat uuid,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; profile_id uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'profile_defaults',p_actor::text,
  jsonb_build_object('back',p_back,'sleeve',p_sleeve,'dice',p_dice,'playmat',p_playmat));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF NOT app.has_cosmetic(p_actor,p_back,'back') OR NOT app.has_cosmetic(p_actor,p_sleeve,'sleeve')
 OR NOT app.has_cosmetic(p_actor,p_dice,'dice') OR (p_playmat IS NOT NULL AND NOT app.has_cosmetic(p_actor,p_playmat,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','cosmetic selection unavailable'));
 END IF;
 UPDATE app.profile SET default_back_id=p_back,default_sleeve_id=p_sleeve,default_dice_id=p_dice,default_playmat_id=p_playmat
 WHERE account_id=p_actor RETURNING id INTO profile_id;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='account profile missing'; END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',profile_id));
END $$;

CREATE FUNCTION app.create_deck(p_actor uuid,p_name text,p_mode text,p_format uuid,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; defaults app.profile; created uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'deck_create',p_actor::text,
  jsonb_build_object('name',p_name,'mode',p_mode,'format',p_format));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 IF p_name IS NULL OR length(btrim(p_name)) NOT BETWEEN 1 AND 120 OR p_mode IS NULL OR p_mode NOT IN ('open','collection')
 OR NOT EXISTS(SELECT 1 FROM app.format WHERE id=p_format) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','invalid deck metadata'));
 END IF;
 SELECT * INTO STRICT defaults FROM app.profile WHERE account_id=p_actor FOR NO KEY UPDATE;
 IF NOT app.has_cosmetic(p_actor,defaults.default_back_id,'back') OR NOT app.has_cosmetic(p_actor,defaults.default_sleeve_id,'sleeve')
 OR NOT app.has_cosmetic(p_actor,defaults.default_dice_id,'dice') OR (defaults.default_playmat_id IS NOT NULL AND NOT app.has_cosmetic(p_actor,defaults.default_playmat_id,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','default cosmetic unavailable'));
 END IF;
 INSERT INTO app.deck(account_id,source_kind,mode,format_id,name,back_id,sleeve_id,dice_id,playmat_id)
 VALUES(p_actor,'account',p_mode,p_format,btrim(p_name),defaults.default_back_id,defaults.default_sleeve_id,defaults.default_dice_id,defaults.default_playmat_id)
 RETURNING id INTO created;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',created));
END $$;

-- Saving preserves user content, not an admission/legal/executable claim. A validator at
-- admission will separately check format, supported programs and Collection availability.
CREATE FUNCTION app.save_deck_contents(p_actor uuid,p_deck uuid,p_expected_revision uuid,p_entries jsonb,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; saved app.deck; revision uuid; next_number numeric; entry jsonb;
 v_definition uuid; v_printing uuid; section_name text; count_text text;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'deck_save',p_deck::text,
  jsonb_build_object('expectedRevision',p_expected_revision,'entries',p_entries));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO saved FROM app.deck WHERE id=p_deck AND account_id=p_actor AND source_kind='account' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','deck unavailable')); END IF;
 IF saved.current_revision_id IS DISTINCT FROM p_expected_revision THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','deck changed; reload before saving'));
 END IF;
 IF p_entries IS NULL OR jsonb_typeof(p_entries)<>'array' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('entries','expected deck entry array'));
 END IF;
 IF jsonb_array_length(p_entries)>1000 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('entries','too many deck entries'));
 END IF;
 -- Validate before creating a revision so failures leave no partial library state.
 BEGIN
  FOR entry IN SELECT value FROM jsonb_array_elements(p_entries) LOOP
   IF jsonb_typeof(entry)<>'object' OR (entry-ARRAY['definitionId','printingId','section','quantity'])<>'{}'::jsonb
   OR jsonb_typeof(entry->'definitionId') IS DISTINCT FROM 'string'
   OR jsonb_typeof(entry->'section') IS DISTINCT FROM 'string'
   OR jsonb_typeof(entry->'quantity') IS DISTINCT FROM 'string'
   OR (entry ? 'printingId' AND jsonb_typeof(entry->'printingId') NOT IN ('string','null')) THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid entry shape';
   END IF;
   v_definition:=(entry->>'definitionId')::uuid; v_printing:=(entry->>'printingId')::uuid;
   section_name:=entry->>'section'; count_text:=entry->>'quantity';
   IF section_name !~ '^[a-z][a-z0-9_]{0,31}$' OR count_text !~ '^[1-9][0-9]{0,77}$'
   OR NOT EXISTS(SELECT 1 FROM app.card_definition WHERE id=v_definition)
   OR (v_printing IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=v_printing AND definition_id=v_definition)) THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deck entry';
   END IF;
  END LOOP;
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(p_entries) e GROUP BY (e->>'definitionId')::uuid,(e->>'printingId')::uuid,e->>'section' HAVING count(*)>1) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='duplicate deck entry';
  END IF;
 EXCEPTION WHEN invalid_parameter_value OR invalid_text_representation THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('entries','invalid deck entries'));
 END;
 SELECT coalesce(max(revision_number),0)+1 INTO next_number FROM app.deck_revision WHERE deck_id=p_deck;
 INSERT INTO app.deck_revision(deck_id,revision_number,created_at) VALUES(p_deck,next_number,clock_timestamp()) RETURNING id INTO revision;
 INSERT INTO app.deck_entry(revision_id,definition_id,printing_id,section,quantity)
 SELECT revision,(e->>'definitionId')::uuid,(e->>'printingId')::uuid,e->>'section',(e->>'quantity')::numeric
 FROM jsonb_array_elements(p_entries) e;
 UPDATE app.deck_revision SET sealed_at=clock_timestamp() WHERE id=revision;
 UPDATE app.deck SET current_revision_id=revision WHERE id=p_deck;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',revision));
END $$;

CREATE FUNCTION app.can_read_deck(p_deck uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.deck WHERE id=p_deck AND account_id=app.current_account_id())
$$;
GRANT EXECUTE ON FUNCTION app.can_read_deck(uuid) TO authenticated;
GRANT EXECUTE ON FUNCTION app.change_profile_defaults(uuid,uuid,uuid,uuid,uuid,uuid),app.create_deck(uuid,text,text,uuid,uuid),
 app.save_deck_contents(uuid,uuid,uuid,jsonb,uuid) TO service_role;
GRANT SELECT ON app.profile,app.cosmetic_grant,app.cosmetic,app.deck,app.deck_revision,app.deck_entry TO authenticated;
CREATE POLICY own_profile_read ON app.profile FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY own_cosmetic_grant_read ON app.cosmetic_grant FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY active_cosmetic_read ON app.cosmetic FOR SELECT TO authenticated USING (state='active');
CREATE POLICY own_deck_read ON app.deck FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY own_deck_revision_read ON app.deck_revision FOR SELECT TO authenticated USING (app.can_read_deck(deck_id));
CREATE POLICY own_deck_entry_read ON app.deck_entry FOR SELECT TO authenticated USING
 (EXISTS(SELECT 1 FROM app.deck_revision r WHERE r.id=revision_id AND app.can_read_deck(r.deck_id)));

COMMIT;
