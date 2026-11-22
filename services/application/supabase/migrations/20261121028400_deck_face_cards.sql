-- Deck face cards and official card art.
--
-- 1. Every printing's image reference is its official Scryfall art crop, built
--    from the Scryfall card ID the catalogue already stores. New printings get
--    it as they are published; existing ones are filled here.
-- 2. A deck may name a face card: a card in its current contents whose art is
--    shown on the deck box. Settings may set it (optional, so existing clients
--    are unchanged); saving contents keeps it valid, defaulting to the first
--    card in the save, which is the first card added in the editor.
-- 3. Deck reads return the face card with its art. Decks without a stored face
--    card fall back to a deterministic card from their contents.
--
-- Additive only: one nullable column, new helpers, and redefined functions whose
-- existing inputs and outputs keep their meaning. No existing field is removed.
BEGIN;

-- ---------------------------------------------------------------------------
-- 1. Official art for every printing
-- ---------------------------------------------------------------------------

-- Scryfall serves each card's images at a path derived from its card ID.
-- Returns NULL for anything that is not a card ID or a known image version.
CREATE FUNCTION app.scryfall_image_url(p_external text, p_version text) RETURNS text
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT CASE
  WHEN p_external IS NULL OR p_version IS NULL OR p_version NOT IN ('art_crop','normal')
  OR p_external !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' THEN NULL
  ELSE 'https://cards.scryfall.io/'||p_version||'/front/'||substr(p_external,1,1)||'/'
   ||substr(p_external,2,1)||'/'||p_external||'.jpg'
 END
$$;
REVOKE ALL ON FUNCTION app.scryfall_image_url(text,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Publication inserts printings without an image; give each its art crop.
CREATE FUNCTION app.fill_printing_image() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF NEW.image_ref IS NULL OR length(btrim(NEW.image_ref))=0 THEN
  NEW.image_ref:=app.scryfall_image_url(NEW.external_printing_ref,'art_crop');
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION app.fill_printing_image() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE TRIGGER printing_official_image BEFORE INSERT OR UPDATE OF image_ref ON app.printing
 FOR EACH ROW EXECUTE FUNCTION app.fill_printing_image();

UPDATE app.printing SET image_ref=app.scryfall_image_url(external_printing_ref,'art_crop')
 WHERE image_ref IS NULL OR length(btrim(image_ref))=0;

-- ---------------------------------------------------------------------------
-- 2. The stored face card
-- ---------------------------------------------------------------------------

ALTER TABLE app.deck ADD COLUMN face_definition_id uuid
 REFERENCES app.card_definition(id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;
CREATE INDEX ON app.deck(face_definition_id);

-- Existing decks start from the first card a reader would list: commander,
-- companion, then main, by name. Their add order was never recorded.
UPDATE app.deck d SET face_definition_id=(
 SELECT e.definition_id FROM app.deck_entry e JOIN app.card_definition c ON c.id=e.definition_id
 WHERE e.revision_id=d.current_revision_id AND e.quantity>0
 ORDER BY CASE e.section WHEN 'commander' THEN 0 WHEN 'companion' THEN 1 WHEN 'main' THEN 2 ELSE 3 END,
  c.name,e.definition_id LIMIT 1)
WHERE d.face_definition_id IS NULL AND d.current_revision_id IS NOT NULL;

-- A card's official art: the printing's art when it has a web address, else
-- the card's English nonfoil printing. Image references stay private columns;
-- this returns only public https links, never private storage references.
CREATE FUNCTION app.card_art_url(p_definition uuid,p_printing uuid) RETURNS text
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT coalesce(
  (SELECT p.image_ref FROM app.printing p WHERE p.id=p_printing AND p.definition_id=p_definition
   AND p.image_ref ~ '^https://[^[:space:]]+$'),
  (SELECT p.image_ref FROM app.printing p WHERE p.definition_id=p_definition
   AND p.image_ref ~ '^https://[^[:space:]]+$'
   ORDER BY (p.language='en') DESC,(p.finish='nonfoil') DESC,p.id LIMIT 1))
$$;
REVOKE ALL ON FUNCTION app.card_art_url(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.card_art_url(uuid,uuid) TO authenticated;

-- The face card a deck shows, with its art: the stored one while it is in the
-- deck's contents, otherwise the first card in reading order. Runs as the
-- caller, so only a deck the caller can read is ever consulted.
CREATE FUNCTION app.deck_face_card_json(p_revision uuid,p_face uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY INVOKER SET search_path='' AS $$
 WITH chosen AS (
  SELECT e.definition_id,e.printing_id FROM app.deck_entry e JOIN app.card_definition c ON c.id=e.definition_id
  WHERE e.revision_id=p_revision AND e.quantity>0
  ORDER BY (e.definition_id IS NOT DISTINCT FROM p_face) DESC,
   CASE e.section WHEN 'commander' THEN 0 WHEN 'companion' THEN 1 WHEN 'main' THEN 2 ELSE 3 END,
   c.name,e.definition_id,e.printing_id NULLS LAST
  LIMIT 1
 )
 SELECT jsonb_build_object('definitionId',chosen.definition_id,
  'artUrl',app.card_art_url(chosen.definition_id,chosen.printing_id))
 FROM chosen
$$;
REVOKE ALL ON FUNCTION app.deck_face_card_json(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.deck_face_card_json(uuid,uuid) TO authenticated;

-- New settings may carry an optional faceCardId. The observed baseline never
-- does: the face card is a preference, so it does not take part in the
-- compare-and-set of the other settings.
CREATE FUNCTION app.valid_deck_settings_change(p_settings jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_settings) IS DISTINCT FROM 'object' THEN RETURN false; END IF;
 IF p_settings ? 'faceCardId' AND (jsonb_typeof(p_settings->'faceCardId') IS DISTINCT FROM 'string'
 OR (p_settings->>'faceCardId')::uuid='00000000-0000-0000-0000-000000000000'::uuid) THEN RETURN false; END IF;
 RETURN app.valid_deck_settings(p_settings-'faceCardId');
EXCEPTION WHEN invalid_text_representation THEN RETURN false;
END $$;
REVOKE ALL ON FUNCTION app.valid_deck_settings_change(jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.change_deck_settings_command(p_subject uuid,p_session uuid,p_request uuid,p_deck uuid,p_expected jsonb,p_settings jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; saved app.deck; observed jsonb; appearance jsonb; face uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR p_deck IS NULL OR p_deck='00000000-0000-0000-0000-000000000000'::uuid
 OR NOT app.valid_deck_settings(p_expected) OR NOT app.valid_deck_settings_change(p_settings)
 OR octet_length(jsonb_build_object('expected',p_expected,'settings',p_settings)::text)>4096 THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 receipt:=app.begin_account_command(player.id,p_request,'deck_settings',p_deck::text,
  jsonb_build_object('expected',p_expected,'settings',p_settings));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF player.state<>'active' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT * INTO saved FROM app.deck WHERE id=p_deck AND account_id=player.id AND source_kind='account' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','deck unavailable')); END IF;
 observed:=jsonb_build_object('name',saved.name,'mode',saved.mode,'formatId',saved.format_id,
  'appearance',jsonb_build_object('backId',saved.back_id,'sleeveId',saved.sleeve_id,'diceId',saved.dice_id,'playmatId',saved.playmat_id));
 IF observed IS DISTINCT FROM p_expected THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('settings','deck settings changed; reload before saving')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.format WHERE id=(p_settings->>'formatId')::uuid) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','invalid deck metadata')); END IF;
 appearance:=p_settings->'appearance';
 IF NOT app.has_cosmetic(player.id,(appearance->>'backId')::uuid,'back')
 OR NOT app.has_cosmetic(player.id,(appearance->>'sleeveId')::uuid,'sleeve')
 OR NOT app.has_cosmetic(player.id,(appearance->>'diceId')::uuid,'dice')
 OR (appearance->>'playmatId' IS NOT NULL AND NOT app.has_cosmetic(player.id,(appearance->>'playmatId')::uuid,'playmat')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cosmetics','cosmetic selection unavailable')); END IF;
 face:=saved.face_definition_id;
 IF p_settings ? 'faceCardId' THEN
  face:=(p_settings->>'faceCardId')::uuid;
  IF NOT EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=saved.current_revision_id
  AND e.definition_id=face AND e.quantity>0) THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('faceCard','card not in deck')); END IF;
 END IF;
 UPDATE app.deck SET name=btrim(p_settings->>'name'),mode=p_settings->>'mode',format_id=(p_settings->>'formatId')::uuid,
  back_id=(appearance->>'backId')::uuid,sleeve_id=(appearance->>'sleeveId')::uuid,
  dice_id=(appearance->>'diceId')::uuid,playmat_id=(appearance->>'playmatId')::uuid,
  face_definition_id=face WHERE id=p_deck;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_deck));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

-- A saved deck always names a card in its contents: when its face card is
-- unset or has left the deck, the first card of this save takes its place.
CREATE OR REPLACE FUNCTION app.save_deck_command(p_subject uuid,p_session uuid,p_request uuid,p_deck uuid,p_expected_revision uuid,p_entries jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; result jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 result:=app.save_deck_contents(player.id,p_deck,p_expected_revision,p_entries,p_request);
 IF result->>'status'='success' AND jsonb_typeof(p_entries)='array' AND jsonb_array_length(p_entries)>0 THEN
  UPDATE app.deck d SET face_definition_id=(p_entries->0->>'definitionId')::uuid
  WHERE d.id=p_deck AND d.account_id=player.id AND d.source_kind='account'
  AND (d.face_definition_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.deck_entry e
   WHERE e.revision_id=d.current_revision_id AND e.definition_id=d.face_definition_id AND e.quantity>0))
  AND EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=d.current_revision_id
   AND e.definition_id=(p_entries->0->>'definitionId')::uuid AND e.quantity>0);
 END IF;
 RETURN result;
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

-- Disposition reads accept exactly what the settings writer accepts.
CREATE OR REPLACE FUNCTION app.read_deck_command(p_subject uuid,p_session uuid,p_kind text,p_command jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; receipt app.command_receipt; request_id uuid;
 scope_name text; target text; terms jsonb; format_id uuid; deck_id uuid; expected uuid;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF jsonb_typeof(p_command) IS DISTINCT FROM 'object' OR octet_length(p_command::text)>264192
 OR jsonb_typeof(p_command->'requestId') IS DISTINCT FROM 'string' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 request_id:=(p_command->>'requestId')::uuid;
 IF request_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 IF p_kind='create' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['formatId','mode','name','requestId']::text[]
  OR jsonb_typeof(p_command->'name') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'mode') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'formatId') IS DISTINCT FROM 'string' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  format_id:=(p_command->>'formatId')::uuid;
  scope_name:='deck_create'; target:=player.id::text;
  terms:=jsonb_build_object('name',p_command->>'name','mode',p_command->>'mode','format',format_id);
 ELSIF p_kind='save' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['deckId','entries','expectedRevisionId','requestId']::text[]
  OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_command->'entries') IS DISTINCT FROM 'array'
  OR jsonb_typeof(p_command->'expectedRevisionId') NOT IN ('string','null') THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  deck_id:=(p_command->>'deckId')::uuid; expected:=(p_command->>'expectedRevisionId')::uuid;
  scope_name:='deck_save'; target:=deck_id::text;
  terms:=jsonb_build_object('expectedRevision',expected,'entries',p_command->'entries');
 ELSIF p_kind='settings' THEN
  IF (SELECT array_agg(key ORDER BY key) FROM jsonb_object_keys(p_command) key)
    IS DISTINCT FROM ARRAY['deckId','expected','requestId','settings']::text[]
  OR jsonb_typeof(p_command->'deckId') IS DISTINCT FROM 'string'
  OR NOT app.valid_deck_settings(p_command->'expected') OR NOT app.valid_deck_settings_change(p_command->'settings')
  OR octet_length(p_command::text)>4096 THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  deck_id:=(p_command->>'deckId')::uuid;
  IF deck_id='00000000-0000-0000-0000-000000000000'::uuid THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
  scope_name:='deck_settings'; target:=deck_id::text;
  terms:=jsonb_build_object('expected',p_command->'expected','settings',p_command->'settings');
 ELSE
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope=scope_name AND request_ref=request_id::text;
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM target
  OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to(terms::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));
 END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,scope_name,request_id));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_text_representation THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
END $$;

-- ---------------------------------------------------------------------------
-- 3. Reads return the face card
-- ---------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION app.read_my_deck(p_deck uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT jsonb_build_object('deck',jsonb_build_object('id',d.id,'name',d.name,'mode',d.mode,'currentRevisionId',d.current_revision_id,
  'format',jsonb_build_object('id',f.id,'key',f.key,'name',f.name),
  'appearance',jsonb_build_object('backId',d.back_id,'sleeveId',d.sleeve_id,'diceId',d.dice_id,'playmatId',d.playmat_id),
  'faceCard',app.deck_face_card_json(r.id,d.face_definition_id)),
  'revision',CASE WHEN r.id IS NULL THEN NULL ELSE jsonb_build_object('id',r.id,'number',r.revision_number::text,
   'entries',(SELECT coalesce(jsonb_agg(jsonb_build_object('definitionId',e.definition_id,'printingId',e.printing_id,'section',e.section,'quantity',e.quantity::text)
    ORDER BY e.section,e.definition_id,e.printing_id NULLS FIRST),'[]') FROM app.deck_entry e WHERE e.revision_id=r.id)) END)
 INTO result FROM app.deck d JOIN app.format f ON f.id=d.format_id
 LEFT JOIN app.deck_revision r ON r.id=d.current_revision_id AND r.deck_id=d.id AND r.sealed_at IS NOT NULL
 WHERE d.id=p_deck AND d.account_id=actor AND d.source_kind='account';
 IF result IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('deck','unavailable')); END IF;
 RETURN jsonb_build_object('status','success','data',result);
END $$;

CREATE OR REPLACE FUNCTION app.read_my_deck_library(p_after uuid,p_limit integer) RETURNS jsonb
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
   'appearance',jsonb_build_object('backId',d.back_id,'sleeveId',d.sleeve_id,'diceId',d.dice_id,'playmatId',d.playmat_id),
   'faceCard',app.deck_face_card_json(r.id,d.face_definition_id)) entry
  FROM app.deck d JOIN app.format f ON f.id=d.format_id
  LEFT JOIN app.deck_revision r ON r.id=d.current_revision_id AND r.deck_id=d.id AND r.sealed_at IS NOT NULL
  WHERE d.account_id=actor AND d.source_kind='account' AND (p_after IS NULL OR d.id>p_after)
  ORDER BY d.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;

COMMIT;
