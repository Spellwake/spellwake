BEGIN;

-- Source metadata is staged in its owning revision, never a second permanent
-- catalogue/cache. New definitions remain hidden until their source is accepted.
ALTER TABLE app.card_definition ADD COLUMN catalogue_receipt_id uuid REFERENCES app.command_receipt(id);
ALTER TABLE app.card_set ADD COLUMN catalogue_receipt_id uuid REFERENCES app.command_receipt(id);
ALTER TABLE app.printing ADD COLUMN catalogue_receipt_id uuid REFERENCES app.command_receipt(id);
ALTER TABLE app.printing ALTER COLUMN image_ref DROP NOT NULL;
ALTER TABLE app.card_definition_revision ADD COLUMN catalogue_receipt_id uuid REFERENCES app.command_receipt(id);
ALTER TABLE app.card_definition_revision ADD COLUMN source_metadata jsonb;
ALTER TABLE app.card_definition_revision ADD CONSTRAINT catalogue_revision_source CHECK (
 (catalogue_receipt_id IS NULL AND source_metadata IS NULL)
 OR (catalogue_receipt_id IS NOT NULL AND jsonb_typeof(source_metadata) IS NOT DISTINCT FROM 'object'));
CREATE INDEX catalogue_revision_publication ON app.card_definition_revision(catalogue_receipt_id) WHERE catalogue_receipt_id IS NOT NULL;

CREATE FUNCTION app.catalogue_source_visible(p_receipt uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT p_receipt IS NULL OR EXISTS(SELECT 1 FROM app.command_receipt
 WHERE id=p_receipt AND principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' AND state='accepted');
$$;
REVOKE EXECUTE ON FUNCTION app.catalogue_source_visible(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.catalogue_source_visible(uuid) TO authenticated;
DROP POLICY catalogue_card_read ON app.card_definition;
CREATE POLICY catalogue_card_read ON app.card_definition FOR SELECT TO authenticated USING(
 (SELECT app.current_account_id()) IS NOT NULL AND app.catalogue_source_visible(catalogue_receipt_id));

DROP POLICY catalogue_set_read ON app.card_set;
CREATE POLICY catalogue_set_read ON app.card_set FOR SELECT TO authenticated USING(
 (SELECT app.current_account_id()) IS NOT NULL AND app.catalogue_source_visible(catalogue_receipt_id));
DROP POLICY catalogue_printing_read ON app.printing;
CREATE POLICY catalogue_printing_read ON app.printing FOR SELECT TO authenticated USING(
 (SELECT app.current_account_id()) IS NOT NULL AND app.catalogue_source_visible(catalogue_receipt_id));

CREATE FUNCTION app.guard_catalogue_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE source_state text;
BEGIN
 IF OLD.catalogue_receipt_id IS NOT NULL THEN
  SELECT state INTO source_state FROM app.command_receipt WHERE id=OLD.catalogue_receipt_id;
  IF source_state IS DISTINCT FROM 'pending' OR TG_OP='DELETE'
  OR (NEW.id,NEW.definition_id,NEW.revision_ref,NEW.rules_text_ref,NEW.catalogue_receipt_id)
   IS DISTINCT FROM (OLD.id,OLD.definition_id,OLD.revision_ref,OLD.rules_text_ref,OLD.catalogue_receipt_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue revision source or accepted snapshot is immutable'; END IF;
 END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER catalogue_revision_history BEFORE UPDATE OR DELETE ON app.card_definition_revision
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_revision();

CREATE FUNCTION app.guard_catalogue_face() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE reference uuid; source_state text;
BEGIN
 reference:=CASE WHEN TG_OP='DELETE' THEN OLD.definition_revision_id ELSE NEW.definition_revision_id END;
 SELECT receipt.state INTO source_state FROM app.card_definition_revision revision
 JOIN app.command_receipt receipt ON receipt.id=revision.catalogue_receipt_id WHERE revision.id=reference;
 IF source_state IS NOT NULL AND source_state<>'pending' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted catalogue faces are immutable'; END IF;
 IF TG_OP='UPDATE' AND (NEW.id,NEW.definition_revision_id,NEW.face_index) IS DISTINCT FROM (OLD.id,OLD.definition_revision_id,OLD.face_index) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue face identity is immutable'; END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER catalogue_face_history BEFORE INSERT OR UPDATE OR DELETE ON app.card_face
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_face();

-- Pending metadata cannot become an owned asset, chosen avatar, qualified pack
-- candidate or executable release binding through another trusted writer.
CREATE FUNCTION app.guard_catalogue_printing_reference() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NEW.printing_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing p WHERE p.id=NEW.printing_id AND app.catalogue_source_visible(p.catalogue_receipt_id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published printing metadata required'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER catalogue_asset_source BEFORE INSERT OR UPDATE ON app.asset_definition
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_printing_reference();
CREATE TRIGGER catalogue_avatar_source BEFORE INSERT OR UPDATE ON app.profile
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_printing_reference();

CREATE FUNCTION app.guard_catalogue_program_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.card_definition_revision r WHERE r.id=NEW.definition_revision_id AND app.catalogue_source_visible(r.catalogue_receipt_id)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published definition revision required'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER catalogue_program_source BEFORE INSERT OR UPDATE ON app.release_card_program
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_program_source();

CREATE FUNCTION app.guard_catalogue_recipe_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF NEW.qualified_at IS NOT NULL AND (EXISTS(SELECT 1 FROM app.booster_product product JOIN app.card_set s ON s.id=product.set_id
  WHERE product.id=NEW.product_id AND NOT app.catalogue_source_visible(s.catalogue_receipt_id))
 OR EXISTS(SELECT 1 FROM app.booster_slot slot JOIN app.booster_slot_card candidate ON candidate.slot_id=slot.id
  JOIN app.printing p ON p.id=candidate.printing_id WHERE slot.recipe_id=NEW.id AND NOT app.catalogue_source_visible(p.catalogue_receipt_id))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='published catalogue required before recipe qualification'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER a_catalogue_recipe_source BEFORE INSERT OR UPDATE ON app.booster_recipe
 FOR EACH ROW EXECUTE FUNCTION app.guard_catalogue_recipe_source();

CREATE FUNCTION app.validate_catalogue_metadata(p_record jsonb) RETURNS void
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE key text; identity uuid; finishes jsonb;
BEGIN
 IF jsonb_typeof(p_record) IS DISTINCT FROM 'object'
 OR NOT p_record ?& ARRAY['externalPrintingRef','oracleRef','name','set','collectorNumber','language','rarity','finishes','layout','faceNames']
 OR p_record-ARRAY['externalPrintingRef','oracleRef','name','set','collectorNumber','language','rarity','finishes','layout','faceNames']<>'{}'
 OR jsonb_typeof(p_record->'set') IS DISTINCT FROM 'object'
 OR NOT (p_record->'set') ?& ARRAY['externalRef','name','code'] OR (p_record->'set')-ARRAY['externalRef','name','code']<>'{}' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid catalogue metadata shape'; END IF;
 FOREACH key IN ARRAY ARRAY['externalPrintingRef','oracleRef'] LOOP
  identity:=(p_record->>key)::uuid;
  IF identity IS NULL OR identity='00000000-0000-0000-0000-000000000000'::uuid OR identity::text<>p_record->>key THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='canonical catalogue identity required'; END IF;
 END LOOP;
 identity:=(p_record->'set'->>'externalRef')::uuid;
 IF identity IS NULL OR identity='00000000-0000-0000-0000-000000000000'::uuid OR identity::text<>p_record->'set'->>'externalRef'
 OR jsonb_typeof(p_record->'name') IS DISTINCT FROM 'string' OR length(p_record->>'name') NOT BETWEEN 1 AND 1024
 OR jsonb_typeof(p_record->'collectorNumber') IS DISTINCT FROM 'string' OR length(p_record->>'collectorNumber') NOT BETWEEN 1 AND 128
 OR jsonb_typeof(p_record->'language') IS DISTINCT FROM 'string' OR NOT coalesce(p_record->>'language' ~ '^[A-Za-z0-9_-]{1,16}$',false)
 OR jsonb_typeof(p_record->'rarity') IS DISTINCT FROM 'string' OR NOT coalesce(p_record->>'rarity' ~ '^[!-~]{1,64}$',false)
 OR jsonb_typeof(p_record->'layout') IS DISTINCT FROM 'string' OR length(p_record->>'layout') NOT BETWEEN 1 AND 64
 OR jsonb_typeof(p_record->'set'->'name') IS DISTINCT FROM 'string' OR length(p_record->'set'->>'name') NOT BETWEEN 1 AND 1024
 OR jsonb_typeof(p_record->'set'->'code') IS DISTINCT FROM 'string' OR NOT coalesce(p_record->'set'->>'code' ~ '^[A-Za-z0-9]{1,16}$',false)
 OR jsonb_typeof(p_record->'faceNames') IS DISTINCT FROM 'array' OR jsonb_array_length(p_record->'faceNames') NOT BETWEEN 1 AND 16
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_record->'faceNames') face WHERE jsonb_typeof(face)<>'string' OR length(face#>>'{}') NOT BETWEEN 1 AND 1024) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid catalogue labels or identity'; END IF;
 finishes:=p_record->'finishes';
 IF jsonb_typeof(finishes) IS DISTINCT FROM 'array' OR jsonb_array_length(finishes) NOT BETWEEN 1 AND 3
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(finishes) finish WHERE jsonb_typeof(finish)<>'string' OR finish#>>'{}' NOT IN ('nonfoil','foil','etched'))
 OR (SELECT count(DISTINCT finish) FROM jsonb_array_elements(finishes) finish)<>jsonb_array_length(finishes) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='distinct supported source finishes required'; END IF;
END $$;

CREATE FUNCTION app.begin_catalogue_publication(p_terms jsonb) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO,YMD' AS $$
DECLARE receipt app.command_receipt; fingerprint text; request_key text; key text; acquired timestamptz; updated timestamptz;
BEGIN
 IF jsonb_typeof(p_terms) IS DISTINCT FROM 'object' OR octet_length(p_terms::text)>2097152
 OR NOT p_terms ?& ARRAY['projection','artifactHash','metadataHash','stageHash','artifactRef','metadataRef','stageRef','sourceUpdatedAt','acquiredAt',
 'compressedBytes','decompressedBytes','recordCount','stageBytes','supportedRecords','unresolvedRecords','printingCount','definitionCount','setCount','batchCount','unresolvedReasons','batchHashes']
 OR p_terms-ARRAY['projection','artifactHash','metadataHash','stageHash','artifactRef','metadataRef','stageRef','sourceUpdatedAt','acquiredAt',
 'compressedBytes','decompressedBytes','recordCount','stageBytes','supportedRecords','unresolvedRecords','printingCount','definitionCount','setCount','batchCount','unresolvedReasons','batchHashes']<>'{}'
 OR p_terms->>'projection' IS DISTINCT FROM 'scryfall-catalogue-metadata-v1' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='complete catalogue source terms required'; END IF;
 FOREACH key IN ARRAY ARRAY['artifactHash','metadataHash','stageHash'] LOOP
  IF jsonb_typeof(p_terms->key) IS DISTINCT FROM 'string' OR NOT coalesce(p_terms->>key ~ '^sha256:[0-9a-f]{64}$',false) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='source and projection hashes required'; END IF;
 END LOOP;
 IF p_terms->>'artifactRef' IS DISTINCT FROM 'prices/scryfall/'||substring(p_terms->>'artifactHash' FROM 8)||'.jsonl.gz'
 OR p_terms->>'metadataRef' IS DISTINCT FROM 'prices/scryfall/metadata/'||substring(p_terms->>'metadataHash' FROM 8)||'.json'
 OR p_terms->>'stageRef' IS DISTINCT FROM 'catalogue/scryfall/'||substring(p_terms->>'stageHash' FROM 8)||'.jsonl' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='content-addressed catalogue evidence required'; END IF;
 FOREACH key IN ARRAY ARRAY['compressedBytes','decompressedBytes','recordCount','stageBytes','supportedRecords','unresolvedRecords','printingCount','definitionCount','setCount','batchCount'] LOOP
  IF jsonb_typeof(p_terms->key) IS DISTINCT FROM 'number' OR NOT coalesce(p_terms->>key ~ '^(0|[1-9][0-9]{0,10})$',false) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded integral catalogue statistics required'; END IF;
 END LOOP;
 acquired:=(p_terms->>'acquiredAt')::timestamptz; updated:=(p_terms->>'sourceUpdatedAt')::timestamptz;
 IF acquired IS NULL OR updated IS NULL OR acquired>clock_timestamp() OR updated>acquired OR acquired-updated>interval '24 hours'
 OR (p_terms->>'compressedBytes')::bigint NOT BETWEEN 1 AND 2147483648
 OR (p_terms->>'decompressedBytes')::bigint NOT BETWEEN 1 AND 17179869184
 OR (p_terms->>'stageBytes')::bigint NOT BETWEEN 1 AND 536870912
 OR (p_terms->>'recordCount')::bigint NOT BETWEEN 1 AND 5000000
 OR (p_terms->>'supportedRecords')::bigint NOT BETWEEN 1 AND 5000000
 OR (p_terms->>'recordCount')::bigint<>(p_terms->>'supportedRecords')::bigint+(p_terms->>'unresolvedRecords')::bigint
 OR (p_terms->>'printingCount')::bigint NOT BETWEEN (p_terms->>'supportedRecords')::bigint AND 3*(p_terms->>'supportedRecords')::bigint
 OR (p_terms->>'definitionCount')::bigint NOT BETWEEN 1 AND (p_terms->>'supportedRecords')::bigint
 OR (p_terms->>'setCount')::bigint NOT BETWEEN 1 AND (p_terms->>'supportedRecords')::bigint
 OR (p_terms->>'batchCount')::bigint NOT BETWEEN 1 AND least(20000,(p_terms->>'supportedRecords')::bigint)
 OR jsonb_typeof(p_terms->'batchHashes') IS DISTINCT FROM 'array' OR jsonb_array_length(p_terms->'batchHashes')<>(p_terms->>'batchCount')::integer
 OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_terms->'batchHashes') hash WHERE jsonb_typeof(hash)<>'string' OR NOT (hash#>>'{}' ~ '^sha256:[0-9a-f]{64}$'))
 OR jsonb_typeof(p_terms->'unresolvedReasons') IS DISTINCT FROM 'object' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='inconsistent catalogue source evidence'; END IF;
 IF EXISTS(SELECT 1 FROM jsonb_each(p_terms->'unresolvedReasons') reason WHERE reason.key NOT IN ('missing_root_oracle','invalid_metadata','invalid_finishes','unsupported_finish','duplicate_finish')
 OR jsonb_typeof(reason.value)<>'number' OR NOT (reason.value#>>'{}' ~ '^[1-9][0-9]{0,6}$'))
 OR (SELECT coalesce(sum((value#>>'{}')::bigint),0) FROM jsonb_each(p_terms->'unresolvedReasons'))<>(p_terms->>'unresolvedRecords')::bigint THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='explicit unresolved source coverage required'; END IF;
 fingerprint:=encode(sha256(convert_to(p_terms::text,'UTF8')),'hex');
 request_key:=(p_terms->>'artifactHash')||':'||(p_terms->>'projection');
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref)
 VALUES('cataloguer:scryfall-metadata-v1','catalogue_source',request_key,p_terms->>'stageHash',fingerprint,'pending',p_terms::text)
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF NOT FOUND THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' AND request_ref=request_key FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_terms->>'stageHash' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue source identity has different publication terms'; END IF;
 END IF;
 RETURN receipt.id;
END $$;

CREATE FUNCTION app.append_catalogue_batch(p_publication uuid,p_sequence integer,p_body text) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE publication app.command_receipt; prior app.command_receipt; fingerprint text; item jsonb; grouped record; definition uuid; revision uuid; common jsonb; batch uuid; p_records jsonb;
BEGIN
 SELECT * INTO STRICT publication FROM app.command_receipt WHERE id=p_publication AND principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' FOR UPDATE;
 IF p_body IS NULL OR octet_length(p_body)>524288 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded catalogue batch bytes required'; END IF;
 p_records:=p_body::jsonb;
 IF p_sequence IS NULL OR p_sequence NOT BETWEEN 1 AND (publication.result_ref::jsonb->>'batchCount')::integer
 OR jsonb_typeof(p_records) IS DISTINCT FROM 'array' OR jsonb_array_length(p_records) NOT BETWEEN 1 AND 500 OR octet_length(p_records::text)>524288 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded catalogue batch required'; END IF;
 IF publication.result_ref::jsonb->'batchHashes'->>(p_sequence-1) IS DISTINCT FROM 'sha256:'||encode(sha256(convert_to(p_body,'UTF8')),'hex') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue batch bytes differ from verified source plan'; END IF;
 fingerprint:=encode(sha256(convert_to(p_records::text,'UTF8')),'hex');
 SELECT * INTO prior FROM app.command_receipt WHERE principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_batch' AND request_ref=p_publication::text||':'||p_sequence::text;
 IF FOUND THEN
  IF prior.target_ref<>p_publication::text OR prior.payload_hash<>fingerprint OR prior.state<>'accepted' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue batch identity has different terms'; END IF;
  RETURN prior.id;
 END IF;
 IF publication.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue source is already terminal'; END IF;
 FOR item IN SELECT value FROM jsonb_array_elements(p_records) ORDER BY value->>'externalPrintingRef' LOOP
  PERFORM app.validate_catalogue_metadata(item);
  PERFORM pg_advisory_xact_lock(hashtextextended('spellwake:printing-source:'||(item->>'externalPrintingRef'),0));
 END LOOP;
 IF (SELECT count(DISTINCT value->>'externalPrintingRef') FROM jsonb_array_elements(p_records))<>jsonb_array_length(p_records) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='duplicate catalogue source printing'; END IF;
 FOR grouped IN SELECT value->>'oracleRef' oracle_ref,jsonb_agg(value ORDER BY value->>'externalPrintingRef') records,
  count(DISTINCT jsonb_build_object('name',value->'name','layout',value->'layout','faceNames',value->'faceNames')) variants
  FROM jsonb_array_elements(p_records) GROUP BY value->>'oracleRef' ORDER BY value->>'oracleRef' LOOP
  IF grouped.variants<>1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='inconsistent source definition metadata'; END IF;
  common:=jsonb_build_object('name',grouped.records->0->'name','layout',grouped.records->0->'layout','faceNames',grouped.records->0->'faceNames');
  INSERT INTO app.card_definition(oracle_ref,name,catalogue_receipt_id) VALUES(grouped.oracle_ref,common->>'name',p_publication) ON CONFLICT(oracle_ref) DO NOTHING;
  SELECT id INTO STRICT definition FROM app.card_definition WHERE oracle_ref=grouped.oracle_ref;
  SELECT id INTO revision FROM app.card_definition_revision WHERE definition_id=definition AND revision_ref='catalogue:'||p_publication::text FOR UPDATE;
  IF FOUND THEN
   IF EXISTS(SELECT 1 FROM app.card_definition_revision WHERE id=revision AND (source_metadata->'common'<>common OR catalogue_receipt_id<>p_publication))
   OR EXISTS(SELECT 1 FROM app.card_definition_revision r CROSS JOIN jsonb_array_elements(r.source_metadata->'records') old
    JOIN jsonb_array_elements(grouped.records) fresh ON old->>'externalPrintingRef'=fresh->>'externalPrintingRef' WHERE r.id=revision) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='duplicate printing or changed definition source'; END IF;
   UPDATE app.card_definition_revision SET source_metadata=jsonb_set(source_metadata,'{records}',(source_metadata->'records')||grouped.records) WHERE id=revision;
  ELSE
   INSERT INTO app.card_definition_revision(definition_id,revision_ref,rules_text_ref,catalogue_receipt_id,source_metadata)
   VALUES(definition,'catalogue:'||p_publication::text,(publication.result_ref::jsonb->>'artifactRef')||'#oracle='||grouped.oracle_ref,p_publication,
    jsonb_build_object('common',common,'records',grouped.records));
  END IF;
 END LOOP;
 -- Materialize only new identities behind the pending source. Existing visible
 -- labels stay unchanged until final release, including during a failed import.
 INSERT INTO app.card_set(external_set_ref,name,code,catalogue_receipt_id)
 SELECT DISTINCT value->'set'->>'externalRef',value->'set'->>'name',value->'set'->>'code',p_publication
 FROM jsonb_array_elements(p_records) ON CONFLICT(external_set_ref) DO NOTHING;
 INSERT INTO app.printing(definition_id,set_id,external_printing_ref,rarity,finish,image_ref,collector_number,language,catalogue_receipt_id)
 SELECT d.id,s.id,record->>'externalPrintingRef',record->>'rarity',finishes.value#>>'{}',NULL,
 record->>'collectorNumber',record->>'language',p_publication
 FROM jsonb_array_elements(p_records) record JOIN app.card_definition d ON d.oracle_ref=record->>'oracleRef'
 JOIN app.card_set s ON s.external_set_ref=record->'set'->>'externalRef' CROSS JOIN jsonb_array_elements(record->'finishes') finishes(value)
 ON CONFLICT(external_printing_ref,finish) DO NOTHING;
 IF EXISTS(SELECT 1 FROM jsonb_array_elements(p_records) record
 JOIN app.printing p ON p.external_printing_ref=record->>'externalPrintingRef'
 JOIN app.card_definition d ON d.id=p.definition_id JOIN app.card_set s ON s.id=p.set_id
 WHERE d.oracle_ref<>record->>'oracleRef' OR s.external_set_ref<>record->'set'->>'externalRef') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue source cannot reassign a printing identity'; END IF;
 INSERT INTO app.command_receipt(principal_key,scope,request_ref,target_ref,payload_hash,state,result_ref,response_payload)
 VALUES('cataloguer:scryfall-metadata-v1','catalogue_batch',p_publication::text||':'||p_sequence::text,p_publication::text,fingerprint,'accepted',
 jsonb_build_object('publicationId',p_publication,'sequence',p_sequence,'recordCount',jsonb_array_length(p_records))::text,
 jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_publication))) RETURNING id INTO batch;
 RETURN batch;
END $$;

CREATE FUNCTION app.finish_catalogue_publication(p_publication uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET statement_timeout='55s' AS $$
DECLARE publication app.command_receipt; terms jsonb; total bigint; printings bigint; definitions bigint; sets bigint; unpublished uuid[];
BEGIN
 -- Serialize final releases, including source snapshots touching shared identities.
 PERFORM pg_advisory_xact_lock(hashtextextended('spellwake:catalogue-publication',0));
 SELECT * INTO STRICT publication FROM app.command_receipt WHERE id=p_publication AND principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' FOR UPDATE;
 IF publication.state='accepted' THEN RETURN publication.id; END IF;
 IF publication.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue publication cannot finish'; END IF;
 terms:=publication.result_ref::jsonb;
 IF (SELECT count(*) FROM app.command_receipt WHERE principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_batch'
 AND target_ref=p_publication::text AND state='accepted')<>(terms->>'batchCount')::bigint THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue batches are incomplete'; END IF;
 -- Transaction-local materialization keeps final publication set-based. It is not
 -- an application table or independently authoritative staging store.
 CREATE TEMP TABLE spellwake_catalogue_release_records ON COMMIT DROP AS
 SELECT r.definition_id,item metadata FROM app.card_definition_revision r
 CROSS JOIN jsonb_array_elements(r.source_metadata->'records') item WHERE r.catalogue_receipt_id=p_publication;
 CREATE UNIQUE INDEX ON spellwake_catalogue_release_records((metadata->>'externalPrintingRef'));
 SELECT count(*),coalesce(sum(jsonb_array_length(metadata->'finishes')),0),count(DISTINCT definition_id),count(DISTINCT metadata->'set'->>'externalRef')
 INTO total,printings,definitions,sets FROM pg_temp.spellwake_catalogue_release_records;
 IF total<>(terms->>'supportedRecords')::bigint OR printings<>(terms->>'printingCount')::bigint
 OR definitions<>(terms->>'definitionCount')::bigint OR sets<>(terms->>'setCount')::bigint
 OR EXISTS(SELECT 1 FROM pg_temp.spellwake_catalogue_release_records GROUP BY metadata->'set'->>'externalRef'
 HAVING count(DISTINCT metadata->'set')<>1) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue projection coverage or set identity differs'; END IF;
 IF EXISTS(SELECT 1 FROM app.card_definition d JOIN app.command_receipt prior ON prior.id=d.catalogue_receipt_id
  JOIN app.card_definition_revision staged ON staged.definition_id=d.id AND staged.catalogue_receipt_id=p_publication
  WHERE prior.state='accepted' AND (prior.result_ref::jsonb->>'sourceUpdatedAt')::timestamptz >= (terms->>'sourceUpdatedAt')::timestamptz)
 OR EXISTS(SELECT 1 FROM app.card_set s JOIN app.command_receipt prior ON prior.id=s.catalogue_receipt_id
  JOIN pg_temp.spellwake_catalogue_release_records record ON record.metadata->'set'->>'externalRef'=s.external_set_ref
  WHERE prior.state='accepted' AND (prior.result_ref::jsonb->>'sourceUpdatedAt')::timestamptz >= (terms->>'sourceUpdatedAt')::timestamptz) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue source does not advance current metadata'; END IF;
 IF (SELECT count(*) FROM pg_temp.spellwake_catalogue_release_records record
 CROSS JOIN jsonb_array_elements(record.metadata->'finishes') finishes(value)
 JOIN app.printing p ON p.external_printing_ref=record.metadata->>'externalPrintingRef' AND p.finish=(finishes.value#>>'{}') AND p.definition_id=record.definition_id
 JOIN app.card_set s ON s.id=p.set_id AND s.external_set_ref=record.metadata->'set'->>'externalRef')<>printings THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='materialized catalogue identities are incomplete'; END IF;
 SELECT coalesce(array_agg(id),'{}') INTO unpublished FROM app.command_receipt
 WHERE principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' AND state='pending' AND id<>p_publication;
 UPDATE app.card_set s SET name=source.metadata->>'name',code=source.metadata->>'code',catalogue_receipt_id=p_publication
 FROM (SELECT DISTINCT metadata->'set' metadata FROM pg_temp.spellwake_catalogue_release_records) source
 WHERE s.external_set_ref=source.metadata->>'externalRef' AND
 ((s.name,s.code) IS DISTINCT FROM (source.metadata->>'name',source.metadata->>'code') OR s.catalogue_receipt_id IS NULL OR s.catalogue_receipt_id=ANY(unpublished));
 UPDATE app.printing p SET rarity=record.metadata->>'rarity',collector_number=record.metadata->>'collectorNumber',language=record.metadata->>'language',catalogue_receipt_id=p_publication
 FROM pg_temp.spellwake_catalogue_release_records record CROSS JOIN jsonb_array_elements(record.metadata->'finishes') finishes(value)
 WHERE p.external_printing_ref=record.metadata->>'externalPrintingRef' AND p.finish=(finishes.value#>>'{}') AND
 ((p.rarity,p.collector_number,p.language) IS DISTINCT FROM (record.metadata->>'rarity',record.metadata->>'collectorNumber',record.metadata->>'language')
 OR p.catalogue_receipt_id IS NULL OR p.catalogue_receipt_id=ANY(unpublished));
 UPDATE app.card_definition d SET name=r.source_metadata->'common'->>'name',catalogue_receipt_id=p_publication
 FROM app.card_definition_revision r WHERE r.definition_id=d.id AND r.catalogue_receipt_id=p_publication;
 INSERT INTO app.card_face(definition_revision_id,face_index,name,characteristics_ref)
 SELECT r.id,ordinality-1,face#>>'{}',r.rules_text_ref||'#face='||(ordinality-1)::text
 FROM app.card_definition_revision r CROSS JOIN jsonb_array_elements(r.source_metadata->'common'->'faceNames') WITH ORDINALITY faces(face,ordinality)
 WHERE r.catalogue_receipt_id=p_publication;
 UPDATE app.command_receipt SET state='accepted',response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_publication)) WHERE id=p_publication;
 DROP TABLE pg_temp.spellwake_catalogue_release_records;
 RETURN p_publication;
END $$;

-- Unpublished catalogue identities cannot enter a new owned deck revision.
CREATE OR REPLACE FUNCTION app.save_deck_contents(p_actor uuid,p_deck uuid,p_expected_revision uuid,p_entries jsonb,p_request uuid)
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
   OR NOT EXISTS(SELECT 1 FROM app.card_definition WHERE id=v_definition AND app.catalogue_source_visible(catalogue_receipt_id))
   OR (v_printing IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=v_printing AND definition_id=v_definition AND app.catalogue_source_visible(catalogue_receipt_id))) THEN
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

-- Only the canonical producer functions can write catalogue source rows. Old
-- definer-owned recipe/deck functions keep their existing interfaces and rights.
REVOKE INSERT,UPDATE,DELETE ON app.card_definition,app.card_definition_revision,app.card_set,app.printing,app.card_face FROM service_role;
REVOKE EXECUTE ON FUNCTION app.guard_catalogue_revision(),app.guard_catalogue_face(),app.guard_catalogue_printing_reference(),app.guard_catalogue_program_source(),app.guard_catalogue_recipe_source(),app.validate_catalogue_metadata(jsonb),app.begin_catalogue_publication(jsonb),app.append_catalogue_batch(uuid,integer,text),app.finish_catalogue_publication(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.begin_catalogue_publication(jsonb),app.append_catalogue_batch(uuid,integer,text),app.finish_catalogue_publication(uuid) TO service_role;

COMMIT;
