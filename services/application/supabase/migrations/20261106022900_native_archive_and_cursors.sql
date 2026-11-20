BEGIN;

CREATE FUNCTION app.read_native_archive_source(p_game uuid,p_context uuid,p_viewer uuid,p_after_sequence numeric,p_limit integer,p_disclosure text,p_source_view_receipt uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE view_receipt app.command_receipt;view_source jsonb;event_row app.game_event;checkpoint app.game_checkpoint;
 frontier numeric;scanned numeric:=p_after_sequence;more boolean:=false;items jsonb:='[]';item jsonb;body jsonb;
 opening app.command_receipt;replay jsonb;manifest app.replay_manifest;
BEGIN
 IF p_game IS NULL OR p_context IS NULL OR p_viewer IS NULL OR p_source_view_receipt IS NULL
  OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
  OR(p_after_sequence IS NOT NULL AND p_after_sequence::text !~ '^(0|[1-9][0-9]{0,77})$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native archive query';END IF;
 SELECT * INTO view_receipt FROM app.command_receipt WHERE id=p_source_view_receipt AND scope='native_view_v1' AND state='accepted';
 IF view_receipt.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='P0002',MESSAGE='native archive View unavailable';END IF;
 PERFORM app.check_native_view_receipt(view_receipt.id);view_source:=view_receipt.normalized_source;
 IF view_receipt.target_ref IS DISTINCT FROM p_game::text OR view_source->'basis'->>'contextId' IS DISTINCT FROM p_context::text
  OR view_source->>'viewerSeatId' IS DISTINCT FROM p_viewer::text OR view_source->>'disclosure' IS DISTINCT FROM p_disclosure THEN
  RAISE EXCEPTION USING ERRCODE='P0002',MESSAGE='native archive View scope unavailable';END IF;
 frontier:=(view_source->>'throughEventSequence')::numeric;
 IF p_after_sequence IS NOT NULL AND(p_after_sequence>frontier OR NOT EXISTS(
  SELECT 1 FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND sequence=p_after_sequence)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native archive boundary outside issued View';END IF;
 SELECT * INTO STRICT opening FROM app.command_receipt WHERE id=(view_source->>'openingReceiptId')::uuid;
 -- Prefix disclosure inherently has a prior immutable171 publication. Other
 -- scans keep replay NULL so later publication cannot change an issued scan.
 IF p_disclosure='operational-prefix' THEN
  SELECT * INTO STRICT manifest FROM app.replay_manifest WHERE game_id=p_game AND context_id=p_context AND source_receipt_id IS NOT NULL;
  IF frontier>manifest.through_event_sequence THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='archive View exceeds published prefix';END IF;
  replay:=jsonb_build_object('manifestId',manifest.id,'artifactRef',manifest.artifact_ref,'visibilityIndexRef',manifest.visibility_index_ref,
   'schemaRevision',manifest.schema_revision,'throughSequence',manifest.through_event_sequence::text,'throughRevision',manifest.through_accepted_revision::text);
 END IF;
 FOR event_row IN SELECT * FROM app.game_event WHERE game_id=p_game AND context_id=p_context
  AND(p_after_sequence IS NULL OR sequence>p_after_sequence) AND sequence<=frontier ORDER BY sequence LIMIT p_limit+1 LOOP
  IF jsonb_array_length(items)=p_limit THEN more:=true;EXIT;END IF;
  IF event_row.sequence IS DISTINCT FROM coalesce(scanned+1,0) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native archive raw sequence gap';END IF;
  SELECT * INTO checkpoint FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context AND accepted_revision=event_row.accepted_revision;
  IF checkpoint.id IS NULL OR checkpoint.source_receipt_id IS DISTINCT FROM event_row.source_receipt_id
   OR event_row.accepted_revision>(view_source->'basis'->>'revision')::numeric
   OR NOT app.native_u64_is_canonical(event_row.accepted_revision::text)
   OR NOT app.native_artifact_ref_is_valid(event_row.private_payload_ref,p_game,p_context,'event')
   OR NOT app.native_artifact_ref_is_valid(checkpoint.private_state_ref,p_game,p_context,'state') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native archive event lacks exact accepted frame';END IF;
  IF checkpoint.accepted_revision=0 THEN
   IF checkpoint.source_receipt_id IS DISTINCT FROM (opening.normalized_source->>'childReceiptId')::uuid THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native archive opening creator differs';END IF;
  ELSIF NOT EXISTS(SELECT 1 FROM app.command_receipt parent WHERE parent.scope IN('native_player_v1','native_host_input_v1')
   AND parent.state='accepted' AND parent.target_ref=p_game::text
   AND parent.normalized_source->>'acceptedCheckpointId'=checkpoint.id::text
   AND parent.normalized_source->>'childReceiptId'=checkpoint.source_receipt_id::text) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native archive frame lacks original protocol source';END IF;
  PERFORM app.check_native_creator_frame(event_row.source_receipt_id);
  item:=jsonb_build_object('eventId',event_row.id,'sequence',event_row.sequence::text,'acceptedRevision',event_row.accepted_revision::text,
   'eventRef',event_row.private_payload_ref,'creatorReceiptId',event_row.source_receipt_id,'checkpointId',checkpoint.id,'stateRef',checkpoint.private_state_ref);
  IF octet_length(jsonb_build_object('items',items||jsonb_build_array(item))::text)>1000000 THEN
   IF jsonb_array_length(items)=0 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native archive record exceeds page budget';END IF;
   more:=true;EXIT;
  END IF;
  items:=items||jsonb_build_array(item);scanned:=event_row.sequence;
 END LOOP;
 body:=jsonb_build_object('gameId',p_game,'contextId',p_context,'viewerSeatId',p_viewer,'disclosure',p_disclosure,
  'afterSequence',p_after_sequence::text,'limit',p_limit,'sourceViewReceiptId',p_source_view_receipt,
  'openingReceiptId',view_source->>'openingReceiptId','items',items,'scannedThrough',scanned::text,
  'nextAfter',CASE WHEN more THEN scanned::text END,'replay',replay);
 body:=body||jsonb_build_object('sourceHash',app.auth_json_hash(body));
 IF octet_length(body::text)>1048576 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native archive source exceeds page budget';END IF;
 RETURN body;
END $$;

CREATE UNIQUE INDEX native_cursor_token ON app.command_receipt((normalized_source->>'token')) WHERE scope='native_cursor_v1' AND state='accepted';

CREATE FUNCTION app.check_native_cursor_receipt(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;view_receipt app.command_receipt;source jsonb;view_source jsonb;semantic jsonb;scan jsonb;
 game_id uuid;context_id uuid;viewer_id uuid;through numeric;purpose text;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt AND scope='native_cursor_v1';
 IF receipt.id IS NULL THEN RETURN;END IF;source:=receipt.normalized_source;
 IF receipt.state<>'accepted' OR jsonb_typeof(source) IS DISTINCT FROM 'object' OR source->>'version' IS DISTINCT FROM 'native-cursor-v1'
  OR jsonb_typeof(source->'basis') IS DISTINCT FROM 'object'
  OR EXISTS(SELECT 1 FROM unnest(ARRAY['viewerSeatId','throughEventSequence','purpose','sourceReceiptId','disclosure','token']) k WHERE jsonb_typeof(source->k) IS DISTINCT FROM 'string')
  OR source->>'throughEventSequence' !~ '^(0|[1-9][0-9]{0,77})$'
  OR source->>'purpose' NOT IN('snapshot','history')
  OR source->>'token' !~ '^n1\.[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
  OR receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native cursor lacks exact accepted issuance';END IF;
 game_id:=(source->'basis'->>'gameId')::uuid;context_id:=(source->'basis'->>'contextId')::uuid;
 viewer_id:=(source->>'viewerSeatId')::uuid;through:=(source->>'throughEventSequence')::numeric;purpose:=source->>'purpose';
 SELECT * INTO view_receipt FROM app.command_receipt WHERE id=(source->>'sourceReceiptId')::uuid AND scope='native_view_v1' AND state='accepted';
 IF view_receipt.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native cursor source View missing';END IF;
 PERFORM app.check_native_view_receipt(view_receipt.id);view_source:=view_receipt.normalized_source;
 semantic:=(source-ARRAY['version','token','scan']);
 IF semantic IS DISTINCT FROM jsonb_build_object('basis',view_source->'basis','viewerSeatId',view_source->>'viewerSeatId',
   'throughEventSequence',through::text,'purpose',purpose,'sourceReceiptId',view_receipt.id,'disclosure',view_source->>'disclosure')
  OR receipt.target_ref IS DISTINCT FROM game_id::text OR receipt.principal_key IS DISTINCT FROM 'native-viewer:'||viewer_id::text
  OR receipt.request_ref IS DISTINCT FROM view_receipt.id::text||':'||purpose||':'||through::text
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(semantic)
  OR receipt.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',receipt.id))
  OR through>(view_source->>'throughEventSequence')::numeric THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native cursor differs from accepted View frontier';END IF;
 IF purpose='snapshot' THEN
  IF through<>(view_source->>'throughEventSequence')::numeric OR source->'scan' IS DISTINCT FROM 'null'::jsonb THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='snapshot cursor must bind complete View frontier';END IF;
 ELSE
  IF jsonb_typeof(source->'scan') IS DISTINCT FROM 'object' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='history cursor scan missing';END IF;
  scan:=app.read_native_archive_source(game_id,context_id,viewer_id,(source->'scan'->>'afterSequence')::numeric,
   (source->'scan'->>'limit')::integer,source->>'disclosure',view_receipt.id);
  IF scan IS DISTINCT FROM source->'scan' OR scan->>'scannedThrough' IS DISTINCT FROM through::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='history cursor differs from exact immutable scan';END IF;
 END IF;
END $$;
CREATE FUNCTION app.guard_native_cursor_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$ BEGIN PERFORM app.check_native_cursor_receipt(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER native_cursor_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope='native_cursor_v1') EXECUTE FUNCTION app.guard_native_cursor_receipt();

CREATE FUNCTION app.read_native_cursor_source(p_game uuid,p_viewer uuid,p_token text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 IF p_token IS NULL OR p_token !~ '^n1\.[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('cursor','unavailable'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE scope='native_cursor_v1' AND state='accepted' AND target_ref=p_game::text
  AND principal_key='native-viewer:'||p_viewer::text AND normalized_source->>'token'=p_token;
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('cursor','unavailable'));END IF;
 PERFORM app.check_native_cursor_receipt(receipt.id);
 RETURN jsonb_build_object('status','success','data',receipt.normalized_source-'scan');
END $$;

CREATE FUNCTION app.issue_native_cursor_source(p_game uuid,p_context uuid,p_epoch text,p_revision text,p_viewer uuid,p_through_sequence numeric,p_purpose text,p_source_receipt uuid,p_scan jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;view_receipt app.command_receipt;semantic jsonb;view_source jsonb;scan jsonb;
 match_id uuid;token text;key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native cursor issuance requires read committed retry';END IF;
 IF p_game IS NULL OR p_context IS NULL OR p_viewer IS NULL OR p_source_receipt IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_viewer,p_source_receipt)
  OR NOT app.native_u64_is_canonical(p_epoch) OR NOT app.native_u64_is_canonical(p_revision)
  OR p_through_sequence IS NULL OR p_through_sequence::text !~ '^(0|[1-9][0-9]{0,77})$'
  OR p_purpose IS NULL OR p_purpose NOT IN('snapshot','history') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native cursor original';END IF;
 SELECT * INTO view_receipt FROM app.command_receipt WHERE id=p_source_receipt AND scope='native_view_v1' AND state='accepted';
 IF view_receipt.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='P0002',MESSAGE='native cursor View unavailable';END IF;
 view_source:=view_receipt.normalized_source;
 IF view_source->'basis' IS DISTINCT FROM jsonb_build_object('gameId',p_game,'contextId',p_context,'epoch',p_epoch,'revision',p_revision)
  OR view_source->>'viewerSeatId' IS DISTINCT FROM p_viewer::text THEN RAISE EXCEPTION USING ERRCODE='P0002',MESSAGE='native cursor View scope unavailable';END IF;
 semantic:=jsonb_build_object('basis',view_source->'basis','viewerSeatId',p_viewer,'throughEventSequence',p_through_sequence::text,
  'purpose',p_purpose,'sourceReceiptId',p_source_receipt,'disclosure',view_source->>'disclosure');
 key:=p_source_receipt::text||':'||p_purpose||':'||p_through_sequence::text;
 receipt:=app.begin_native_source_receipt('native-viewer:'||p_viewer::text,key,'native_cursor_v1',p_game,semantic,NULL,NULL);
 IF receipt.state<>'pending' THEN PERFORM app.check_native_cursor_receipt(receipt.id);RETURN jsonb_build_object('status','success','data',receipt.normalized_source-'scan');END IF;
 SELECT game_row.match_id INTO match_id FROM app.game game_row WHERE game_row.id=p_game;
 PERFORM id FROM app.match WHERE id=match_id FOR SHARE;PERFORM id FROM app.game WHERE id=p_game FOR SHARE;
 PERFORM app.check_native_view_receipt(view_receipt.id);
 IF p_through_sequence>(view_source->>'throughEventSequence')::numeric THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native cursor exceeds View frontier';END IF;
 IF p_purpose='snapshot' THEN
  IF p_scan IS NOT NULL OR p_through_sequence<>(view_source->>'throughEventSequence')::numeric THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='snapshot cursor requires complete View and no scan';END IF;
 ELSE
  IF jsonb_typeof(p_scan) IS DISTINCT FROM 'object' OR octet_length(p_scan::text)>1048576 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='history cursor requires bounded exact scan';END IF;
  scan:=app.read_native_archive_source(p_game,p_context,p_viewer,(p_scan->>'afterSequence')::numeric,(p_scan->>'limit')::integer,view_source->>'disclosure',p_source_receipt);
  IF scan IS DISTINCT FROM p_scan OR scan->>'scannedThrough' IS DISTINCT FROM p_through_sequence::text THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='history cursor scan differs from source';END IF;
 END IF;
 token:='n1.'||gen_random_uuid()::text;
 UPDATE app.command_receipt SET normalized_source=semantic||jsonb_build_object('version','native-cursor-v1','token',token,'scan',p_scan) WHERE id=receipt.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',receipt.id));
 PERFORM app.check_native_cursor_receipt(receipt.id);
 RETURN app.read_native_cursor_source(p_game,p_viewer,token);
END $$;

REVOKE ALL ON FUNCTION app.read_native_archive_source(uuid,uuid,uuid,numeric,integer,text,uuid),app.check_native_cursor_receipt(uuid),
 app.guard_native_cursor_receipt(),app.read_native_cursor_source(uuid,uuid,text),app.issue_native_cursor_source(uuid,uuid,text,text,uuid,numeric,text,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
