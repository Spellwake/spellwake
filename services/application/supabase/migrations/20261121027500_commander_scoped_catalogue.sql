BEGIN;

-- This approval publishes an exact subset from a retained archive without
-- occupying or rewriting the canonical full-catalogue import identity.
CREATE FUNCTION app.begin_commander_pilot_catalogue(p_terms jsonb) RETURNS uuid
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
 IF p_terms->>'recordCount' <> '4' OR p_terms->>'definitionCount' <> '4'
    OR p_terms->>'printingCount' <> '4' OR p_terms->>'batchCount' <> '1' OR p_terms->>'unresolvedRecords' <> '0'
    OR p_terms->>'artifactHash' IS DISTINCT FROM 'sha256:212faafc74b0c9305381f00c91021d089e9d17a602ea86f14890e4610ad4062c'
    OR p_terms->'batchHashes' IS DISTINCT FROM '["sha256:bcb38ef90a6b1484225f096ea143e23c9cb99387d597c7a87fd5683cf80fe6bc"]'::jsonb
    OR p_terms->>'stageHash' IS DISTINCT FROM 'sha256:0a48b1be271b981739d4a0858ab61c54fce785d3868591acaab20801a0a79feb'
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='Four Commander approved catalogue records required'; END IF;
 request_key:=(p_terms->>'artifactHash')||':'||(p_terms->>'projection')||':commander-pilot:'||(p_terms->>'stageHash');
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

REVOKE ALL ON FUNCTION app.begin_commander_pilot_catalogue(jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.begin_commander_pilot_catalogue(jsonb) TO service_role;

CREATE OR REPLACE FUNCTION app.finish_catalogue_publication(p_publication uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET statement_timeout='55s' AS $$
DECLARE publication app.command_receipt; terms jsonb; total bigint; printings bigint; definitions bigint; sets bigint; unpublished uuid[]; commander_subset boolean;
BEGIN
 -- Serialize final releases, including source snapshots touching shared identities.
 PERFORM pg_advisory_xact_lock(hashtextextended('spellwake:catalogue-publication',0));
 SELECT * INTO STRICT publication FROM app.command_receipt WHERE id=p_publication AND principal_key='cataloguer:scryfall-metadata-v1' AND scope='catalogue_source' FOR UPDATE;
 IF publication.state='accepted' THEN RETURN publication.id; END IF;
 IF publication.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue publication cannot finish'; END IF;
 terms:=publication.result_ref::jsonb;
 commander_subset:=publication.request_ref='sha256:212faafc74b0c9305381f00c91021d089e9d17a602ea86f14890e4610ad4062c:scryfall-catalogue-metadata-v1:commander-pilot:sha256:0a48b1be271b981739d4a0858ab61c54fce785d3868591acaab20801a0a79feb';
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
 IF commander_subset THEN
  -- This exact subset can reuse an accepted set only when all stored set fields agree.
  -- Accepted definitions and printing metadata keep their original source ownership.
  IF EXISTS(SELECT 1 FROM app.card_definition d JOIN app.command_receipt prior ON prior.id=d.catalogue_receipt_id
   JOIN app.card_definition_revision staged ON staged.definition_id=d.id AND staged.catalogue_receipt_id=p_publication
   WHERE prior.state='accepted')
  OR EXISTS(SELECT 1 FROM app.card_set s JOIN app.command_receipt prior ON prior.id=s.catalogue_receipt_id
   JOIN pg_temp.spellwake_catalogue_release_records record ON record.metadata->'set'->>'externalRef'=s.external_set_ref
   WHERE prior.state='accepted' AND (s.name,s.code) IS DISTINCT FROM
    (record.metadata->'set'->>'name',record.metadata->'set'->>'code'))
  OR EXISTS(SELECT 1 FROM pg_temp.spellwake_catalogue_release_records record
   JOIN app.printing p ON p.external_printing_ref=record.metadata->>'externalPrintingRef'
   JOIN app.command_receipt prior ON prior.id=p.catalogue_receipt_id
   WHERE prior.state='accepted' AND (p.rarity,p.collector_number,p.language) IS DISTINCT FROM
    (record.metadata->>'rarity',record.metadata->>'collectorNumber',record.metadata->>'language')) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Commander catalogue subset conflicts with accepted metadata'; END IF;
 ELSIF EXISTS(SELECT 1 FROM app.card_definition d JOIN app.command_receipt prior ON prior.id=d.catalogue_receipt_id
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

COMMIT;
