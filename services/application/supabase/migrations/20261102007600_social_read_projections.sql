-- Explicit own-session social projections. No new table or client mutation grant.
BEGIN;

CREATE FUNCTION app.social_person_snapshot(p_actor uuid,p_peer uuid,p_at timestamptz) RETURNS jsonb
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('id',a.id,'username',CASE WHEN a.state='deleted' THEN 'Deleted player' ELSE a.username END,
  'isDeleted',a.state='deleted',
  'relationship',CASE WHEN a.state='deleted' THEN 'none' WHEN f.state='accepted' THEN 'friend'
   WHEN f.state='pending' AND f.requested_by_id=p_actor THEN 'outgoing_request'
   WHEN f.state='pending' THEN 'incoming_request' ELSE 'none' END,
  'friendshipId',CASE WHEN a.state<>'deleted' AND f.state IN ('pending','accepted') THEN f.id END,
  'isBlocked',EXISTS(SELECT 1 FROM app.block b WHERE b.blocker_id=p_actor AND b.blocked_id=a.id),
  'canContact',app.contact_allowed_at(p_actor,a.id,p_at),
  'conversationId',c.id,
  'unreadMessages',(SELECT count(*)::text FROM app.message m JOIN app.notification n
   ON n.source_kind='dm_message' AND n.source_ref=m.id::text AND n.recipient_id=p_actor AND n.read_at IS NULL
   WHERE m.conversation_id=c.id AND m.sender_id<>p_actor),
  'supporter',app.active_supporter_tier(a.id) IS NOT NULL)
 FROM app.account a
 LEFT JOIN app.friendship f ON f.account_low_id=least(p_actor,a.id) AND f.account_high_id=greatest(p_actor,a.id)
 LEFT JOIN app.conversation c ON c.kind='dm' AND c.dm_low_account_id=least(p_actor,a.id)
  AND c.dm_high_account_id=greatest(p_actor,a.id)
  AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=p_actor)
 WHERE a.id=p_peer AND a.id<>p_actor
$$;
REVOKE ALL ON FUNCTION app.social_person_snapshot(uuid,uuid,timestamptz) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.read_my_social_people(p_kind text,p_query text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; evaluated timestamptz:=statement_timestamp(); items jsonb;
BEGIN
 IF p_kind IS NULL OR p_kind NOT IN ('directory','threads') OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid
 OR (p_kind='directory' AND (p_query IS NULL OR length(btrim(p_query)) NOT BETWEEN 1 AND 64))
 OR (p_kind='threads' AND p_query IS NOT NULL) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 WITH candidates AS (
  SELECT a.id FROM app.account a WHERE p_kind='directory' AND a.state='active' AND a.id<>actor
   -- Preserve literal search and use identical ASCII folding across runtimes.
   AND strpos(translate(a.username,'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'),translate(btrim(p_query),'ABCDEFGHIJKLMNOPQRSTUVWXYZ','abcdefghijklmnopqrstuvwxyz'))>0
  UNION
  SELECT CASE WHEN f.account_low_id=actor THEN f.account_high_id ELSE f.account_low_id END
   FROM app.friendship f WHERE p_kind='threads' AND actor IN (f.account_low_id,f.account_high_id) AND f.state IN ('pending','accepted')
  UNION
  SELECT b.blocked_id FROM app.block b WHERE p_kind='threads' AND b.blocker_id=actor
  UNION
  SELECT CASE WHEN c.dm_low_account_id=actor THEN c.dm_high_account_id ELSE c.dm_low_account_id END
   FROM app.conversation c JOIN app.conversation_member cm ON cm.conversation_id=c.id AND cm.account_id=actor
   WHERE p_kind='threads' AND c.kind='dm' AND actor IN (c.dm_low_account_id,c.dm_high_account_id)
  UNION
  SELECT b.account_id FROM app.game_participation a JOIN app.game_participation b ON b.game_id=a.game_id AND b.account_id<>actor
   WHERE p_kind='threads' AND a.account_id=actor AND a.real_game AND b.real_game
   AND greatest(a.completed_at,b.completed_at) BETWEEN evaluated-interval '336 hours' AND evaluated
 ), selected AS (SELECT id FROM candidates WHERE p_after IS NULL OR id>p_after ORDER BY id LIMIT p_limit)
 SELECT coalesce(jsonb_agg(app.social_person_snapshot(actor,id,evaluated) ORDER BY id),'[]') INTO items FROM selected;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'id' END));
END $$;

-- Sixteen maximum-length Unicode messages fit the existing1MiB RPC envelope even with JSON escapes.
CREATE FUNCTION app.read_my_direct_messages(p_conversation uuid,p_before_at timestamptz,p_before_id uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; peer uuid; items jsonb;
BEGIN
 IF p_conversation IS NULL OR p_conversation='00000000-0000-0000-0000-000000000000'::uuid
 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 16 OR ((p_before_at IS NULL)<>(p_before_id IS NULL))
 OR p_before_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT CASE WHEN c.dm_low_account_id=actor THEN c.dm_high_account_id ELSE c.dm_low_account_id END INTO peer
  FROM app.conversation c WHERE c.id=p_conversation AND c.kind='dm' AND actor IN (c.dm_low_account_id,c.dm_high_account_id)
  AND EXISTS(SELECT 1 FROM app.conversation_member cm WHERE cm.conversation_id=c.id AND cm.account_id=actor);
 IF peer IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('conversation','unavailable')); END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('id',m.message_id,'senderId',m.sender_id,'author',m.author,'body',m.body,'sentAt',m.sent_at)
  ORDER BY m.sent_at DESC,m.message_id DESC),'[]') INTO items
  FROM app.read_conversation_messages(actor,p_conversation,p_before_at,p_before_id,p_limit) m;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'conversationId',p_conversation,
  'person',app.social_person_snapshot(actor,peer,statement_timestamp()),'items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN jsonb_build_object('sentAt',items->-1->'sentAt','id',items->-1->'id') END));
END $$;

REVOKE ALL ON FUNCTION app.read_my_social_people(text,text,uuid,integer),app.read_my_direct_messages(uuid,timestamptz,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_social_people(text,text,uuid,integer),app.read_my_direct_messages(uuid,timestamptz,uuid,integer) TO authenticated;
-- The actor-selected reader remains the private canonical history implementation.
REVOKE EXECUTE ON FUNCTION app.read_conversation_messages(uuid,uuid,timestamptz,uuid,integer) FROM service_role;
COMMIT;
