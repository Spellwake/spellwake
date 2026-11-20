-- Actor-derived global unread count and current selected identity, independent of list paging.
BEGIN;
CREATE FUNCTION app.social_thread_candidates(p_actor uuid,p_at timestamptz,p_peer uuid) RETURNS TABLE(id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT CASE WHEN f.account_low_id=p_actor THEN f.account_high_id ELSE f.account_low_id END
  FROM app.friendship f WHERE p_actor IN (f.account_low_id,f.account_high_id) AND f.state IN ('pending','accepted')
  AND (p_peer IS NULL OR p_peer=CASE WHEN f.account_low_id=p_actor THEN f.account_high_id ELSE f.account_low_id END)
 UNION
 SELECT b.blocked_id FROM app.block b WHERE b.blocker_id=p_actor AND (p_peer IS NULL OR b.blocked_id=p_peer)
 UNION
 SELECT CASE WHEN c.dm_low_account_id=p_actor THEN c.dm_high_account_id ELSE c.dm_low_account_id END
  FROM app.conversation c JOIN app.conversation_member cm ON cm.conversation_id=c.id AND cm.account_id=p_actor
  WHERE c.kind='dm' AND p_actor IN (c.dm_low_account_id,c.dm_high_account_id)
  AND (p_peer IS NULL OR (c.dm_low_account_id=least(p_actor,p_peer) AND c.dm_high_account_id=greatest(p_actor,p_peer)))
 UNION
 SELECT b.account_id FROM app.game_participation a JOIN app.game_participation b ON b.game_id=a.game_id AND b.account_id<>p_actor
  WHERE a.account_id=p_actor AND a.real_game AND b.real_game
  AND greatest(a.completed_at,b.completed_at) BETWEEN p_at-interval '336 hours' AND p_at
  AND (p_peer IS NULL OR b.account_id=p_peer)
$$;
REVOKE ALL ON FUNCTION app.social_thread_candidates(uuid,timestamptz,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.read_my_social_people(p_kind text,p_query text,p_after uuid,p_limit integer) RETURNS jsonb
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
  SELECT id FROM app.social_thread_candidates(actor,evaluated,NULL) WHERE p_kind='threads'
 ), selected AS (SELECT id FROM candidates WHERE p_after IS NULL OR id>p_after ORDER BY id LIMIT p_limit)
 SELECT coalesce(jsonb_agg(app.social_person_snapshot(actor,id,evaluated) ORDER BY id),'[]') INTO items FROM selected;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,
  'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'id' END));
END $$;

CREATE FUNCTION app.read_my_social_status(p_peer uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; person jsonb; unread text; evaluated timestamptz:=statement_timestamp();
BEGIN
 IF p_peer='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('page','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT count(*)::text INTO unread FROM app.notification n
 JOIN app.message m ON n.source_ref=m.id::text AND m.sender_id<>actor
 JOIN app.conversation c ON c.id=m.conversation_id AND c.kind='dm'
  AND actor IN (c.dm_low_account_id,c.dm_high_account_id)
 JOIN app.conversation_member cm ON cm.conversation_id=c.id AND cm.account_id=actor
 WHERE n.recipient_id=actor AND n.source_kind='dm_message' AND n.read_at IS NULL;
 -- Selection uses the exact same retained identity sources as the paged thread read.
 IF p_peer IS NOT NULL AND p_peer<>actor AND EXISTS(SELECT 1 FROM app.account a WHERE a.id=p_peer
  AND (a.state='active' OR EXISTS(SELECT 1 FROM app.social_thread_candidates(actor,evaluated,p_peer)))) THEN
  person:=app.social_person_snapshot(actor,p_peer,evaluated);
 END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,
  'unreadMessages',unread,'personId',p_peer,'person',person));
END $$;
REVOKE ALL ON FUNCTION app.read_my_social_status(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_social_status(uuid) TO authenticated;
COMMIT;
