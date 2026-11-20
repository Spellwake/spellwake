BEGIN;
-- Raw slot grants deliberately exclude private deck references and retirement history.
-- This caller-bound projection is the only added elevation and never accepts an actor ID.
CREATE FUNCTION app.read_current_room_slots(p_room uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_room IS NULL OR p_room='00000000-0000-0000-0000-000000000000'::uuid OR p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF NOT app.room_visible(p_room) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('room','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT s.id,jsonb_build_object('id',s.id,'accountId',s.account_id,'seatIndex',s.seat_index::text,'controllerKind',s.controller_kind,'ready',s.ready,'deckRevisionId',CASE WHEN s.controller_kind='human' AND s.account_id=actor THEN s.deck_revision_id END) body
 FROM app.room_slot s WHERE s.room_id=p_room AND s.retired_at IS NULL AND(p_after IS NULL OR s.id>p_after) ORDER BY s.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_visible_rooms(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT r.id,jsonb_build_object('id',r.id,'hostId',r.host_id,'formatId',r.format_id,'profileId',r.profile_id,'mode',r.mode,'competitive',r.competitive,'visibility',r.visibility,'state',r.state,'revision',r.revision::text,'structure',r.structure,'configurationId',r.configuration_id,'targetSeatCount',r.target_seat_count::text,'countdownAt',r.countdown_at) body
 FROM app.room r WHERE p_after IS NULL OR r.id>p_after ORDER BY r.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_visible_room(p_room uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;room_data jsonb;slots jsonb;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_room IS NULL OR p_room='00000000-0000-0000-0000-000000000000'::uuid OR p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT jsonb_build_object('id',r.id,'hostId',r.host_id,'formatId',r.format_id,'profileId',r.profile_id,'mode',r.mode,'competitive',r.competitive,'visibility',r.visibility,'state',r.state,'revision',r.revision::text,'structure',r.structure,'configurationId',r.configuration_id,'targetSeatCount',r.target_seat_count::text,'countdownAt',r.countdown_at) INTO room_data FROM app.room r WHERE r.id=p_room;
 IF room_data IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('room','unavailable'));END IF;
 slots:=app.read_current_room_slots(p_room,p_after,p_limit);
 IF slots->>'status'<>'success' THEN RETURN slots;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'room',room_data,'slots',slots->'data'));
END $$;
REVOKE ALL ON FUNCTION app.read_current_room_slots(uuid,uuid,integer),app.read_visible_rooms(uuid,integer),app.read_visible_room(uuid,uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_current_room_slots(uuid,uuid,integer),app.read_visible_rooms(uuid,integer),app.read_visible_room(uuid,uuid,integer) TO authenticated;
COMMIT;
