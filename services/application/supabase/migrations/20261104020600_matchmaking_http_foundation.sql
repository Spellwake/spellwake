BEGIN;
CREATE FUNCTION app.enqueue_matchmaking_command(p_subject uuid,p_session uuid,p_request uuid,p_mode text,p_competitive boolean,p_alternatives jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.enqueue_matchmaking_intent(player.id,p_mode,p_competitive,p_alternatives,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_parameter_value THEN IF SQLERRM='queue intent exceeds command envelope' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('intent','invalid envelope'));END IF;RAISE;
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.cancel_matchmaking_command(p_subject uuid,p_session uuid,p_request uuid,p_entry uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;intent_id uuid;claim_room uuid;actors uuid[];observed_intent uuid;observed_room uuid;observed_actors uuid[];
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue cancellation requires read committed retry';END IF;
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 SELECT intent_receipt_id INTO intent_id FROM app.queue_entry WHERE id=p_entry AND account_id=actor_id;
 SELECT claimed_room_id INTO claim_room FROM app.queue_entry WHERE intent_receipt_id=intent_id AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM(SELECT actor_id a UNION SELECT account_id FROM app.queue_entry WHERE claimed_room_id=claim_room UNION SELECT sponsor_id FROM app.room_slot WHERE room_id=claim_room AND retired_at IS NULL AND sponsor_id IS NOT NULL) ids;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 SELECT intent_receipt_id INTO observed_intent FROM app.queue_entry WHERE id=p_entry AND account_id=actor_id;
 SELECT claimed_room_id INTO observed_room FROM app.queue_entry WHERE intent_receipt_id=observed_intent AND claimed_room_id IS NOT NULL ORDER BY id LIMIT 1;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO observed_actors FROM(SELECT actor_id a UNION SELECT account_id FROM app.queue_entry WHERE claimed_room_id=observed_room UNION SELECT sponsor_id FROM app.room_slot WHERE room_id=observed_room AND retired_at IS NULL AND sponsor_id IS NOT NULL) ids;
 IF (intent_id,claim_room,actors) IS DISTINCT FROM(observed_intent,observed_room,observed_actors) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue cancellation account scope changed before fence';END IF;
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.cancel_matchmaking(player.id,p_entry,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_matchmaking_command(p_subject uuid,p_session uuid,p_kind text,p_request uuid,p_entry uuid,p_mode text,p_competitive boolean,p_alternatives jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;scope_name text;target uuid;terms jsonb;receipt app.command_receipt;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF p_kind='enqueue' AND p_entry IS NULL AND p_mode IN('open','collection') AND p_competitive IS NOT NULL AND jsonb_typeof(p_alternatives)='array' THEN
  scope_name:='queue_intent_v1';target:=player.id;terms:=jsonb_build_object('mode',p_mode,'competitive',p_competitive,'alternatives',p_alternatives);
  IF octet_length(terms::text)>262144 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('intent','invalid envelope'));END IF;
 ELSIF p_kind='cancel' AND p_entry IS NOT NULL AND p_entry<>'00000000-0000-0000-0000-000000000000'::uuid AND p_mode IS NULL AND p_competitive IS NULL AND p_alternatives IS NULL THEN
  scope_name:='queue_cancel';target:=p_entry;terms:=jsonb_build_object('expected',NULL,'room',NULL);
 ELSE RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
 END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope=scope_name AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM target::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,scope_name,p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
-- Own grouping adds only intent identity, never claim receipts or normalized source proofs.
CREATE FUNCTION app.read_my_queue(p_intent uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN(p_intent,p_after) OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'intentId',intent_receipt_id,'deckRevisionId',deck_revision_id,'mode',mode,'competitive',competitive,'alternativeIndex',alternative_index::text,'formatId',format_id,'state',state,'claimedRoomId',claimed_room_id,'reservedMatchId',reserved_match_id) body FROM app.queue_entry WHERE account_id=actor AND(p_intent IS NULL OR intent_receipt_id=p_intent) AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_queue_intent(p_intent uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;receipt app.command_receipt;alternatives jsonb;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_intent AND principal_key='human:'||actor::text AND scope='queue_intent_v1' AND target_ref=actor::text AND state='accepted' AND normalized_source->>'version'='queue-intent-v1';
 IF receipt.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('intent','unavailable'));END IF;
 -- Every public field is allowlisted; preserve both source array orders exactly.
 SELECT coalesce(jsonb_agg(jsonb_build_object('deckRevisionId',a.value->'deckRevisionId','choices',(SELECT coalesce(jsonb_agg(jsonb_build_object('profileId',c.value->'profileId','configurationKey',c.value->'configurationKey','maximumAiSeats',c.value->'maximumAiSeats','maximumSponsoredAiSeats',c.value->'maximumSponsoredAiSeats') ORDER BY c.ordinality),'[]'::jsonb) FROM jsonb_array_elements(a.value->'choices') WITH ORDINALITY c(value,ordinality))) ORDER BY a.ordinality),'[]'::jsonb) INTO alternatives FROM jsonb_array_elements(receipt.normalized_source->'alternatives') WITH ORDINALITY a(value,ordinality);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'intentId',receipt.id,'requestId',receipt.request_ref,'mode',receipt.normalized_source->'mode','competitive',receipt.normalized_source->'competitive','alternatives',alternatives));
END $$;
CREATE FUNCTION app.read_table_profiles(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'key',key,'minimumSeats',minimum_seats::text,'seatCapacity',seat_capacity::text,'hasMatchmaking',matchmaking_config IS NOT NULL) body FROM app.table_profile_revision WHERE admission_qualification_ref IS NOT NULL AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_table_profile_configurations(p_profile uuid,p_after integer,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;configuration jsonb;items jsonb;cursor_index text;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_profile IS NULL OR p_profile='00000000-0000-0000-0000-000000000000'::uuid OR p_after NOT BETWEEN 0 AND 511 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT matchmaking_config INTO configuration FROM app.table_profile_revision WHERE id=p_profile AND admission_qualification_ref IS NOT NULL AND matchmaking_config IS NOT NULL;
 IF configuration IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('profile','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY ordinal),'[]'::jsonb) INTO items FROM(
 SELECT ordinality-1 ordinal,jsonb_build_object('index',(ordinality-1)::text,'key',value->'key','formatId',value->'formatId','seatCount',value->'seatCount','structure',value->'structure') body FROM jsonb_array_elements(configuration->'configurations') WITH ORDINALITY source(value,ordinality) WHERE p_after IS NULL OR ordinality-1>p_after ORDER BY ordinality LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_index:=items->-1->>'index';END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'profileId',p_profile,'items',items,'nextIndex',cursor_index));
END $$;
REVOKE ALL ON FUNCTION app.enqueue_matchmaking_command(uuid,uuid,uuid,text,boolean,jsonb),app.cancel_matchmaking_command(uuid,uuid,uuid,uuid),app.read_matchmaking_command(uuid,uuid,text,uuid,uuid,text,boolean,jsonb),app.read_my_queue(uuid,uuid,integer),app.read_my_queue_intent(uuid),app.read_table_profiles(uuid,integer),app.read_table_profile_configurations(uuid,integer,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.enqueue_matchmaking_command(uuid,uuid,uuid,text,boolean,jsonb),app.cancel_matchmaking_command(uuid,uuid,uuid,uuid),app.read_matchmaking_command(uuid,uuid,text,uuid,uuid,text,boolean,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_my_queue(uuid,uuid,integer),app.read_my_queue_intent(uuid),app.read_table_profiles(uuid,integer),app.read_table_profile_configurations(uuid,integer,integer) TO authenticated;
COMMIT;
