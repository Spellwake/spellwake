BEGIN;

-- Minimal immutable acknowledgments contain IDs/codes, never copied profile/provider secrets.
ALTER TABLE app.command_receipt ADD COLUMN response_payload jsonb;
ALTER TABLE app.command_receipt ADD CONSTRAINT command_response_envelope CHECK (
 response_payload IS NULL OR coalesce(
 jsonb_typeof(response_payload)='object'
 AND response_payload->>'status' IN ('success','fail')
 AND jsonb_typeof(response_payload->'data')='object'
 AND NOT jsonb_path_exists(response_payload,'$.data.* ? (@.type() != "string")'),false));
ALTER TABLE app.command_receipt ADD CONSTRAINT command_response_state CHECK (
 response_payload IS NULL OR coalesce((state='accepted' AND response_payload->>'status'='success')
 OR (state='rejected' AND response_payload->>'status'='fail'),false));
CREATE UNIQUE INDEX command_family_request ON app.command_receipt(principal_key,scope,request_ref);

CREATE FUNCTION app.guard_command_receipt() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF OLD.state<>'pending' OR (NEW.id,NEW.account_id,NEW.controller_grant_id,NEW.principal_key,NEW.request_ref,NEW.scope,NEW.target_ref,NEW.payload_hash)
 IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.controller_grant_id,OLD.principal_key,OLD.request_ref,OLD.scope,OLD.target_ref,OLD.payload_hash)
 OR (OLD.operation_id IS NOT NULL AND NEW.operation_id IS DISTINCT FROM OLD.operation_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='command source or terminal disposition cannot change';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER command_receipt_source BEFORE UPDATE ON app.command_receipt
 FOR EACH ROW EXECUTE FUNCTION app.guard_command_receipt();
CREATE TRIGGER command_receipt_history BEFORE DELETE ON app.command_receipt
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

-- Internal helpers are not granted to callers. Narrow service-only domain functions own them.
CREATE FUNCTION app.begin_account_command(p_actor uuid,p_request uuid,p_scope text,p_target text,p_payload jsonb)
RETURNS app.command_receipt LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE result app.command_receipt; fingerprint text;
BEGIN
 IF p_actor IS NULL OR p_request IS NULL OR p_scope IS NULL OR p_target IS NULL OR p_payload IS NULL
 OR length(p_scope)>64 OR length(p_target)>128 OR octet_length(p_payload::text)>65536 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid command identity or payload bounds';
 END IF;
 fingerprint:=encode(sha256(convert_to(p_payload::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(account_id,principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES(p_actor,'human:'||p_actor::text,p_request::text,p_scope,p_target,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO result;
 IF FOUND THEN RETURN result; END IF;
 SELECT * INTO STRICT result FROM app.command_receipt
 WHERE principal_key='human:'||p_actor::text AND scope=p_scope AND request_ref=p_request::text FOR UPDATE;
 IF result.target_ref<>p_target OR result.payload_hash<>fingerprint THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='request identity was used for different command terms';
 END IF;
 IF result.state='pending' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='command disposition is not yet terminal';
 END IF;
 RETURN result;
END $$;

CREATE FUNCTION app.finish_account_command(p_receipt uuid,p_accepted boolean,p_data jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE payload jsonb;
BEGIN
 payload:=jsonb_build_object('status',CASE WHEN p_accepted THEN 'success' ELSE 'fail' END,'data',p_data);
 UPDATE app.command_receipt SET state=CASE WHEN p_accepted THEN 'accepted' ELSE 'rejected' END,response_payload=payload
 WHERE id=p_receipt AND state='pending';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='command disposition changed'; END IF;
 RETURN payload;
END $$;

-- Pair actions use account locks before the shared contact lock. Deletion uses the same accounts.
CREATE FUNCTION app.lock_active_contact_accounts(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE count_active bigint;
BEGIN
 IF p_actor IS NULL OR p_other IS NULL OR p_actor=p_other THEN RETURN false; END IF;
 PERFORM id FROM app.account WHERE id IN (p_actor,p_other) ORDER BY id FOR UPDATE;
 SELECT count(*) INTO count_active FROM app.account WHERE id IN (p_actor,p_other) AND state='active';
 IF count_active<>2 THEN RETURN false; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('contact:'||least(p_actor,p_other)::text||':'||greatest(p_actor,p_other)::text,0));
 RETURN true;
END $$;

CREATE FUNCTION app.contact_blocked(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.block WHERE (blocker_id=p_actor AND blocked_id=p_other)
 OR (blocker_id=p_other AND blocked_id=p_actor))
$$;

CREATE FUNCTION app.contact_allowed(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT p_actor<>p_other AND EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active')
 AND EXISTS(SELECT 1 FROM app.account WHERE id=p_other AND state='active')
 AND NOT app.contact_blocked(p_actor,p_other)
 AND (EXISTS(SELECT 1 FROM app.friendship WHERE account_low_id=least(p_actor,p_other)
 AND account_high_id=greatest(p_actor,p_other) AND state='accepted')
 OR EXISTS(SELECT 1 FROM app.game_participation a JOIN app.game_participation b
 ON a.game_id=b.game_id AND a.account_id=p_actor AND b.account_id=p_other
 WHERE a.real_game AND b.real_game AND greatest(a.completed_at,b.completed_at)
 BETWEEN statement_timestamp()-interval '336 hours' AND statement_timestamp()))
$$;

CREATE FUNCTION app.change_friendship(p_actor uuid,p_other uuid,p_action text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; v_friendship app.friendship;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'social',p_other::text,
  jsonb_build_object('action',p_action,'other',p_other::text));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_action NOT IN ('request','accept','remove') OR p_action IS NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('action','invalid friend action'));
 END IF;
 IF NOT app.lock_active_contact_accounts(p_actor,p_other) OR app.contact_blocked(p_actor,p_other) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 SELECT * INTO v_friendship FROM app.friendship
 WHERE account_low_id=least(p_actor,p_other) AND account_high_id=greatest(p_actor,p_other) FOR UPDATE;
 IF p_action='request' THEN
  IF v_friendship.id IS NULL THEN
   INSERT INTO app.friendship(account_low_id,account_high_id,requested_by_id,state)
   VALUES(least(p_actor,p_other),greatest(p_actor,p_other),p_actor,'pending') RETURNING * INTO v_friendship;
  ELSIF v_friendship.state='removed' THEN
   UPDATE app.friendship SET requested_by_id=p_actor,state='pending',accepted_at=NULL WHERE id=v_friendship.id RETURNING * INTO v_friendship;
  END IF;
 ELSIF p_action='accept' THEN
  IF v_friendship.id IS NULL OR v_friendship.state<>'pending' OR v_friendship.requested_by_id=p_actor THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('friendship','no incoming request to accept'));
  END IF;
  UPDATE app.friendship SET state='accepted',accepted_at=clock_timestamp() WHERE id=v_friendship.id RETURNING * INTO v_friendship;
 ELSE
  IF v_friendship.id IS NULL THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('friendship','friendship unavailable'));
  END IF;
  UPDATE app.friendship SET state='removed',accepted_at=NULL WHERE id=v_friendship.id RETURNING * INTO v_friendship;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',v_friendship.id::text));
END $$;

CREATE FUNCTION app.change_block(p_actor uuid,p_other uuid,p_blocked boolean,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'social',p_other::text,
  jsonb_build_object('action','block','other',p_other::text,'blocked',p_blocked));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_blocked IS NULL OR NOT app.lock_active_contact_accounts(p_actor,p_other) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 IF p_blocked THEN
  INSERT INTO app.block(blocker_id,blocked_id,created_at) VALUES(p_actor,p_other,clock_timestamp()) ON CONFLICT DO NOTHING;
 ELSE
  DELETE FROM app.block WHERE blocker_id=p_actor AND blocked_id=p_other;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_other::text));
END $$;

GRANT EXECUTE ON FUNCTION app.change_friendship(uuid,uuid,text,uuid),app.change_block(uuid,uuid,boolean,uuid),app.contact_allowed(uuid,uuid) TO service_role;
GRANT SELECT ON app.friendship,app.block,app.notification TO authenticated;
CREATE POLICY participant_read ON app.friendship FOR SELECT TO authenticated USING
 (app.current_account_id() IN (account_low_id,account_high_id));
CREATE POLICY blocker_read ON app.block FOR SELECT TO authenticated USING (blocker_id=app.current_account_id());
CREATE POLICY recipient_read ON app.notification FOR SELECT TO authenticated USING (recipient_id=app.current_account_id());
GRANT SELECT ON app.friend_request TO authenticated;

COMMIT;
