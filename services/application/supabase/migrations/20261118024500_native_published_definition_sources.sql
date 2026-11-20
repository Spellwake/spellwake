BEGIN;

-- One maintained card/printing projection, reused by room, between-game and
-- reserved admission sources. Its callers retain their distinct admission guards.
CREATE OR REPLACE FUNCTION app.native_deck_source(p_release uuid,p_revision uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE entries jsonb; projected bigint; unique_entries bigint; selected bigint;
BEGIN
 SELECT count(*) INTO selected FROM app.deck_entry WHERE revision_id=p_revision;
 SELECT count(*),count(DISTINCT e.id),jsonb_agg(jsonb_build_object('entryId',e.id,'definitionId',e.definition_id,'oracleRef',definition.oracle_ref,
  'definitionRevisionId',body.id,'definitionRevisionRef',body.revision_ref,'rulesTextRef',body.rules_text_ref,
  'programRef',p.program_ref,'unlimitedBasicLand',p.unlimited_basic_land,
  'printing',CASE WHEN printing.id IS NULL THEN NULL ELSE jsonb_build_object('id',printing.id,
   'externalPrintingRef',printing.external_printing_ref,'finish',printing.finish,'setId',printing.set_id) END,
  'section',e.section,'quantity',e.quantity::text) ORDER BY e.id) INTO projected,unique_entries,entries
 FROM app.deck_entry e JOIN app.card_definition definition ON definition.id=e.definition_id
 JOIN app.card_definition_revision body ON body.definition_id=e.definition_id
 JOIN app.release_card_program p ON p.definition_revision_id=body.id AND p.release_id=p_release
 LEFT JOIN app.command_receipt publication ON publication.id=body.catalogue_receipt_id
 LEFT JOIN app.printing printing ON printing.id=e.printing_id WHERE e.revision_id=p_revision AND CASE
  WHEN body.catalogue_receipt_id IS NULL THEN body.revision_ref NOT LIKE 'catalogue:%'
  ELSE publication.principal_key='cataloguer:scryfall-metadata-v1' AND publication.scope='catalogue_source'
   AND publication.state='accepted' AND body.revision_ref='catalogue:'||publication.id::text
   AND body.rules_text_ref=(publication.result_ref::jsonb->>'artifactRef')||'#oracle='||definition.oracle_ref
  END;
 IF projected<>selected OR unique_entries<>selected THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native deck publication source is incomplete';
 END IF;
 RETURN entries;
END $$;

COMMIT;
