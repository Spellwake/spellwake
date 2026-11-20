BEGIN;
ALTER TABLE app.queue_entry ADD CONSTRAINT queue_state_source CHECK(state IN ('queued','canceled','reserved') AND ((state='reserved')=(reserved_match_id IS NOT NULL)));
ALTER TABLE app.invitation ADD CONSTRAINT invitation_state_source CHECK(state IN ('active','canceled') AND token_hash ~ '^[0-9a-f]{64}$');
CREATE FUNCTION app.issue_room_invitation(p_actor uuid,p_room uuid,p_expected numeric,p_token_hash text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; invitation_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_invite',coalesce(p_room::text,''),jsonb_build_object('basis',p_expected,'tokenHash',p_token_hash));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.host_id<>p_actor OR lobby.state NOT IN ('lobby','countdown') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_expected<>lobby.revision THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('revision','conflict')); END IF;
 IF p_token_hash IS NULL OR p_token_hash !~ '^[0-9a-f]{64}$' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('token','invalid hash')); END IF;
 INSERT INTO app.invitation(room_id,issuer_id,token_hash,state) VALUES(p_room,p_actor,p_token_hash,'active') RETURNING id INTO invitation_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',invitation_id));
END $$;
CREATE FUNCTION app.revoke_room_invitation(p_actor uuid,p_invitation uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; parent_room uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_invite_revoke',coalesce(p_invitation::text,''),'{}'::jsonb);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending') FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT room_id INTO parent_room FROM app.invitation WHERE id=p_invitation AND issuer_id=p_actor;
 PERFORM id FROM app.room WHERE id=parent_room AND host_id=p_actor FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('invitation','unavailable')); END IF;
 UPDATE app.invitation SET state='canceled' WHERE id=p_invitation;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_invitation));
END $$;
CREATE FUNCTION app.join_room(p_actor uuid,p_room uuid,p_token_hash text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; original_host uuid; profile app.table_profile_revision; slot app.room_slot; chosen bigint;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_join',coalesce(p_room::text,''),jsonb_build_object('tokenHash',p_token_hash));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT host_id INTO original_host FROM app.room WHERE id=p_room;
 PERFORM id FROM app.account WHERE id IN (p_actor,original_host) ORDER BY id FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=original_host AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF lobby.host_id<>original_host THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room host changed before account fence'; END IF;
 SELECT * INTO slot FROM app.room_slot WHERE room_id=p_room AND account_id=p_actor AND controller_kind='human';
 IF slot.id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id)); END IF;
 IF lobby.visibility<>'public' AND NOT EXISTS(SELECT 1 FROM app.invitation WHERE room_id=p_room AND token_hash=p_token_hash AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('invitation','unavailable')); END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 IF profile.admission_qualification_ref IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unqualified')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE room_id=p_room AND controller_kind='human' AND account_id IS NULL ORDER BY seat_index LIMIT 1 FOR NO KEY UPDATE;
 IF slot.id IS NULL THEN
  SELECT min(candidate) INTO chosen FROM (SELECT 0::bigint candidate UNION SELECT seat_index+1 FROM app.room_slot WHERE room_id=p_room AND seat_index<profile.seat_capacity-1) candidates
   WHERE candidate<profile.seat_capacity AND NOT EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND seat_index=candidate);
  IF chosen IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('capacity','full')); END IF;
  INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES(p_room,p_actor,chosen,'human',false) RETURNING * INTO slot;
 ELSE UPDATE app.room_slot SET account_id=p_actor,sponsor_id=NULL,deck_revision_id=NULL,ready=false WHERE id=slot.id;
 END IF;
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id));
END $$;

CREATE FUNCTION app.enqueue_matchmaking(p_actor uuid,p_revision uuid,p_competitive boolean,p_preferences_ref text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; library app.deck; entry_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'queue_enter',p_actor::text,jsonb_build_object('revision',p_revision,'competitive',p_competitive,'preferencesRef',p_preferences_ref));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT d.* INTO library FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=p_revision AND r.sealed_at IS NOT NULL AND d.source_kind='account' AND d.account_id=p_actor FOR SHARE OF r,d;
 IF library.id IS NULL OR p_competitive IS NULL OR (p_competitive AND library.mode<>'collection') OR p_preferences_ref IS NULL OR length(btrim(p_preferences_ref)) NOT BETWEEN 1 AND 2048 THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','invalid deck or preferences')); END IF;
 INSERT INTO app.queue_entry(account_id,deck_revision_id,mode,competitive,preferences_ref,state) VALUES(p_actor,p_revision,library.mode,p_competitive,p_preferences_ref,'queued') RETURNING id INTO entry_id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',entry_id));
END $$;
CREATE FUNCTION app.cancel_matchmaking(p_actor uuid,p_entry uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; entry app.queue_entry;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'queue_cancel',coalesce(p_entry::text,''),'{}'::jsonb);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending') FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO entry FROM app.queue_entry WHERE id=p_entry AND account_id=p_actor FOR NO KEY UPDATE;
 IF entry.id IS NULL OR entry.state NOT IN ('queued','canceled') OR entry.reserved_match_id IS NOT NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','unavailable or reserved')); END IF;
 UPDATE app.queue_entry SET state='canceled' WHERE id=p_entry;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_entry));
END $$;
CREATE FUNCTION app.guard_queue_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'queued' OR NEW.reserved_match_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue entry requires original unreserved source'; END IF;
  RETURN NEW;
 END IF;
 IF TG_OP='DELETE' OR (NEW.id,NEW.account_id,NEW.deck_revision_id,NEW.mode,NEW.competitive,NEW.preferences_ref) IS DISTINCT FROM
  (OLD.id,OLD.account_id,OLD.deck_revision_id,OLD.mode,OLD.competitive,OLD.preferences_ref)
 OR (OLD.state<>'queued' AND NEW IS DISTINCT FROM OLD)
 OR NEW.state NOT IN ('queued','canceled','reserved')
 OR ((NEW.state='reserved')<>(NEW.reserved_match_id IS NOT NULL)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue source identity and terminal reservation are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER queue_history BEFORE INSERT OR UPDATE OR DELETE ON app.queue_entry FOR EACH ROW EXECUTE FUNCTION app.guard_queue_history();
CREATE FUNCTION app.guard_invitation_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.state<>'active' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invitation requires original active source'; END IF;
  RETURN NEW;
 END IF;
 IF TG_OP='DELETE' OR (NEW.id,NEW.room_id,NEW.issuer_id,NEW.token_hash) IS DISTINCT FROM (OLD.id,OLD.room_id,OLD.issuer_id,OLD.token_hash)
 OR (NEW.state IS DISTINCT FROM OLD.state AND NOT(OLD.state='active' AND NEW.state='canceled')) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invitation identity and revocation history are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER invitation_history BEFORE INSERT OR UPDATE OR DELETE ON app.invitation FOR EACH ROW EXECUTE FUNCTION app.guard_invitation_history();
CREATE FUNCTION app.guard_invitation_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent_host uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='invitation source requires read committed retry'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.issuer_id AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invitation issuer is unavailable'; END IF;
 SELECT host_id INTO parent_host FROM app.room WHERE id=NEW.room_id FOR SHARE;
 IF parent_host IS NULL OR parent_host<>NEW.issuer_id THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invitation must bind its room host'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER invitation_source AFTER INSERT ON app.invitation
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_invitation_source();
CREATE FUNCTION app.guard_room_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.host_id) IS DISTINCT FROM (OLD.id,OLD.host_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room ownership is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER room_identity BEFORE UPDATE ON app.room FOR EACH ROW EXECUTE FUNCTION app.guard_room_identity();
REVOKE ALL ON FUNCTION app.guard_invitation_source(),app.guard_room_identity(),app.issue_room_invitation(uuid,uuid,numeric,text,uuid),app.revoke_room_invitation(uuid,uuid,uuid),app.join_room(uuid,uuid,text,uuid),app.enqueue_matchmaking(uuid,uuid,boolean,text,uuid),app.cancel_matchmaking(uuid,uuid,uuid),app.guard_queue_history(),app.guard_invitation_history()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
