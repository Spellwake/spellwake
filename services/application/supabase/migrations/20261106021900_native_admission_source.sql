BEGIN;
-- Oracle identity is not a mutable display label. Preserve existing name/source
-- publication behavior and the sole existing printing identity owner.
CREATE FUNCTION app.guard_card_definition_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.oracle_ref) IS DISTINCT FROM (OLD.id,OLD.oracle_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='card definition identity is immutable';END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER card_definition_identity BEFORE UPDATE ON app.card_definition
 FOR EACH ROW EXECUTE FUNCTION app.guard_card_definition_identity();

-- Extend the existing owner: legacy identity refs must also stay fixed. Pending
-- catalogue imports may still append source_metadata before publication commits.
CREATE OR REPLACE FUNCTION app.guard_catalogue_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE source_state text;
BEGIN
 IF TG_OP='UPDATE' AND (NEW.id,NEW.definition_id,NEW.revision_ref,NEW.rules_text_ref)
  IS DISTINCT FROM (OLD.id,OLD.definition_id,OLD.revision_ref,OLD.rules_text_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='card definition revision identity is immutable';END IF;
 IF OLD.catalogue_receipt_id IS NOT NULL THEN
  SELECT state INTO source_state FROM app.command_receipt WHERE id=OLD.catalogue_receipt_id;
  IF source_state IS DISTINCT FROM 'pending' OR TG_OP='DELETE'
  OR (NEW.id,NEW.definition_id,NEW.revision_ref,NEW.rules_text_ref,NEW.catalogue_receipt_id)
   IS DISTINCT FROM (OLD.id,OLD.definition_id,OLD.revision_ref,OLD.rules_text_ref,OLD.catalogue_receipt_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='catalogue revision source or accepted snapshot is immutable';END IF;
 END IF;
 IF TG_OP='DELETE' THEN RETURN OLD;END IF;
 RETURN NEW;
END $$;

-- A consistent observation before a native opening exists, not admission,
-- participant authorization, a retained lease, or proof of executable readiness.
CREATE FUNCTION app.read_native_admission_source(p_game uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE g app.game; m app.match; profile app.table_profile_revision; release app.qualified_release;
 s app.match_seat; d app.between_game_decision; previous app.game; result app.game_result;
 chosen app.deck_revision; original app.deck_revision; entries jsonb; seats jsonb:='[]';
 selected uuid; count_seats bigint; count_games bigint; maximum numeric; count_entries bigint; total_quantity numeric;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_game IS NULL OR p_game='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN unavailable;END IF;
 SELECT * INTO g FROM app.game WHERE id=p_game;
 IF g.id IS NULL OR g.state<>'reserved' THEN RETURN unavailable;END IF;
 SELECT * INTO m FROM app.match WHERE id=g.match_id;
 IF m.id IS NULL OR m.room_id IS NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL
 OR m.admission_qualification_ref IS NULL OR length(btrim(m.admission_qualification_ref))=0
 OR m.state<>'ready' OR m.reserved_game_id IS DISTINCT FROM g.id
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt source WHERE source.principal_key='admission:'||m.id::text
  AND source.scope='match_decks_v1' AND source.target_ref=m.id::text AND source.state='accepted'
  AND source.response_payload->'data'->>'releaseId'=m.accepted_release_id::text
  AND source.response_payload->'data'->>'qualificationRef'=m.admission_qualification_ref)
 OR NOT EXISTS(SELECT 1 FROM app.game game0 JOIN app.command_receipt source
  ON source.scope='room_start_v1' AND source.principal_key='room-start:'||m.room_id::text
  AND source.target_ref=m.room_id::text AND source.state='accepted'
  AND source.response_payload->'data'->>'resourceId'=m.id::text
  AND source.response_payload->'data'->>'gameId'=game0.id::text
  WHERE game0.match_id=m.id AND game0.game_index=0) THEN RETURN unavailable;END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=m.profile_id;
 SELECT * INTO release FROM app.qualified_release WHERE id=m.accepted_release_id;
 SELECT count(*) INTO count_seats FROM app.match_seat WHERE match_id=m.id;
 IF profile.admission_qualification_ref IS NULL OR count_seats NOT BETWEEN 2 AND 6
 OR count_seats NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
 OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=m.id AND
  (deck_accepted_at IS NULL OR source_revision_id IS NULL OR deck_snapshot_ref IS NULL)) THEN RETURN unavailable;END IF;
 SELECT count(*),max(game_index) INTO count_games,maximum FROM app.game WHERE match_id=m.id;
 IF release.id IS NULL OR m.current_game_id IS NOT NULL OR g.accepted_revision<>0
 OR EXISTS(SELECT 1 FROM app.match_result WHERE match_id=m.id)
 OR count_games<>maximum+1 OR g.game_index<>maximum OR (m.structure='bo1' AND count_games<>1)
 OR EXISTS(SELECT 1 FROM app.game other WHERE other.match_id=m.id AND other.id<>g.id AND
  (other.state<>'completed' OR NOT EXISTS(SELECT 1 FROM app.game_result WHERE game_id=other.id)))
 OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending')
 OR EXISTS(SELECT 1 FROM app.game_result WHERE game_id=g.id)
 OR EXISTS(SELECT 1 FROM app.game_context WHERE game_id=g.id)
 OR EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=g.id)
 OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=g.id)
 OR EXISTS(SELECT 1 FROM app.operational_state WHERE game_id=g.id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved native admission frontier is inconsistent';END IF;
 IF g.game_index>0 THEN
  SELECT * INTO previous FROM app.game WHERE match_id=m.id AND game_index=g.game_index-1;
  SELECT * INTO result FROM app.game_result WHERE game_id=previous.id;
  IF m.structure<>'bo3' OR result.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.command_receipt source
   WHERE source.principal_key='match-lifecycle:'||m.id::text AND source.scope='game_outcome_v1'
   AND source.target_ref=result.id::text AND source.request_ref=result.id::text AND source.state='accepted'
   AND source.response_payload->'data'->>'phase'='between_games')
  OR (SELECT count(*) FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=previous.id AND state='answered')<>count_seats
  OR NOT EXISTS(SELECT 1 FROM app.between_game_decision decision JOIN app.command_receipt reply
   ON reply.scope='between_game_reply_v1' AND reply.principal_key='match-seat:'||decision.seat_id::text
   AND reply.target_ref=decision.id::text AND reply.state='accepted'
   AND reply.response_payload->'data'->>'reservedGameId'=g.id::text
   WHERE decision.match_id=m.id AND decision.after_game_id=previous.id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved native successor selection is inconsistent';END IF;
 END IF;
 FOR s IN SELECT * FROM app.match_seat WHERE match_id=m.id ORDER BY seat_index LOOP
  d:=NULL;
  IF g.game_index=0 THEN selected:=s.source_revision_id;
  ELSE
   SELECT * INTO d FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=previous.id AND seat_id=s.id AND state='answered';
   IF d.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved native seat selection is missing';END IF;
   PERFORM app.check_between_game_answer(d.id);
   selected:=d.selected_deck_revision_id;
  END IF;
  SELECT * INTO chosen FROM app.deck_revision WHERE id=selected;
  SELECT * INTO original FROM app.deck_revision WHERE id=s.source_revision_id;
  IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR original.sealed_at IS NULL
  OR EXISTS((SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=selected GROUP BY definition_id,printing_id
    EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id)
   UNION ALL (SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id
    EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=selected GROUP BY definition_id,printing_id)) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved native deck source is inconsistent';END IF;
  SELECT count(*),sum(e.quantity) INTO count_entries,total_quantity FROM app.deck_entry e WHERE e.revision_id=selected;
  IF count_entries NOT BETWEEN 1 AND 1000 OR total_quantity>1000 THEN RETURN unavailable;END IF;
  IF EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=selected AND
   ((SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id=p.definition_revision_id
     WHERE p.release_id=m.accepted_release_id AND b.definition_id=e.definition_id)<>1
    OR NOT EXISTS(SELECT 1 FROM app.card_definition WHERE id=e.definition_id)
    OR (e.printing_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=e.printing_id AND definition_id=e.definition_id)))) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reserved native card identity is inconsistent';END IF;
  SELECT jsonb_agg(jsonb_build_object('entryId',e.id,'definitionId',e.definition_id,'oracleRef',definition.oracle_ref,
   'definitionRevisionId',body.id,'definitionRevisionRef',body.revision_ref,'rulesTextRef',body.rules_text_ref,
   'programRef',p.program_ref,'unlimitedBasicLand',p.unlimited_basic_land,
   'printing',CASE WHEN printing.id IS NULL THEN NULL ELSE jsonb_build_object('id',printing.id,
    'externalPrintingRef',printing.external_printing_ref,'finish',printing.finish,'setId',printing.set_id) END,
   'section',e.section,'quantity',e.quantity::text) ORDER BY e.id) INTO entries
  FROM app.deck_entry e JOIN app.card_definition definition ON definition.id=e.definition_id
  JOIN app.card_definition_revision body ON body.definition_id=e.definition_id
  JOIN app.release_card_program p ON p.definition_revision_id=body.id AND p.release_id=m.accepted_release_id
  LEFT JOIN app.printing printing ON printing.id=e.printing_id WHERE e.revision_id=selected;
  seats:=seats||jsonb_build_array(jsonb_build_object('seatId',s.id,'seatIndex',s.seat_index::text,
   'accountId',s.account_id,'controllerKind',s.controller_kind,'sponsorId',s.sponsor_id,
   'originalRevisionId',s.source_revision_id,'originalSnapshotRef',s.deck_snapshot_ref,'selectedRevisionId',selected,
   'selectionDecisionId',d.id,'selectionReplyRef',d.reply_ref,'entries',entries));
 END LOOP;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('version','native-admission-source-v1',
  'gameId',g.id,'matchId',m.id,'roomId',m.room_id,'gameIndex',g.game_index::text,'matchRevision',m.revision::text,
  'mode',m.mode,'formatId',m.format_id,'legalityRevisionId',m.legality_revision_id,'competitive',m.competitive,
  'structure',m.structure,'admissionQualificationRef',m.admission_qualification_ref,
  'profile',jsonb_build_object('id',profile.id,'key',profile.key,'revisionRef',profile.revision_ref,
   'rulesRef',profile.rules_ref,'planarPoolRef',profile.planar_pool_ref,'minimumSeats',profile.minimum_seats::text,
   'seatCapacity',profile.seat_capacity::text,'qualificationRef',profile.admission_qualification_ref),
  'release',jsonb_build_object('id',release.id,'releaseRef',release.release_ref,'kernelRef',release.kernel_ref,
   'rulesRef',release.rules_ref,'programManifestRef',release.program_manifest_ref,'evidenceRef',release.evidence_ref),
  'seats',seats));
END $$;
REVOKE ALL ON FUNCTION app.guard_card_definition_identity(),app.read_native_admission_source(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_native_admission_source(uuid) TO service_role;
COMMIT;
