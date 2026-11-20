BEGIN;

CREATE FUNCTION app.native_receipt_query_is_valid(p_query jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE original jsonb;
BEGIN
 IF jsonb_typeof(p_query) IS DISTINCT FROM 'object' OR octet_length(p_query::text)>1048576 THEN RETURN false;END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_query) k WHERE k NOT IN('gameId','contextId','outstanding'))
  OR jsonb_typeof(p_query->'gameId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(p_query->>'gameId')
  OR jsonb_typeof(p_query->'contextId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(p_query->>'contextId')
  OR jsonb_typeof(p_query->'outstanding') IS DISTINCT FROM 'array' THEN RETURN false;END IF;
 IF jsonb_array_length(p_query->'outstanding')>100 OR(SELECT count(DISTINCT x->>'requestId') FROM jsonb_array_elements(p_query->'outstanding') x)<>jsonb_array_length(p_query->'outstanding') THEN RETURN false;END IF;
 FOR original IN SELECT * FROM jsonb_array_elements(p_query->'outstanding') LOOP
  IF NOT app.native_player_original_is_valid(original) OR original->'basis'->>'gameId' IS DISTINCT FROM p_query->>'gameId'
   OR original->'basis'->>'contextId' IS DISTINCT FROM p_query->>'contextId' THEN RETURN false;END IF;
 END LOOP;
 RETURN true;
END $$;

CREATE FUNCTION app.read_native_own_receipt(p_game uuid,p_context uuid,p_seat uuid,p_original jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 IF NOT app.native_player_original_is_valid(p_original) OR p_original->'basis'->>'gameId' IS DISTINCT FROM p_game::text
  OR p_original->'basis'->>'contextId' IS DISTINCT FROM p_context::text THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native receipt original';END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='native-seat:'||p_seat::text AND scope='native_player_v1' AND request_ref=p_original->>'requestId';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','success','data',jsonb_build_object('actorSeatId',p_seat,'original',p_original,'state','unknown','response',NULL));END IF;
 IF receipt.target_ref IS DISTINCT FROM p_game::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('actorSeatId',p_seat,'original',p_original)) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='native original not terminal';END IF;
 PERFORM app.check_native_commit_receipt(receipt.id);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('actorSeatId',p_seat,'original',p_original,'state',receipt.state,'response',receipt.response_payload));
END $$;

CREATE FUNCTION app.read_native_host_receipt(p_game uuid,p_seat uuid,p_original jsonb) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE opening jsonb;
BEGIN
 IF NOT app.native_player_original_is_valid(p_original) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native host receipt query';END IF;
 opening:=app.read_native_opening_source(p_game);
 IF opening->>'status' IS DISTINCT FROM 'success' OR opening->'data'->>'contextId' IS DISTINCT FROM p_original->'basis'->>'contextId'
  OR NOT EXISTS(SELECT 1 FROM app.match_seat seat JOIN app.game game_row ON game_row.match_id=seat.match_id WHERE seat.id=p_seat AND game_row.id=p_game AND seat.deck_accepted_at IS NOT NULL) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));END IF;
 RETURN app.read_native_own_receipt(p_game,(p_original->'basis'->>'contextId')::uuid,p_seat,p_original);
END $$;

CREATE FUNCTION app.read_native_player_receipts(p_subject uuid,p_session uuid,p_grant uuid,p_epoch numeric,p_secret_ref text,p_query jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;authority jsonb;actor jsonb;opening jsonb;item jsonb;original jsonb;items jsonb:='[]';body jsonb;
 native_game_id uuid;native_context_id uuid;native_match_id uuid;seat_id uuid;is_human boolean:=p_subject IS NOT NULL;
BEGIN
 IF is_human THEN
  IF p_grant IS NOT NULL OR p_epoch IS NOT NULL OR p_secret_ref IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='mixed native authority';END IF;
  authority:=app.read_auth_session_authority(p_subject,p_session);
  SELECT * INTO player FROM app.account WHERE auth_subject=p_subject AND state IN('active','deletion_pending');
  IF authority IS NULL OR player.id IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));END IF;
 ELSIF p_session IS NOT NULL OR p_grant IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 IF NOT app.native_receipt_query_is_valid(p_query) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 native_game_id:=(p_query->>'gameId')::uuid;native_context_id:=(p_query->>'contextId')::uuid;
 SELECT match_id INTO native_match_id FROM app.game WHERE id=native_game_id;
 IF is_human THEN
  SELECT seat.id INTO seat_id FROM app.match_seat seat WHERE seat.match_id=native_match_id AND seat.controller_kind='human' AND seat.account_id=player.id;
  IF seat_id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));END IF;
 ELSE
  actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,native_match_id);
  IF actor IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
  seat_id:=(actor->>'seatId')::uuid;
 END IF;
 -- No current host fence, game advancement, handle re-resolution or native work.
 opening:=app.read_native_opening_source(native_game_id);
 IF opening->>'status' IS DISTINCT FROM 'success' OR opening->'data'->>'contextId' IS DISTINCT FROM native_context_id::text THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('game','unavailable'));END IF;
 FOR original IN SELECT * FROM jsonb_array_elements(p_query->'outstanding') LOOP
  item:=app.read_native_own_receipt(native_game_id,native_context_id,seat_id,original);
  IF item->>'status' IS DISTINCT FROM 'success' THEN RETURN item;END IF;
  items:=items||jsonb_build_array(item->'data');
 END LOOP;
 IF is_human THEN
  authority:=app.read_auth_session_authority(p_subject,p_session);
  IF authority IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method'
   OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=player.id AND auth_subject=p_subject AND state IN('active','deletion_pending')) THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));END IF;
 ELSIF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,native_match_id) IS DISTINCT FROM actor THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('actor','unavailable'));END IF;
 body:=jsonb_build_object('status','success','data',jsonb_build_object('receipts',items));
 IF octet_length(body::text)>2097152 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native receipt recovery exceeds private response budget';END IF;
 RETURN body;
END $$;
CREATE FUNCTION app.read_native_human_receipts(p_subject uuid,p_session uuid,p_query jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_native_player_receipts(p_subject,p_session,NULL,NULL,NULL,p_query) $$;
CREATE FUNCTION app.read_native_controller_receipts(p_grant uuid,p_epoch numeric,p_secret_ref text,p_query jsonb) RETURNS jsonb
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$ SELECT app.read_native_player_receipts(NULL,NULL,p_grant,p_epoch,p_secret_ref,p_query) $$;

REVOKE ALL ON FUNCTION app.native_receipt_query_is_valid(jsonb),app.read_native_own_receipt(uuid,uuid,uuid,jsonb),
 app.read_native_host_receipt(uuid,uuid,jsonb),app.read_native_player_receipts(uuid,uuid,uuid,numeric,text,jsonb),
 app.read_native_human_receipts(uuid,uuid,jsonb),app.read_native_controller_receipts(uuid,numeric,text,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
