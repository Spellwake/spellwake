-- Re-enable deck faceCard fields in read_my_deck and read_my_deck_library now
-- that the deployed application API accepts optional DeckSummary.face_card (#36).
BEGIN;

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
