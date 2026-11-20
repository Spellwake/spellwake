BEGIN;
-- Crop numbers have the same finite float8 semantics as the existing stored columns.
-- Pin shortest-precise output independently of a caller's session formatting settings.
CREATE FUNCTION app.normalize_profile_art(p_value jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' SET extra_float_digits=3 AS $$
DECLARE x double precision;y double precision;s double precision;printing uuid;
BEGIN
 IF p_value IS NULL OR p_value='null'::jsonb THEN RETURN 'null'::jsonb;END IF;
 IF jsonb_typeof(p_value) IS DISTINCT FROM 'object' OR octet_length(p_value::text)>4096
 OR(SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_value) k) IS DISTINCT FROM ARRAY['cropX','cropY','printingId','scale']::text[]
 OR jsonb_typeof(p_value->'printingId') IS DISTINCT FROM 'string' OR(p_value->>'printingId')!~'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(p_value->'cropX') IS DISTINCT FROM 'number' OR jsonb_typeof(p_value->'cropY') IS DISTINCT FROM 'number' OR jsonb_typeof(p_value->'scale') IS DISTINCT FROM 'number' THEN RETURN NULL;END IF;
 printing:=(p_value->>'printingId')::uuid;x:=(p_value->>'cropX')::double precision;y:=(p_value->>'cropY')::double precision;s:=(p_value->>'scale')::double precision;
 IF printing='00000000-0000-0000-0000-000000000000'::uuid OR NOT(x BETWEEN 0 AND 1) OR NOT(y BETWEEN 0 AND 1) OR NOT(s>0 AND s<'Infinity'::double precision) THEN RETURN NULL;END IF;
 RETURN jsonb_build_object('printingId',printing,'cropX',x,'cropY',y,'scale',s);
EXCEPTION WHEN numeric_value_out_of_range OR invalid_text_representation THEN RETURN NULL;
END $$;
CREATE FUNCTION app.profile_art_snapshot(p_profile app.profile) RETURNS jsonb
LANGUAGE sql IMMUTABLE SET search_path='' SET extra_float_digits=3 AS $$
 SELECT CASE WHEN p_profile.printing_id IS NULL THEN 'null'::jsonb ELSE app.normalize_profile_art(jsonb_build_object('printingId',p_profile.printing_id,'cropX',p_profile.crop_x,'cropY',p_profile.crop_y,'scale',p_profile.crop_scale)) END
$$;
CREATE FUNCTION app.change_profile_art_command(p_subject uuid,p_session uuid,p_request uuid,p_expected jsonb,p_art jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;current_profile app.profile;receipt app.command_receipt;expected jsonb;art jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='profile art command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 expected:=app.normalize_profile_art(p_expected);art:=app.normalize_profile_art(p_art);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR expected IS NULL OR art IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 receipt:=app.begin_account_command(player.id,p_request,'profile_art',player.id::text,jsonb_build_object('expected',expected,'cardArtProfile',art));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 IF player.state<>'active' THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));END IF;
 SELECT * INTO current_profile FROM app.profile WHERE account_id=player.id FOR NO KEY UPDATE;
 IF current_profile.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable'));END IF;
 IF app.profile_art_snapshot(current_profile) IS DISTINCT FROM expected THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','profile art changed; reload before saving'));END IF;
 -- References are private trusted metadata. No URL is synthesized or delivered.
 IF art<>'null'::jsonb AND NOT EXISTS(SELECT 1 FROM app.printing p WHERE p.id=(art->>'printingId')::uuid AND app.catalogue_source_visible(p.catalogue_receipt_id) AND p.image_ref IS NOT NULL AND length(btrim(p.image_ref))>0) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('art','catalogue art unavailable'));END IF;
 UPDATE app.profile SET printing_id=(art->>'printingId')::uuid,crop_x=(art->>'cropX')::double precision,crop_y=(art->>'cropY')::double precision,crop_scale=(art->>'scale')::double precision WHERE id=current_profile.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',player.id));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_profile_art_command(p_subject uuid,p_session uuid,p_request uuid,p_expected jsonb,p_art jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;receipt app.command_receipt;expected jsonb;art jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='profile art command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 expected:=app.normalize_profile_art(p_expected);art:=app.normalize_profile_art(p_art);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR expected IS NULL OR art IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text AND scope='profile_art' AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM player.id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('expected',expected,'cardArtProfile',art))) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,'profile_art',p_request));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_my_profile_art() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;current_profile app.profile;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 SELECT * INTO current_profile FROM app.profile WHERE account_id=actor;
 IF current_profile.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'cardArtProfile',app.profile_art_snapshot(current_profile),'supporter',app.active_supporter_tier(actor) IS NOT NULL));
END $$;
CREATE FUNCTION app.read_public_profile_art(p_player uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;player app.account;current_profile app.profile;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_player IS NULL OR p_player='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('player','unavailable'));END IF;
 SELECT * INTO player FROM app.account a WHERE a.id=p_player AND(a.id=actor OR a.state='active' OR EXISTS(SELECT 1 FROM app.social_thread_candidates(actor,statement_timestamp(),p_player)));
 IF player.id IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('player','unavailable'));END IF;
 IF player.state='active' THEN SELECT * INTO current_profile FROM app.profile WHERE account_id=player.id;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('viewerAccountId',actor,'person',jsonb_build_object('id',player.id,'username',CASE WHEN player.state='deleted' THEN 'Deleted player' ELSE player.username END,'isDeleted',player.state='deleted'),'cardArtProfile',app.profile_art_snapshot(current_profile),'supporter',app.active_supporter_tier(player.id) IS NOT NULL));
END $$;
REVOKE ALL ON FUNCTION app.normalize_profile_art(jsonb),app.profile_art_snapshot(app.profile),app.change_profile_art_command(uuid,uuid,uuid,jsonb,jsonb),app.read_profile_art_command(uuid,uuid,uuid,jsonb,jsonb),app.read_my_profile_art(),app.read_public_profile_art(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.change_profile_art_command(uuid,uuid,uuid,jsonb,jsonb),app.read_profile_art_command(uuid,uuid,uuid,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_my_profile_art(),app.read_public_profile_art(uuid) TO authenticated;
COMMIT;
