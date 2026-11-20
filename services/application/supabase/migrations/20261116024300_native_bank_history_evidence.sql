BEGIN;
-- Add authenticated bank-input evidence to the existing private history reader.
-- Its signature and existing grants stay unchanged; ordinary groups stay exact.
CREATE OR REPLACE FUNCTION app.read_native_transition_sources(p_game uuid,p_context uuid,p_anchor_checkpoint uuid,p_after_revision numeric,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE source jsonb;anchor app.game_checkpoint;checkpoint_row app.game_checkpoint;
 creator app.command_receipt;native_parent app.command_receipt;bank_evidence jsonb;before_bank jsonb;body jsonb;marker jsonb;events jsonb;groups jsonb:='[]'::jsonb;
 last_revision numeric;last_sequence numeric;next_revision numeric;result jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
BEGIN
 IF p_game IS NULL OR p_context IS NULL OR p_anchor_checkpoint IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_anchor_checkpoint)
  OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32
  OR (p_after_revision IS NOT NULL AND p_after_revision::text !~ '^(0|[1-9][0-9]{0,77})$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded anchored native history request required';END IF;
 source:=app.read_native_game_source(p_game);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'contextId' IS DISTINCT FROM p_context::text THEN RETURN unavailable;END IF;
 SELECT * INTO anchor FROM app.game_checkpoint WHERE id=p_anchor_checkpoint AND game_id=p_game AND context_id=p_context
  AND accepted_revision<=(source->'data'->>'acceptedRevision')::numeric AND source_receipt_id IS NOT NULL;
 IF anchor.id IS NULL THEN RETURN unavailable;END IF;
 PERFORM app.check_native_creator_frame(anchor.source_receipt_id);
 IF p_after_revision IS NOT NULL AND (p_after_revision>anchor.accepted_revision OR NOT EXISTS(
  SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context AND accepted_revision=p_after_revision)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native history cursor is outside its anchor';END IF;
 SELECT max(sequence) INTO last_sequence FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND accepted_revision<=anchor.accepted_revision;
 IF last_sequence IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history anchor has no event prefix';END IF;
 FOR checkpoint_row IN SELECT * FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
   AND accepted_revision<=anchor.accepted_revision AND (p_after_revision IS NULL OR accepted_revision>p_after_revision)
   ORDER BY accepted_revision LIMIT p_limit LOOP
  PERFORM app.check_native_creator_frame(checkpoint_row.source_receipt_id);
  SELECT * INTO STRICT creator FROM app.command_receipt WHERE id=checkpoint_row.source_receipt_id;
  body:=CASE WHEN creator.scope='game_opening_v1' THEN creator.normalized_source ELSE creator.normalized_source->'candidate' END;
  -- check_native_creator_frame rejects explicit null/unknown raw markers before
  -- ->> projects historical absence to the nullable inspection representation.
  marker:=CASE WHEN body ? 'eventSchemaRevision' THEN body->'eventSchemaRevision' ELSE 'null'::jsonb END;
  bank_evidence:=NULL;
  -- Only the maintained new cause claims bank-input semantics. Historical
  -- generic timeout groups retain their exact previous projection.
  IF body->'terminal'->>'reason'='native_bank_expired' THEN
   IF creator.scope<>'host_candidate_v1' OR creator.state<>'accepted'
    OR body->'bankTimeout' IS NULL OR body->'bankTimeout'='null'::jsonb THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires its timeout creator';END IF;
   SELECT * INTO native_parent FROM app.command_receipt
    WHERE scope='native_host_input_v1' AND state='accepted'
     AND normalized_source->>'childReceiptId'=creator.id::text;
   -- check_native_commit_receipt deliberately ignores absent/other families;
   -- establish the exact accepted host parent before invoking that checker.
   IF native_parent.id IS NULL OR native_parent.normalized_source->'original'->>'kind' IS DISTINCT FROM 'clock'
    OR native_parent.normalized_source->'original'->>'inputRef' IS DISTINCT FROM body->'bankTimeout'->>'proofRef' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires exact clock original and input proof';END IF;
   PERFORM app.check_native_commit_receipt(native_parent.id);
   PERFORM app.check_bank_forfeit_game(p_game);
   SELECT value INTO before_bank FROM jsonb_array_elements(creator.normalized_source->'before') value
    WHERE value->>'seatId'=body->'bankTimeout'->>'seatId';
   IF before_bank IS NULL OR before_bank->>'clockRevision' IS DISTINCT FROM body->'bankTimeout'->>'clockRevision' THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native bank history requires its immutable clock basis';END IF;
   bank_evidence:=jsonb_build_object('originalRequestId',native_parent.request_ref,
    'original',native_parent.normalized_source->'original','timeout',body->'bankTimeout',
    'beforeClock',before_bank,'acceptedAt',creator.normalized_source->'acceptedAt');
  END IF;

  SELECT coalesce(jsonb_agg(jsonb_build_object(
    'eventId',e.id,'sequence',e.sequence::text,'acceptedRevision',e.accepted_revision::text,
    'privatePayloadRef',e.private_payload_ref,'acceptedAt',to_char(e.accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
    'creatorReceiptId',e.source_receipt_id,'facts',(
      SELECT coalesce(jsonb_agg(jsonb_build_object('accountId',f.account_id,'kind',f.kind,'factRevision',f.fact_revision,'value',f.value::text)
        ORDER BY f.account_id,f.kind,f.fact_revision,f.id),'[]'::jsonb)
      FROM app.game_activity_fact f WHERE f.source_event_id=e.id)) ORDER BY e.sequence),'[]'::jsonb)
    INTO events FROM app.game_event e WHERE e.game_id=p_game AND e.context_id=p_context AND e.source_receipt_id=creator.id;
  IF jsonb_array_length(events)=0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native history checkpoint has no exact creator events';END IF;
  groups:=groups||jsonb_build_array(jsonb_build_object('checkpointId',checkpoint_row.id,'acceptedRevision',checkpoint_row.accepted_revision::text,
    'stateRef',checkpoint_row.private_state_ref,'factsRef',checkpoint_row.facts_ref,'creatorReceiptId',creator.id,
    'eventSchemaRevision',marker,'events',events)||CASE WHEN bank_evidence IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('bankExpiry',bank_evidence) END);
  last_revision:=checkpoint_row.accepted_revision;
 END LOOP;
 IF last_revision IS NOT NULL AND EXISTS(SELECT 1 FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context
  AND accepted_revision>last_revision AND accepted_revision<=anchor.accepted_revision) THEN next_revision:=last_revision;END IF;
 result:=jsonb_build_object('status','success','data',jsonb_build_object('anchor',jsonb_build_object(
  'gameId',p_game,'contextId',p_context,'anchorCheckpointId',anchor.id,'anchorStateRef',anchor.private_state_ref,
  'anchorCreatorReceiptId',anchor.source_receipt_id,'throughAcceptedRevision',anchor.accepted_revision::text,'throughEventSequence',last_sequence::text),
  'groups',groups,'nextAfterRevision',next_revision::text));
 IF octet_length(result::text)>8388608 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native history response exceeds bounded source budget';END IF;
 RETURN result;
END $$;
COMMIT;
