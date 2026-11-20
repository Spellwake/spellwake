BEGIN;
-- This is the exact private fingerprint projection, not a public source payload.
CREATE FUNCTION app.human_ante_terms(p_kind text,p_target uuid,p_seat uuid,p_expected numeric,p_lines jsonb,p_hash text) RETURNS jsonb
LANGUAGE plpgsql SET search_path='' AS $$
DECLARE assets uuid[];
BEGIN
 IF p_target IS NULL OR p_seat IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_target,p_seat) OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RETURN NULL;END IF;
 IF p_kind='offer' AND p_hash IS NULL THEN
  assets:=app.bundle_asset_ids(p_lines);
  IF '00000000-0000-0000-0000-000000000000'::uuid=ANY(assets) THEN RETURN NULL;END IF;
  RETURN jsonb_build_object('match',p_target,'seat',p_seat,'basis',p_expected::text,'assets',p_lines,'aiCoins',NULL);
 ELSIF p_kind='accept' AND p_lines IS NULL AND p_hash IS NOT NULL AND p_hash ~ '^[0-9a-f]{64}$' THEN
  RETURN jsonb_build_object('commitment',p_target,'seat',p_seat,'basis',p_expected::text,'termsHash',p_hash);
 ELSIF p_kind='cancel' AND p_lines IS NULL AND p_hash IS NULL THEN
  RETURN jsonb_build_object('commitment',p_target,'seat',p_seat,'basis',p_expected::text);
 END IF;
 RETURN NULL;
EXCEPTION WHEN invalid_parameter_value OR numeric_value_out_of_range OR invalid_text_representation THEN RETURN NULL;
END $$;
CREATE FUNCTION app.human_ante_command(p_subject uuid,p_session uuid,p_request uuid,p_kind text,p_target uuid,p_expected numeric,p_lines jsonb,p_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;actor_id uuid;matched uuid;observed_match uuid;actors uuid[];observed_actors uuid[];seat uuid;terms jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='human ante requires read committed retry';END IF;
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';END IF;
 IF p_kind='offer' THEN matched:=p_target;ELSE SELECT match_id INTO matched FROM app.ante_commitment WHERE id=p_target;END IF;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM(SELECT actor_id a UNION SELECT account_id FROM app.match_seat WHERE match_id=matched AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=matched AND sponsor_id IS NOT NULL) scope;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified account changed';END IF;
 IF p_kind='offer' THEN observed_match:=p_target;ELSE SELECT match_id INTO observed_match FROM app.ante_commitment WHERE id=p_target;END IF;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO observed_actors FROM(SELECT actor_id a UNION SELECT account_id FROM app.match_seat WHERE match_id=observed_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=observed_match AND sponsor_id IS NOT NULL) scope;
 IF (matched,actors) IS DISTINCT FROM(observed_match,observed_actors) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante participant scope changed before account fence';END IF;
 SELECT s.id INTO seat FROM app.match_seat s JOIN app.match m ON m.id=s.match_id WHERE s.match_id=matched AND s.account_id=player.id AND s.controller_kind='human' AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL;
 IF seat IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 terms:=app.human_ante_terms(p_kind,p_target,seat,p_expected,p_lines,p_hash);
 IF terms IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 IF p_kind='offer' THEN RETURN app.set_ante_stake(matched,seat,p_expected,p_lines,NULL,p_request);
 ELSIF p_kind='accept' THEN RETURN app.accept_ante_terms(p_target,seat,p_expected,p_hash,p_request);
 ELSE RETURN app.cancel_ante(p_target,seat,p_expected,p_request);END IF;
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM IN('ante offer source terms changed','ante acceptance source terms changed','ante cancellation terms changed') THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));
 ELSIF SQLERRM='ante bundle is unavailable or held elsewhere' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('assets','ante bundle unavailable'));END IF;RAISE;
END $$;
CREATE FUNCTION app.read_human_ante_command(p_subject uuid,p_session uuid,p_request uuid,p_kind text,p_target uuid,p_expected numeric,p_lines jsonb,p_hash text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;matched uuid;seat uuid;terms jsonb;scope_name text;receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='human ante requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_kind='offer' THEN matched:=p_target;ELSE SELECT match_id INTO matched FROM app.ante_commitment WHERE id=p_target;END IF;
 SELECT s.id INTO seat FROM app.match_seat s JOIN app.match m ON m.id=s.match_id WHERE s.match_id=matched AND s.account_id=player.id AND s.controller_kind='human' AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL;
 IF seat IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 terms:=app.human_ante_terms(p_kind,p_target,seat,p_expected,p_lines,p_hash);
 IF terms IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 scope_name:=CASE p_kind WHEN 'offer' THEN 'ante_offer_v1' WHEN 'accept' THEN 'ante_accept_v1' ELSE 'ante_cancel_v1' END;
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='ante-seat:'||seat::text AND scope=scope_name AND request_ref=p_request::text;
 IF FOUND AND(receipt.target_ref IS DISTINCT FROM p_target::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms)) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('state',coalesce(receipt.state,'unknown'),'response',receipt.response_payload));
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
CREATE FUNCTION app.read_match_ante(p_match uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;c app.ante_commitment;agreement jsonb;stakes jsonb;allocations jsonb;own_seat uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT id INTO own_seat FROM app.match_seat WHERE match_id=p_match AND account_id=actor AND controller_kind='human';
 SELECT * INTO c FROM app.ante_commitment WHERE match_id=p_match;
 IF c.id IS NOT NULL THEN
  -- Accepted terms are immutable display sources; do not rehash their redaction.
  SELECT coalesce(jsonb_agg(jsonb_build_object('seatId',entry->'seatId','fundingKind',entry->'fundingKind','aiCoins',entry->'aiCoins','entries',entry->'entries','value',CASE WHEN s.value_numerator IS NOT NULL THEN jsonb_build_object('numerator',s.value_numerator::text,'denominator',s.value_denominator::text) END) ORDER BY entry->>'seatId'),'[]'::jsonb) INTO stakes FROM jsonb_array_elements(c.terms->'stakes') entry LEFT JOIN app.ante_stake s ON s.commitment_id=c.id AND s.match_seat_id=(entry->>'seatId')::uuid;
  IF c.settlement IS NOT NULL THEN
   SELECT coalesce(jsonb_agg(jsonb_build_object('sourceSeatId',s.match_seat_id,'assetId',a.value->'sourceAssetId','recipientSeatId',a.value->'recipientSeatId','quantity',a.value->'quantity','classification',a.value->'classification') ORDER BY a.ordinal),'[]'::jsonb) INTO allocations FROM jsonb_array_elements(c.settlement->'allocations') WITH ORDINALITY a(value,ordinal) JOIN app.ante_stake s ON s.id=(a.value->>'sourceStakeId')::uuid AND s.commitment_id=c.id;
  END IF;
  agreement:=jsonb_build_object('id',c.id,'state',c.state,'termsRevision',c.terms_revision::text,'termsHash',c.terms_hash,'acceptedSeatIds',c.accepted_seat_ids,'acceptedAt',c.accepted_at,'startDay',c.start_day_utc,'stakes',stakes,'settlementResultId',c.settlement_result_id,'allocations',allocations);
 END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'ownSeatId',own_seat,'agreement',agreement));
END $$;
REVOKE ALL ON FUNCTION app.human_ante_terms(text,uuid,uuid,numeric,jsonb,text),app.human_ante_command(uuid,uuid,uuid,text,uuid,numeric,jsonb,text),app.read_human_ante_command(uuid,uuid,uuid,text,uuid,numeric,jsonb,text),app.read_match_ante(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.human_ante_command(uuid,uuid,uuid,text,uuid,numeric,jsonb,text),app.read_human_ante_command(uuid,uuid,uuid,text,uuid,numeric,jsonb,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_match_ante(uuid) TO authenticated;
COMMIT;
