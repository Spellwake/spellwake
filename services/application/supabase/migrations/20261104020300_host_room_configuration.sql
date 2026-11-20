BEGIN;
CREATE FUNCTION app.configure_room_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric,p_profile uuid,p_structure text,p_teams jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 RETURN app.configure_room(player.id,p_room,p_expected,p_profile,p_structure,p_teams,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN invalid_parameter_value THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_room_configuration_command(p_subject uuid,p_session uuid,p_request uuid,p_room uuid,p_expected numeric,p_profile uuid,p_structure text,p_teams jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;terms jsonb;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_room IS NULL OR p_room='00000000-0000-0000-0000-000000000000'::uuid OR p_profile IS NULL OR p_profile='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_structure IS NULL OR p_structure NOT IN('bo1','bo3') OR jsonb_typeof(p_teams) IS DISTINCT FROM 'array' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 terms:=jsonb_build_object('basis',p_expected,'profile',p_profile,'structure',p_structure,'teams',p_teams);
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='room_config' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_room::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'room_config',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
-- Hosts retain configuration metadata for their own started/closed rooms and while deletion waits.
-- This does not authorize configuring a room outside the canonical active lobby states.
CREATE FUNCTION app.read_host_room_configuration(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;lobby app.room;teams jsonb;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room AND host_id=actor;
 IF lobby.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('room','unavailable'));END IF;
 IF (SELECT count(*) FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL)>6 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('configuration','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('slotId',id,'teamRef',team_ref) ORDER BY id),'[]'::jsonb) INTO teams FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'roomId',lobby.id,'revision',lobby.revision::text,'profileId',lobby.profile_id,'structure',lobby.structure,'teams',teams));
END $$;
REVOKE ALL ON FUNCTION app.configure_room_command(uuid,uuid,uuid,uuid,numeric,uuid,text,jsonb),app.read_room_configuration_command(uuid,uuid,uuid,uuid,numeric,uuid,text,jsonb),app.read_host_room_configuration(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.configure_room_command(uuid,uuid,uuid,uuid,numeric,uuid,text,jsonb),app.read_room_configuration_command(uuid,uuid,uuid,uuid,numeric,uuid,text,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_host_room_configuration(uuid) TO authenticated;
COMMIT;
