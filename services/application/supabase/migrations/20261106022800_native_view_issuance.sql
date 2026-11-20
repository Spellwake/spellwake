BEGIN;

CREATE FUNCTION app.native_checkpoint_source(p_game uuid,p_context uuid,p_epoch text,p_revision text,p_viewer uuid,p_disclosure text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE original jsonb;current_source jsonb;checkpoint app.game_checkpoint;game_row app.game;
 disclosure jsonb;manifest app.replay_manifest;frontier numeric;
BEGIN
 IF p_game IS NULL OR p_context IS NULL OR p_viewer IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_viewer)
  OR NOT app.native_u64_is_canonical(p_epoch) OR NOT app.native_u64_is_canonical(p_revision)
  OR p_disclosure IS NULL OR p_disclosure NOT IN('live','completed','operational-prefix') THEN RETURN NULL;END IF;
 original:=app.read_native_opening_source(p_game);current_source:=app.read_native_game_source(p_game);
 IF original->>'status' IS DISTINCT FROM 'success' OR current_source->>'status' IS DISTINCT FROM 'success'
  OR original->'data'->>'contextId' IS DISTINCT FROM p_context::text OR original->'data'->>'nativeEpoch' IS DISTINCT FROM p_epoch
  OR current_source->'data'->>'contextId' IS DISTINCT FROM p_context::text THEN RETURN NULL;END IF;
 SELECT * INTO game_row FROM app.game WHERE id=p_game;
 IF NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=p_viewer AND match_id=game_row.match_id
  AND deck_accepted_at IS NOT NULL AND source_revision_id IS NOT NULL AND deck_snapshot_ref IS NOT NULL) THEN RETURN NULL;END IF;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE game_id=p_game AND context_id=p_context AND accepted_revision=p_revision::numeric;
 IF checkpoint.id IS NULL OR checkpoint.accepted_revision>game_row.accepted_revision THEN RETURN NULL;END IF;
 IF checkpoint.accepted_revision=0 THEN
  IF checkpoint.id::text IS DISTINCT FROM original->'data'->>'checkpointId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native original checkpoint differs';END IF;
 ELSE
  -- Core-only owner fixtures do not become the new public native protocol.
  IF NOT EXISTS(SELECT 1 FROM app.command_receipt parent WHERE parent.scope IN('native_player_v1','native_host_input_v1')
   AND parent.state='accepted' AND parent.target_ref=p_game::text
   AND parent.normalized_source->>'childReceiptId'=checkpoint.source_receipt_id::text
   AND parent.normalized_source->>'acceptedCheckpointId'=checkpoint.id::text) THEN RETURN NULL;END IF;
 END IF;
 PERFORM app.check_native_creator_frame(checkpoint.source_receipt_id);
 SELECT max(sequence) INTO frontier FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND accepted_revision<=checkpoint.accepted_revision;
 IF frontier IS NULL OR(SELECT count(*)::numeric FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND sequence<=frontier)<>frontier+1
  OR EXISTS(SELECT 1 FROM app.game_event WHERE game_id=p_game AND context_id=p_context AND sequence<=frontier AND accepted_revision>checkpoint.accepted_revision) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native checkpoint has inconsistent full event frontier';END IF;
 disclosure:=app.native_disclosure_source(p_game,p_context);
 IF(p_disclosure='completed' AND disclosure->'completed' IS DISTINCT FROM 'true'::jsonb)
  OR(p_disclosure='operational-prefix' AND disclosure->'operationalPrefix' IS DISTINCT FROM 'true'::jsonb)
  OR(p_disclosure='live' AND disclosure->'live' IS DISTINCT FROM 'true'::jsonb) THEN RETURN NULL;END IF;
 IF p_disclosure IN('completed','operational-prefix') AND disclosure->>'replayManifestId' IS NOT NULL THEN
  SELECT * INTO STRICT manifest FROM app.replay_manifest WHERE id=(disclosure->>'replayManifestId')::uuid;
  IF frontier>manifest.through_event_sequence OR checkpoint.accepted_revision>manifest.through_accepted_revision THEN RETURN NULL;END IF;
 END IF;
 RETURN jsonb_build_object('basis',jsonb_build_object('gameId',p_game,'contextId',p_context,'epoch',p_epoch,'revision',p_revision),
  'viewerSeatId',p_viewer,'disclosure',p_disclosure,'openingReceiptId',original->'data'->>'receiptId',
  'checkpointId',checkpoint.id,'throughEventSequence',frontier::text);
END $$;

CREATE FUNCTION app.check_native_view_receipt(p_receipt uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;source jsonb;cut jsonb;semantic jsonb;game_id uuid;context_id uuid;viewer_id uuid;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt AND scope='native_view_v1';
 IF receipt.id IS NULL THEN RETURN;END IF;source:=receipt.normalized_source;
 IF receipt.state<>'accepted' OR jsonb_typeof(source) IS DISTINCT FROM 'object'
  OR source->>'version' IS DISTINCT FROM 'native-view-v1' OR jsonb_typeof(source->'basis') IS DISTINCT FROM 'object'
  OR jsonb_typeof(source->'viewerSeatId') IS DISTINCT FROM 'string' OR jsonb_typeof(source->'disclosure') IS DISTINCT FROM 'string'
  OR jsonb_typeof(source->'viewRef') IS DISTINCT FROM 'string'
  OR receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native view has no accepted original source';END IF;
 game_id:=(source->'basis'->>'gameId')::uuid;context_id:=(source->'basis'->>'contextId')::uuid;viewer_id:=(source->>'viewerSeatId')::uuid;
 semantic:=jsonb_build_object('basis',source->'basis','viewerSeatId',viewer_id,'disclosure',source->>'disclosure');
 cut:=app.native_checkpoint_source(game_id,context_id,source->'basis'->>'epoch',source->'basis'->>'revision',viewer_id,source->>'disclosure');
 IF cut IS NULL OR(source-ARRAY['version','viewRef']) IS DISTINCT FROM cut
  OR receipt.target_ref IS DISTINCT FROM game_id::text OR receipt.principal_key IS DISTINCT FROM 'native-viewer:'||viewer_id::text
  OR receipt.request_ref IS DISTINCT FROM context_id::text||':'||(source->'basis'->>'epoch')||':'||(source->'basis'->>'revision')||':'||(source->>'disclosure')
  OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(semantic)
  OR receipt.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',receipt.id))
  OR NOT app.native_artifact_ref_is_valid(source->>'viewRef',game_id,context_id,'view') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native view differs from accepted cut and original';END IF;
END $$;
CREATE FUNCTION app.guard_native_view_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$ BEGIN PERFORM app.check_native_view_receipt(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER native_view_original_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED
 FOR EACH ROW WHEN(NEW.scope='native_view_v1') EXECUTE FUNCTION app.guard_native_view_receipt();

CREATE FUNCTION app.read_native_view_source(p_game uuid,p_context uuid,p_epoch text,p_revision text,p_viewer uuid,p_disclosure text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;key text;
BEGIN
 IF NOT app.native_u64_is_canonical(p_epoch) OR NOT app.native_u64_is_canonical(p_revision) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('view','unavailable'));END IF;
 key:=p_context::text||':'||p_epoch||':'||p_revision||':'||p_disclosure;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='native-viewer:'||p_viewer::text
  AND scope='native_view_v1' AND target_ref=p_game::text AND request_ref=key AND state='accepted';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('view','unavailable'));END IF;
 PERFORM app.check_native_view_receipt(receipt.id);
 RETURN jsonb_build_object('status','success','data',receipt.normalized_source||jsonb_build_object('receiptId',receipt.id));
END $$;

CREATE FUNCTION app.publish_native_view_source(p_game uuid,p_context uuid,p_epoch text,p_revision text,p_viewer uuid,p_disclosure text,p_view_ref text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;semantic jsonb;basis jsonb;cut jsonb;match_id uuid;key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native view issuance requires read committed retry';END IF;
 IF p_game IS NULL OR p_context IS NULL OR p_viewer IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context,p_viewer)
  OR NOT app.native_u64_is_canonical(p_epoch) OR NOT app.native_u64_is_canonical(p_revision)
  OR p_disclosure IS NULL OR p_disclosure NOT IN('live','completed','operational-prefix') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native view original';END IF;
 basis:=jsonb_build_object('gameId',p_game,'contextId',p_context,'epoch',p_epoch,'revision',p_revision);
 semantic:=jsonb_build_object('basis',basis,'viewerSeatId',p_viewer,'disclosure',p_disclosure);
 key:=p_context::text||':'||p_epoch||':'||p_revision||':'||p_disclosure;
 receipt:=app.begin_native_source_receipt('native-viewer:'||p_viewer::text,key,'native_view_v1',p_game,semantic,NULL,NULL);
 -- Semantic first winner: new random View refs never replace an issued registry.
 IF receipt.state<>'pending' THEN RETURN app.read_native_view_source(p_game,p_context,p_epoch,p_revision,p_viewer,p_disclosure);END IF;
 IF NOT app.native_artifact_ref_is_valid(p_view_ref,p_game,p_context,'view') THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native View artifact scope';END IF;
 SELECT game_row.match_id INTO match_id FROM app.game game_row WHERE game_row.id=p_game;
 PERFORM id FROM app.match WHERE id=match_id FOR SHARE;
 PERFORM id FROM app.game WHERE id=p_game FOR SHARE;
 cut:=app.native_checkpoint_source(p_game,p_context,p_epoch,p_revision,p_viewer,p_disclosure);
 IF cut IS NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native view source unavailable';END IF;
 UPDATE app.command_receipt SET normalized_source=cut||jsonb_build_object('version','native-view-v1','viewRef',p_view_ref) WHERE id=receipt.id;
 PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',receipt.id));
 PERFORM app.check_native_view_receipt(receipt.id);
 RETURN app.read_native_view_source(p_game,p_context,p_epoch,p_revision,p_viewer,p_disclosure);
END $$;

REVOKE ALL ON FUNCTION app.native_checkpoint_source(uuid,uuid,text,text,uuid,text),app.check_native_view_receipt(uuid),app.guard_native_view_receipt(),
 app.read_native_view_source(uuid,uuid,text,text,uuid,text),app.publish_native_view_source(uuid,uuid,text,text,uuid,text,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
