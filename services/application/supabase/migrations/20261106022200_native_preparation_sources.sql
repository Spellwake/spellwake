BEGIN;

-- One maintained card/printing projection, reused by room, between-game and
-- reserved admission sources. Its callers retain their distinct admission guards.
CREATE FUNCTION app.native_deck_source(p_release uuid,p_revision uuid) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_agg(jsonb_build_object('entryId',e.id,'definitionId',e.definition_id,'oracleRef',definition.oracle_ref,
  'definitionRevisionId',body.id,'definitionRevisionRef',body.revision_ref,'rulesTextRef',body.rules_text_ref,
  'programRef',p.program_ref,'unlimitedBasicLand',p.unlimited_basic_land,
  'printing',CASE WHEN printing.id IS NULL THEN NULL ELSE jsonb_build_object('id',printing.id,
   'externalPrintingRef',printing.external_printing_ref,'finish',printing.finish,'setId',printing.set_id) END,
  'section',e.section,'quantity',e.quantity::text) ORDER BY e.id)
 FROM app.deck_entry e JOIN app.card_definition definition ON definition.id=e.definition_id
 JOIN app.card_definition_revision body ON body.definition_id=e.definition_id
 JOIN app.release_card_program p ON p.definition_revision_id=body.id AND p.release_id=p_release
 LEFT JOIN app.printing printing ON printing.id=e.printing_id WHERE e.revision_id=p_revision
$$;

CREATE FUNCTION app.read_native_room_source(p_room uuid,p_basis numeric,p_release uuid,p_legality uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room;profile app.table_profile_revision;release app.qualified_release;
 slot app.room_slot;selected app.deck_revision;library app.deck;entries jsonb;
 slots jsonb:='[]';terms jsonb;source_status text;entry_count bigint;total_quantity numeric;
 unavailable constant jsonb:='{"status":"fail","data":{"source":"unavailable"}}'::jsonb;
BEGIN
 IF p_room IS NULL OR p_release IS NULL OR p_legality IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_release,p_legality)
  OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN unavailable;END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room AND revision=p_basis AND state IN('lobby','countdown');
 IF lobby.id IS NULL THEN RETURN unavailable;END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 SELECT * INTO release FROM app.qualified_release WHERE id=p_release;
 IF profile.id IS NULL OR profile.admission_qualification_ref IS NULL OR release.id IS NULL
  OR NOT EXISTS(SELECT 1 FROM app.legality_revision WHERE id=p_legality)
  OR(SELECT count(*) FROM app.room_slot WHERE room_id=lobby.id AND retired_at IS NULL
   AND(account_id IS NOT NULL OR sponsor_id IS NOT NULL)) NOT BETWEEN 1 AND 6 THEN RETURN unavailable;END IF;
 FOR slot IN SELECT * FROM app.room_slot WHERE room_id=lobby.id AND retired_at IS NULL
  AND(account_id IS NOT NULL OR sponsor_id IS NOT NULL) ORDER BY seat_index LOOP
  entries:=NULL;source_status:='unselected';
  IF slot.deck_revision_id IS NOT NULL THEN
   source_status:='unsupported';
   SELECT * INTO selected FROM app.deck_revision WHERE id=slot.deck_revision_id AND sealed_at IS NOT NULL;
   SELECT * INTO library FROM app.deck WHERE id=selected.deck_id;
   IF selected.id IS NOT NULL AND library.mode=lobby.mode AND library.format_id=lobby.format_id
    AND((slot.controller_kind='human' AND library.source_kind='account' AND library.account_id=slot.account_id)
     OR(slot.controller_kind='ai' AND(library.source_kind='system_pool' OR library.account_id=slot.sponsor_id))) THEN
    SELECT count(*),sum(quantity) INTO entry_count,total_quantity FROM app.deck_entry WHERE revision_id=selected.id;
    IF EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=selected.id AND(
     (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id=p.definition_revision_id WHERE p.release_id=p_release AND b.definition_id=e.definition_id)>1
     OR(e.printing_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=e.printing_id AND definition_id=e.definition_id)))) THEN
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native room card identity is inconsistent';END IF;
    IF entry_count BETWEEN 1 AND 1000 AND total_quantity<=1000
     AND NOT EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=selected.id AND NOT EXISTS(
      SELECT 1 FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id=p.definition_revision_id
       WHERE p.release_id=p_release AND b.definition_id=e.definition_id)) THEN
     entries:=app.native_deck_source(p_release,selected.id);
     IF jsonb_array_length(entries) IS DISTINCT FROM entry_count THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native room entry projection is incomplete';END IF;
     source_status:='available';
    END IF;
   END IF;
  END IF;
  slots:=slots||jsonb_build_array(jsonb_build_object('slotId',slot.id,'seatIndex',slot.seat_index::text,
   'accountId',slot.account_id,'sponsorId',slot.sponsor_id,'controllerKind',slot.controller_kind,
   'teamRef',slot.team_ref,'ready',slot.ready,'sourceStatus',source_status,
   'selectedRevisionId',slot.deck_revision_id,'entries',entries));
 END LOOP;
 terms:=jsonb_build_object('version','native-room-source-v1','room',jsonb_build_object('id',lobby.id,'revision',lobby.revision::text,
  'hostId',lobby.host_id,'state',lobby.state,'configurationId',lobby.configuration_id,'profileId',lobby.profile_id,
  'formatId',lobby.format_id,'mode',lobby.mode,'competitive',lobby.competitive,'structure',lobby.structure,
  'countdownAt',to_char(lobby.countdown_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')),
  'profile',jsonb_build_object('id',profile.id,'key',profile.key,'revisionRef',profile.revision_ref,'rulesRef',profile.rules_ref,
   'planarPoolRef',profile.planar_pool_ref,'minimumSeats',profile.minimum_seats::text,'seatCapacity',profile.seat_capacity::text,'qualificationRef',profile.admission_qualification_ref),
  'release',jsonb_build_object('id',release.id,'releaseRef',release.release_ref,'kernelRef',release.kernel_ref,
   'rulesRef',release.rules_ref,'programManifestRef',release.program_manifest_ref,'evidenceRef',release.evidence_ref),
  'legalityRevisionId',p_legality,'slots',slots);
 IF octet_length(terms::text)>2097152 THEN RETURN unavailable;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('sourceHash',app.auth_json_hash(terms),'terms',terms));
END $$;

CREATE FUNCTION app.read_native_between_source(p_decision uuid,p_basis numeric,p_selected_revision uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE decision app.between_game_decision;m app.match;seat app.match_seat;selected app.deck_revision;
 original app.deck_revision;library app.deck;profile app.table_profile_revision;release app.qualified_release;
 entries jsonb;terms jsonb;entry_count bigint;total_quantity numeric;
 unavailable constant jsonb:='{"status":"fail","data":{"source":"unavailable"}}'::jsonb;
BEGIN
 IF p_decision IS NULL OR p_selected_revision IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_decision,p_selected_revision)
  OR p_basis IS NULL OR p_basis::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN unavailable;END IF;
 SELECT * INTO decision FROM app.between_game_decision WHERE id=p_decision AND state='pending';
 SELECT * INTO m FROM app.match WHERE id=decision.match_id;
 IF decision.id IS NULL OR m.id IS NULL OR m.revision<>p_basis OR m.state<>'between_games' OR m.availability<>'available'
  OR m.current_game_id IS NOT NULL OR m.started_at IS NULL OR m.accepted_release_id IS NULL
  OR decision.after_game_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=decision.after_game_id AND match_id=m.id AND state IN('completed','no_contest'))
  OR EXISTS(SELECT 1 FROM app.game later JOIN app.game prior ON prior.id=decision.after_game_id WHERE later.match_id=m.id AND later.game_index>prior.game_index)
  OR NOT EXISTS(SELECT 1 FROM app.command_receipt a WHERE a.scope='match_decks_v1' AND a.principal_key='admission:'||m.id::text
   AND a.target_ref=m.id::text AND a.state='accepted' AND a.response_payload->'data'->>'releaseId'=m.accepted_release_id::text
   AND a.response_payload->'data'->>'qualificationRef'=m.admission_qualification_ref) THEN RETURN unavailable;END IF;
 SELECT * INTO seat FROM app.match_seat WHERE id=decision.seat_id AND match_id=m.id;
 SELECT * INTO original FROM app.deck_revision WHERE id=seat.source_revision_id AND sealed_at IS NOT NULL;
 SELECT * INTO selected FROM app.deck_revision WHERE id=p_selected_revision AND sealed_at IS NOT NULL;
 SELECT * INTO library FROM app.deck WHERE id=selected.deck_id;
 IF seat.id IS NULL OR original.id IS NULL OR selected.id IS NULL OR seat.deck_accepted_at IS NULL OR seat.deck_snapshot_ref IS NULL
  OR NOT EXISTS(SELECT 1 FROM app.match_clock WHERE match_seat_id=seat.id AND clock_revision=decision.clock_revision)
  OR NOT((seat.controller_kind='human' AND library.source_kind='account' AND library.account_id=seat.account_id)
   OR(seat.controller_kind='ai' AND(library.source_kind='system_pool' OR library.account_id=seat.sponsor_id))) THEN RETURN unavailable;END IF;
 IF EXISTS((SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=selected.id GROUP BY definition_id,printing_id
   EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id)
  UNION ALL(SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id
   EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=selected.id GROUP BY definition_id,printing_id)) THEN RETURN unavailable;END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=m.profile_id;
 SELECT * INTO release FROM app.qualified_release WHERE id=m.accepted_release_id;
 IF profile.id IS NULL OR profile.admission_qualification_ref IS NULL OR release.id IS NULL THEN RETURN unavailable;END IF;
 SELECT count(*),sum(quantity) INTO entry_count,total_quantity FROM app.deck_entry WHERE revision_id=selected.id;
 IF entry_count NOT BETWEEN 1 AND 1000 OR total_quantity>1000 THEN RETURN unavailable;END IF;
 IF EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=selected.id AND(
  (SELECT count(*) FROM app.release_card_program p JOIN app.card_definition_revision b ON b.id=p.definition_revision_id
   WHERE p.release_id=release.id AND b.definition_id=e.definition_id)<>1
  OR(e.printing_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM app.printing WHERE id=e.printing_id AND definition_id=e.definition_id)))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native between-game card identity is inconsistent';END IF;
 entries:=app.native_deck_source(release.id,selected.id);
 IF jsonb_array_length(entries) IS DISTINCT FROM entry_count THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native between-game entry projection is incomplete';END IF;
 terms:=jsonb_build_object('version','native-between-source-v1','decision',jsonb_build_object('id',decision.id,'matchId',m.id,
  'seatId',seat.id,'afterGameId',decision.after_game_id,'basisRevision',decision.basis_revision::text,'clockRevision',decision.clock_revision::text,
  'deadlineRevision',decision.deadline_revision::text,'deadlineAt',to_char(decision.deadline_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'state',decision.state),
  'match',jsonb_build_object('id',m.id,'revision',m.revision::text,'mode',m.mode,'formatId',m.format_id,'profileId',m.profile_id,
   'legalityRevisionId',m.legality_revision_id,'releaseId',m.accepted_release_id,'availabilityRevision',m.availability_revision::text,
   'availability',m.availability,'structure',m.structure,'startedAt',to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')),
  'profile',jsonb_build_object('id',profile.id,'key',profile.key,'revisionRef',profile.revision_ref,'rulesRef',profile.rules_ref,
   'planarPoolRef',profile.planar_pool_ref,'minimumSeats',profile.minimum_seats::text,'seatCapacity',profile.seat_capacity::text,'qualificationRef',profile.admission_qualification_ref),
  'release',jsonb_build_object('id',release.id,'releaseRef',release.release_ref,'kernelRef',release.kernel_ref,'rulesRef',release.rules_ref,
   'programManifestRef',release.program_manifest_ref,'evidenceRef',release.evidence_ref),
  'originalRevisionId',seat.source_revision_id,'originalSnapshotRef',seat.deck_snapshot_ref,'selectedRevisionId',selected.id,'entries',entries);
 IF octet_length(terms::text)>2097152 THEN RETURN unavailable;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('sourceHash',app.auth_json_hash(terms),'terms',terms));
END $$;

-- Preserve all219 reservation/history/source guards and its exact public shape.
CREATE OR REPLACE FUNCTION app.read_native_admission_source(p_game uuid) RETURNS jsonb
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
  entries:=app.native_deck_source(m.accepted_release_id,selected);
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
REVOKE ALL ON FUNCTION app.native_deck_source(uuid,uuid),app.read_native_room_source(uuid,numeric,uuid,uuid),app.read_native_between_source(uuid,numeric,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
