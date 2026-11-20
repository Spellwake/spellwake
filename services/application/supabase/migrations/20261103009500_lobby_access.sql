BEGIN;

-- One caller-bound predicate avoids recursive room/slot policies. No arbitrary actor argument.
CREATE FUNCTION app.room_visible(p_room uuid) RETURNS boolean
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();
BEGIN
 IF actor IS NULL THEN RETURN false; END IF;
 RETURN EXISTS(SELECT 1 FROM app.room r WHERE r.id=p_room AND
  (r.host_id=actor OR (r.visibility='public' AND r.state IN ('lobby','countdown')) OR EXISTS(
   SELECT 1 FROM app.room_slot s WHERE s.room_id=r.id AND s.account_id=actor AND s.controller_kind='human')));
END $$;
REVOKE ALL ON FUNCTION app.room_visible(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.room_visible(uuid) TO authenticated;

CREATE POLICY room_visible ON app.room FOR SELECT TO authenticated USING (app.room_visible(id));
CREATE POLICY room_slot_visible ON app.room_slot FOR SELECT TO authenticated USING (app.room_visible(room_id));
GRANT SELECT ON app.room TO authenticated;
-- Private deck references, sponsor attribution and potentially hidden teams are not raw public rows.
GRANT SELECT(id,room_id,account_id,seat_index,controller_kind,ready) ON app.room_slot TO authenticated;

CREATE FUNCTION app.create_room_command(p_subject uuid,p_session uuid,p_request uuid,p_format uuid,
 p_profile uuid,p_mode text,p_competitive boolean,p_visibility text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 RETURN app.create_room(player.id,p_format,p_profile,p_mode,p_competitive,p_visibility,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.select_room_deck_command(p_subject uuid,p_session uuid,p_request uuid,
 p_room uuid,p_expected numeric,p_revision uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 RETURN app.select_room_deck(player.id,p_room,p_expected,p_revision,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.leave_room_command(p_subject uuid,p_session uuid,p_request uuid,
 p_room uuid,p_expected numeric) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 -- The shared live-session lock permits pending accounts;094 allows their release only.
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 RETURN app.leave_room(player.id,p_room,p_expected,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

REVOKE ALL ON FUNCTION app.create_room_command(uuid,uuid,uuid,uuid,uuid,text,boolean,text),
 app.select_room_deck_command(uuid,uuid,uuid,uuid,numeric,uuid),app.leave_room_command(uuid,uuid,uuid,uuid,numeric)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.create_room_command(uuid,uuid,uuid,uuid,uuid,text,boolean,text),
 app.select_room_deck_command(uuid,uuid,uuid,uuid,numeric,uuid),app.leave_room_command(uuid,uuid,uuid,uuid,numeric)
 TO service_role;

COMMIT;
