BEGIN;
-- One canonical session-bound intent predicate for private retirement work.
CREATE FUNCTION app.accepted_deletion_intent(p_account uuid) RETURNS uuid LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT r.id FROM app.command_receipt r WHERE r.account_id=p_account AND r.principal_key='human:'||p_account::text AND r.scope='account_delete' AND r.target_ref=p_account::text AND r.state='accepted'
 AND substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"') IS NOT NULL
 AND r.result_ref=jsonb_build_object('version',1,'statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"'))::text
 AND r.payload_hash=app.auth_json_hash(jsonb_build_object('statusKeyHash',substring(r.result_ref FROM '"statusKeyHash": "([0-9a-f]{64})"')))
 AND r.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',p_account)) ORDER BY r.id LIMIT 1
$$;
CREATE OR REPLACE FUNCTION app.read_deletion_work(p_actor uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion work bounds';END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='deletion_pending') OR app.accepted_deletion_intent(p_actor) IS NULL THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='canonical deletion intent required';END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('kind',work.kind,'id',work.id) ORDER BY work.kind,work.id),'[]'::jsonb) INTO result FROM (
 SELECT * FROM (
 SELECT 'trade'::text kind,id FROM app.trade_offer WHERE state='offered' AND p_actor IN(sender_id,recipient_id)
 UNION ALL SELECT 'listing',id FROM app.market_listing WHERE state='listed' AND seller_id=p_actor
 UNION ALL SELECT 'prediction',id FROM app.prediction_order WHERE state='open' AND account_id=p_actor
 UNION ALL SELECT 'queue',id FROM app.queue_entry WHERE state IN('queued','claimed') AND account_id=p_actor
 UNION ALL SELECT 'invitation',id FROM app.invitation WHERE issuer_id=p_actor AND state='active'
 UNION ALL SELECT 'room',r.id FROM app.room r WHERE r.state IN('lobby','countdown') AND (r.host_id=p_actor OR EXISTS(SELECT 1 FROM app.room_slot s WHERE s.room_id=r.id AND s.account_id=p_actor AND s.retired_at IS NULL))
 UNION ALL SELECT 'ai_slot',s.id FROM app.room_slot s JOIN app.room r ON r.id=s.room_id WHERE s.sponsor_id=p_actor AND s.controller_kind='ai' AND s.retired_at IS NULL AND r.state IN('lobby','countdown') AND r.queue_claim_receipt_id IS NULL AND NOT EXISTS(SELECT 1 FROM app.ai_runner_binding b WHERE b.room_slot_id=s.id AND b.match_seat_id IS NOT NULL)
 UNION ALL SELECT 'ai_stop',b.id FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id JOIN app.room r ON r.id=s.room_id WHERE b.desired_state='running' AND p_actor IN(s.sponsor_id,r.host_id) AND (r.state='closed' OR b.match_seat_id IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM app.match m WHERE (m.room_id=r.id OR m.id IN(SELECT match_id FROM app.match_seat WHERE id=b.match_seat_id)) AND m.state NOT IN('completed','no_contest'))
 UNION ALL SELECT 'ante',a.id FROM app.ante_commitment a WHERE a.state='offered' AND EXISTS(SELECT 1 FROM app.match_seat s WHERE s.match_id=a.match_id AND p_actor IN(s.account_id,s.sponsor_id))
 ) eligible ORDER BY kind,id LIMIT p_limit) work;
 RETURN result;
END $$;
-- Own transaction: KEY SHARE freezes the pending lifecycle without conflicting
-- with the domain writer's UUID-sorted NO KEY UPDATE account fences.
CREATE OR REPLACE FUNCTION app.cancel_deletion_work(p_actor uuid,p_kind text,p_target uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE intent uuid; request uuid; basis text:=''; receipt app.command_receipt; action text; result jsonb; seat uuid; slot app.room_slot; lobby app.room; binding app.ai_runner_binding; actors uuid[]; stopped uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deletion cleanup requires read committed retry';END IF;
 IF p_kind IS NULL OR p_kind NOT IN('trade','listing','prediction','queue','invitation','room','ai_slot','ai_stop','ante') OR p_target IS NULL OR p_target='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid deletion cleanup identity';END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='deletion_pending' FOR KEY SHARE;
 intent:=app.accepted_deletion_intent(p_actor);
 IF NOT FOUND OR intent IS NULL THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='canonical pending deletion required';END IF;
 IF p_kind='room' THEN SELECT revision::text INTO basis FROM app.room WHERE id=p_target;
 ELSIF p_kind='ai_slot' THEN SELECT r.revision::text INTO basis FROM app.room_slot s JOIN app.room r ON r.id=s.room_id WHERE s.id=p_target;
 ELSIF p_kind='ai_stop' THEN SELECT binding_epoch::text INTO basis FROM app.ai_runner_binding WHERE id=p_target;
 ELSIF p_kind='ante' THEN SELECT terms_revision::text INTO basis FROM app.ante_commitment WHERE id=p_target;END IF;
 request:=substr(encode(sha256(convert_to('spellwake-deletion-cleanup-v2:'||intent::text||':'||p_kind||':'||p_target::text||':'||coalesce(basis,''),'UTF8')),'hex'),1,32)::uuid;
 receipt:=app.begin_account_command(p_actor,request,'deletion_cleanup_v2',p_target::text,jsonb_build_object('intentId',intent,'kind',p_kind,'basis',basis));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','deletion-cleanup-v2','intentId',intent,'kind',p_kind,'targetId',p_target,'basis',basis) WHERE id=receipt.id;
 IF p_kind='trade' THEN
 SELECT CASE WHEN sender_id=p_actor THEN 'cancel' ELSE 'reject' END INTO action FROM app.trade_offer WHERE id=p_target AND p_actor IN(sender_id,recipient_id);
 IF action IS NOT NULL THEN result:=app.respond_trade(p_actor,p_target,action,request);END IF;
 ELSIF p_kind='listing' THEN
 IF EXISTS(SELECT 1 FROM app.market_listing WHERE id=p_target AND seller_id=p_actor) THEN result:=app.cancel_listing(p_actor,p_target,request);END IF;
 ELSIF p_kind='prediction' THEN
 IF EXISTS(SELECT 1 FROM app.prediction_order WHERE id=p_target AND account_id=p_actor) THEN result:=app.cancel_prediction_order(p_actor,p_target,request);END IF;
 ELSIF p_kind='queue' THEN
 IF EXISTS(SELECT 1 FROM app.queue_entry WHERE id=p_target AND account_id=p_actor) THEN result:=app.cancel_matchmaking_claim(p_actor,p_target,request,NULL,NULL);END IF;
 ELSIF p_kind='invitation' THEN
 IF EXISTS(SELECT 1 FROM app.invitation WHERE id=p_target AND issuer_id=p_actor) THEN result:=app.revoke_room_invitation(p_actor,p_target,request);END IF;
 ELSIF p_kind='room' THEN
 result:=app.leave_room(p_actor,p_target,basis::numeric,request);
 ELSIF p_kind='ai_slot' THEN
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE id=p_target AND sponsor_id=p_actor) THEN result:=app.remove_ai_slot(p_actor,p_target,basis::numeric,request);END IF;
 ELSIF p_kind='ante' THEN
 SELECT id INTO seat FROM app.match_seat WHERE match_id=(SELECT match_id FROM app.ante_commitment WHERE id=p_target) AND p_actor IN(account_id,sponsor_id) ORDER BY id LIMIT 1;
 IF seat IS NOT NULL THEN result:=app.cancel_ante(p_target,seat,basis::numeric,request);END IF;
 ELSE
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_target;
 SELECT * INTO slot FROM app.room_slot WHERE id=binding.room_slot_id;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id;
 actors:=ARRAY[p_actor,slot.sponsor_id,lobby.host_id];
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=slot.room_id FOR NO KEY UPDATE;
 SELECT * INTO binding FROM app.ai_runner_binding WHERE id=p_target FOR NO KEY UPDATE;
 IF binding.room_slot_id IS DISTINCT FROM slot.id OR NOT EXISTS(SELECT 1 FROM app.room_slot s WHERE s.id=slot.id AND s.room_id=lobby.id AND s.sponsor_id IS NOT DISTINCT FROM slot.sponsor_id) OR NOT(lobby.host_id=ANY(actors)) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='cleanup controller parents changed before fence';END IF;
 PERFORM id FROM app.match WHERE room_id=lobby.id OR id IN(SELECT match_id FROM app.match_seat WHERE id=binding.match_seat_id) ORDER BY id FOR SHARE;
 IF p_actor IN(slot.sponsor_id,lobby.host_id) AND binding.binding_epoch::text=basis AND (lobby.state='closed' OR binding.match_seat_id IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM app.match m WHERE (m.room_id=lobby.id OR m.id IN(SELECT match_id FROM app.match_seat WHERE id=binding.match_seat_id)) AND m.state NOT IN('completed','no_contest')) THEN
 stopped:=app.request_ai_stop(binding.id,receipt.id);
 result:=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',binding.id,'stopId',stopped,'disposition','stop requested'));
 END IF;
 END IF;
 IF result IS NULL THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='deletion work unavailable';END IF;
 RETURN app.finish_account_command(receipt.id,result->>'status'='success',result->'data');
END $$;
REVOKE ALL ON FUNCTION app.accepted_deletion_intent(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
-- Preserve the existing dedicated trusted cleanup surface; no user-role grant.
REVOKE ALL ON FUNCTION app.read_deletion_work(uuid,integer),app.cancel_deletion_work(uuid,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_deletion_work(uuid,integer),app.cancel_deletion_work(uuid,text,uuid) TO service_role;
COMMIT;
