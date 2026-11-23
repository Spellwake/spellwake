BEGIN;

CREATE INDEX IF NOT EXISTS card_definition_revision_definition_id_id_idx
  ON app.card_definition_revision (definition_id, id DESC);

CREATE INDEX IF NOT EXISTS card_face_definition_revision_id_idx
  ON app.card_face (definition_revision_id, face_index);

-- Scope latest revision lookup per definition_id instead of scanning all revisions.
CREATE OR REPLACE FUNCTION app.card_printing_rich_json(p_printing_id uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  WITH latest_legality AS (
    SELECT id FROM app.legality_revision ORDER BY effective_at DESC, imported_at DESC LIMIT 1
  )
  SELECT jsonb_build_object(
    'printingId', p.id,
    'definitionId', d.id,
    'oracleRef', d.oracle_ref,
    'name', d.name,
    'externalPrintingRef', p.external_printing_ref,
    'rarity', p.rarity,
    'finish', p.finish,
    'imageUrl', app.printing_art_url(p.image_ref, p.external_printing_ref),
    'setId', s.id,
    'setName', s.name,
    'setCode', COALESCE(NULLIF(btrim(s.code), ''), s.external_set_ref),
    'rulesTextRef', rev.rules_text_ref,
    'faces', COALESCE((
      SELECT jsonb_agg(
        jsonb_build_object(
          'faceIndex', f.face_index,
          'name', f.name,
          'characteristicsRef', f.characteristics_ref
        ) ORDER BY f.face_index
      )
      FROM app.card_face f
      WHERE f.definition_revision_id = rev.id
    ), '[]'::jsonb),
    'legalities', COALESCE((
      SELECT jsonb_object_agg(fmt.key, cl.status)
      FROM app.card_legality cl
      JOIN app.format fmt ON fmt.id = cl.format_id
      JOIN latest_legality ll ON ll.id = cl.legality_revision_id
      WHERE cl.definition_id = d.id
    ), '{}'::jsonb)
  )
  FROM app.printing p
  JOIN app.card_definition d ON d.id = p.definition_id
  JOIN app.card_set s ON s.id = p.set_id
  LEFT JOIN LATERAL (
    SELECT r.id, r.rules_text_ref
    FROM app.card_definition_revision r
    WHERE r.definition_id = d.id
      AND app.catalogue_source_visible(r.catalogue_receipt_id)
    ORDER BY r.id DESC
    LIMIT 1
  ) rev ON TRUE
  WHERE p.id = p_printing_id
    AND app.catalogue_source_visible(p.catalogue_receipt_id)
    AND app.catalogue_source_visible(d.catalogue_receipt_id)
    AND app.catalogue_source_visible(s.catalogue_receipt_id)
$$;

COMMIT;
