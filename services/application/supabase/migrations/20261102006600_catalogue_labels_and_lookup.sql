BEGIN;

-- Sourced display/lookup metadata. Existing rows remain explicitly unknown;
-- absence is not filled with guessed language, collector number or set code.
ALTER TABLE app.card_set ADD COLUMN code text CHECK(code IS NULL OR (length(code) BETWEEN 1 AND 16 AND code=btrim(code)));
CREATE UNIQUE INDEX card_set_code ON app.card_set(lower(code)) WHERE code IS NOT NULL;
ALTER TABLE app.printing ADD COLUMN collector_number text CHECK(collector_number IS NULL OR length(collector_number) BETWEEN 1 AND 128);
ALTER TABLE app.printing ADD COLUMN language text CHECK(language IS NULL OR length(language) BETWEEN 1 AND 16);
CREATE INDEX printing_definition_page ON app.printing(definition_id,id);

GRANT SELECT(id,name,oracle_ref) ON app.card_definition TO authenticated;
GRANT SELECT(id,name,code) ON app.card_set TO authenticated;
GRANT SELECT(id,definition_id,set_id,external_printing_ref,rarity,finish,collector_number,language) ON app.printing TO authenticated;
CREATE POLICY catalogue_card_read ON app.card_definition FOR SELECT TO authenticated USING((SELECT app.current_account_id()) IS NOT NULL);
CREATE POLICY catalogue_set_read ON app.card_set FOR SELECT TO authenticated USING((SELECT app.current_account_id()) IS NOT NULL);
CREATE POLICY catalogue_printing_read ON app.printing FOR SELECT TO authenticated USING((SELECT app.current_account_id()) IS NOT NULL);

CREATE FUNCTION app.read_my_cards(p_query text,p_exact boolean,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF p_query IS NULL OR length(p_query)>1024 OR p_exact IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(to_jsonb(page) ORDER BY page.id),'[]') INTO result FROM (
  SELECT d.id,d.name,d.oracle_ref AS "oracleRef" FROM app.card_definition d
  WHERE (CASE WHEN p_exact THEN lower(d.name)=lower(btrim(p_query)) ELSE starts_with(lower(d.name),lower(btrim(p_query))) END)
  AND (p_after IS NULL OR d.id>p_after) ORDER BY d.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;

CREATE FUNCTION app.read_my_printings(p_definition uuid,p_name text,p_set_code text,p_collector text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF (p_definition IS NULL)=(p_name IS NULL) OR p_definition='00000000-0000-0000-0000-000000000000'::uuid
 OR (p_name IS NOT NULL AND length(p_name) NOT BETWEEN 1 AND 1024)
 OR (p_set_code IS NOT NULL AND length(p_set_code) NOT BETWEEN 1 AND 16)
 OR (p_collector IS NOT NULL AND length(p_collector) NOT BETWEEN 1 AND 128)
 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT p.id,jsonb_build_object('id',p.id,'definitionId',p.definition_id,'externalPrintingRef',p.external_printing_ref,
   'rarity',p.rarity,'finish',p.finish,'collectorNumber',p.collector_number,'language',p.language,
   'set',jsonb_build_object('id',s.id,'name',s.name,'code',s.code)) entry
  FROM app.printing p JOIN app.card_set s ON s.id=p.set_id JOIN app.card_definition d ON d.id=p.definition_id
  WHERE (p_definition IS NULL OR p.definition_id=p_definition) AND (p_name IS NULL OR lower(d.name)=lower(btrim(p_name)))
  AND (p_set_code IS NULL OR lower(s.code)=lower(p_set_code)) AND (p_collector IS NULL OR p.collector_number=p_collector)
  AND (p_after IS NULL OR p.id>p_after) ORDER BY p.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;

-- Batch metadata reads keep a thousand-entry deck below the existing per-response
-- bound: callers resolve at most100 definitions and100 printings per request.
CREATE FUNCTION app.read_my_catalogue(p_definitions uuid[],p_printings uuid[]) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; cards jsonb; printings jsonb;
BEGIN
 IF p_definitions IS NULL OR p_printings IS NULL OR cardinality(p_definitions)>100 OR cardinality(p_printings)>100
 OR array_position(p_definitions,NULL) IS NOT NULL OR array_position(p_printings,NULL) IS NOT NULL
 OR '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_definitions)
 OR '00000000-0000-0000-0000-000000000000'::uuid=ANY(p_printings)
 OR cardinality(p_definitions)<>(SELECT count(DISTINCT id) FROM unnest(p_definitions) id)
 OR cardinality(p_printings)<>(SELECT count(DISTINCT id) FROM unnest(p_printings) id) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(to_jsonb(card) ORDER BY card.id),'[]') INTO cards FROM (
  SELECT d.id,d.name,d.oracle_ref AS "oracleRef" FROM app.card_definition d WHERE d.id=ANY(p_definitions)
 ) card;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',p.id,'definitionId',p.definition_id,'externalPrintingRef',p.external_printing_ref,
  'rarity',p.rarity,'finish',p.finish,'collectorNumber',p.collector_number,'language',p.language,
  'set',jsonb_build_object('id',s.id,'name',s.name,'code',s.code)) ORDER BY p.id),'[]') INTO printings
 FROM app.printing p JOIN app.card_set s ON s.id=p.set_id WHERE p.id=ANY(p_printings);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('cards',cards,'printings',printings));
END $$;
REVOKE EXECUTE ON FUNCTION app.read_my_cards(text,boolean,uuid,integer),app.read_my_printings(uuid,text,text,text,uuid,integer),app.read_my_catalogue(uuid[],uuid[])
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_cards(text,boolean,uuid,integer),app.read_my_printings(uuid,text,text,text,uuid,integer),app.read_my_catalogue(uuid[],uuid[]) TO authenticated;

COMMIT;
