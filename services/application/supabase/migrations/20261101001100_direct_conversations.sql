BEGIN;

-- The normalized DM pair is a uniqueness key; membership rows remain the read authority.
ALTER TABLE app.conversation ADD COLUMN dm_low_account_id uuid REFERENCES app.account(id) ON DELETE RESTRICT;
ALTER TABLE app.conversation ADD COLUMN dm_high_account_id uuid REFERENCES app.account(id) ON DELETE RESTRICT;
ALTER TABLE app.conversation ADD CONSTRAINT conversation_kind_sources CHECK (
 (kind='dm' AND match_id IS NULL AND dm_low_account_id IS NOT NULL AND dm_high_account_id IS NOT NULL AND dm_low_account_id<dm_high_account_id)
 OR (kind='game' AND match_id IS NOT NULL AND dm_low_account_id IS NULL AND dm_high_account_id IS NULL));
CREATE UNIQUE INDEX conversation_dm_pair ON app.conversation(dm_low_account_id,dm_high_account_id) WHERE kind='dm';
ALTER TABLE app.message ADD CONSTRAINT bounded_message_body CHECK (length(body) BETWEEN 1 AND 4000 AND length(btrim(body))>0);

CREATE FUNCTION app.guard_conversation_identity() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF (NEW.id,NEW.kind,NEW.match_id,NEW.dm_low_account_id,NEW.dm_high_account_id,NEW.created_at)
 IS DISTINCT FROM (OLD.id,OLD.kind,OLD.match_id,OLD.dm_low_account_id,OLD.dm_high_account_id,OLD.created_at) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='conversation identity cannot change';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER conversation_identity BEFORE UPDATE ON app.conversation
 FOR EACH ROW EXECUTE FUNCTION app.guard_conversation_identity();
CREATE TRIGGER conversation_membership_history BEFORE UPDATE OR DELETE ON app.conversation_member
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

CREATE FUNCTION app.check_dm_membership() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE v_conversation_id uuid; conversation app.conversation; member_count bigint; wrong_count bigint;
BEGIN
 IF TG_TABLE_NAME='conversation' THEN v_conversation_id:=NEW.id; ELSE v_conversation_id:=NEW.conversation_id; END IF;
 SELECT * INTO conversation FROM app.conversation WHERE id=v_conversation_id;
 IF conversation.kind='dm' THEN
  SELECT count(*),count(*) FILTER(WHERE account_id NOT IN (conversation.dm_low_account_id,conversation.dm_high_account_id))
  INTO member_count,wrong_count FROM app.conversation_member m WHERE m.conversation_id=v_conversation_id;
  IF member_count<>2 OR wrong_count<>0 THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='direct conversation requires exactly its two participants';
  END IF;
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER dm_conversation_members AFTER INSERT OR UPDATE ON app.conversation
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_dm_membership();
CREATE CONSTRAINT TRIGGER dm_member_pair AFTER INSERT ON app.conversation_member
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_dm_membership();

CREATE FUNCTION app.open_direct_conversation(p_actor uuid,p_other uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; conversation_id uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'social',p_other::text,
  jsonb_build_object('action','open_dm','other',p_other::text));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF NOT app.lock_active_contact_accounts(p_actor,p_other) OR NOT app.contact_allowed(p_actor,p_other) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 SELECT id INTO conversation_id FROM app.conversation
 WHERE kind='dm' AND dm_low_account_id=least(p_actor,p_other) AND dm_high_account_id=greatest(p_actor,p_other);
 IF conversation_id IS NULL THEN
  INSERT INTO app.conversation(kind,dm_low_account_id,dm_high_account_id,created_at)
  VALUES('dm',least(p_actor,p_other),greatest(p_actor,p_other),clock_timestamp()) RETURNING id INTO conversation_id;
  INSERT INTO app.conversation_member(conversation_id,account_id) VALUES(conversation_id,p_actor),(conversation_id,p_other);
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',conversation_id::text));
END $$;

CREATE FUNCTION app.send_direct_message(p_actor uuid,p_conversation uuid,p_body text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; conversation app.conversation; other_account uuid; message_id uuid;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'chat',p_conversation::text,
  jsonb_build_object('action','send_dm','body',p_body));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_body IS NULL OR length(p_body) NOT BETWEEN 1 AND 4000 OR p_body !~ '[^[:space:]]' THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('body','message must contain 1 to 4000 characters'));
 END IF;
 SELECT * INTO conversation FROM app.conversation WHERE id=p_conversation AND kind='dm';
 IF conversation.id IS NULL OR p_actor NOT IN (conversation.dm_low_account_id,conversation.dm_high_account_id) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('conversation','conversation unavailable'));
 END IF;
 other_account:=CASE WHEN conversation.dm_low_account_id=p_actor THEN conversation.dm_high_account_id ELSE conversation.dm_low_account_id END;
 IF NOT app.lock_active_contact_accounts(p_actor,other_account) OR NOT app.contact_allowed(p_actor,other_account)
 OR NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=p_conversation AND account_id=p_actor) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 INSERT INTO app.message(conversation_id,sender_id,send_request_ref,body,sent_at)
 VALUES(p_conversation,p_actor,p_request::text,p_body,clock_timestamp()) RETURNING id INTO message_id;
 INSERT INTO app.notification(recipient_id,source_kind,source_ref,body)
 VALUES(other_account,'dm_message',message_id::text,'New direct message') ON CONFLICT DO NOTHING;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',message_id::text));
END $$;

CREATE FUNCTION app.can_read_conversation(p_conversation uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=p_conversation AND account_id=app.current_account_id())
$$;
GRANT EXECUTE ON FUNCTION app.open_direct_conversation(uuid,uuid,uuid),app.send_direct_message(uuid,uuid,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.can_read_conversation(uuid) TO authenticated;
GRANT SELECT ON app.conversation,app.conversation_member,app.message TO authenticated;
CREATE POLICY member_read ON app.conversation FOR SELECT TO authenticated USING (app.can_read_conversation(id));
CREATE POLICY own_membership_read ON app.conversation_member FOR SELECT TO authenticated USING (account_id=app.current_account_id());
CREATE POLICY participant_read ON app.message FOR SELECT TO authenticated USING (app.can_read_conversation(conversation_id));

COMMIT;
