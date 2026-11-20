BEGIN;

CREATE FUNCTION app.check_message_sender() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE conversation app.conversation; other_account uuid;
BEGIN
 SELECT * INTO STRICT conversation FROM app.conversation WHERE id=NEW.conversation_id;
 IF NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=NEW.conversation_id AND account_id=NEW.sender_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='sender is not a conversation participant';
 END IF;
 IF conversation.kind='dm' THEN
  other_account:=CASE WHEN conversation.dm_low_account_id=NEW.sender_id THEN conversation.dm_high_account_id ELSE conversation.dm_low_account_id END;
  IF NOT app.lock_active_contact_accounts(NEW.sender_id,other_account) OR NOT app.contact_allowed(NEW.sender_id,other_account) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='contact unavailable';
  END IF;
 END IF;
 NEW.sent_at:=clock_timestamp();
 RETURN NEW;
END $$;
CREATE TRIGGER message_sender BEFORE INSERT ON app.message
 FOR EACH ROW EXECUTE FUNCTION app.check_message_sender();
CREATE INDEX message_conversation_page ON app.message(conversation_id,sent_at DESC,id DESC);

-- Expired contact eligibility changes sending, not existing participant history access.
CREATE FUNCTION app.read_conversation_messages(p_actor uuid,p_conversation uuid,p_before_at timestamptz,p_before_id uuid,p_limit integer)
RETURNS TABLE(message_id uuid,sender_id uuid,author text,body text,sent_at timestamptz)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR ((p_before_at IS NULL)<>(p_before_id IS NULL)) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid message page bounds';
 END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state IN ('active','deletion_pending'))
 OR NOT EXISTS(SELECT 1 FROM app.conversation_member WHERE conversation_id=p_conversation AND account_id=p_actor) THEN
  RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='conversation unavailable';
 END IF;
 RETURN QUERY SELECT m.id,m.sender_id,CASE WHEN a.state='deleted' THEN 'Deleted player' ELSE a.username END,m.body,m.sent_at
 FROM app.message m JOIN app.account a ON a.id=m.sender_id WHERE m.conversation_id=p_conversation
 AND (p_before_at IS NULL OR (m.sent_at,m.id)<(p_before_at,p_before_id))
 ORDER BY m.sent_at DESC,m.id DESC LIMIT p_limit;
END $$;
GRANT EXECUTE ON FUNCTION app.read_conversation_messages(uuid,uuid,timestamptz,uuid,integer) TO service_role;

COMMIT;
