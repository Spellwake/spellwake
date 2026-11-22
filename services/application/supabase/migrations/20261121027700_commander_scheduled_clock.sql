BEGIN;

-- The worker receives exact accepted checkpoints for due opening responses.
CREATE FUNCTION app.read_commander_clock_page(
 p_release uuid, p_owner text, p_after uuid, p_limit integer
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_release IS NULL OR p_owner IS NULL OR btrim(p_owner)='' OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander clock page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
  SELECT jsonb_agg(job.item ORDER BY job.id)
  FROM (
   SELECT g.id,jsonb_build_object('id',g.id,'requestId',cp.id,'basis',jsonb_build_object(
      'gameId',g.id,'contextId',cp.context_id,'epoch',o.normalized_source->>'nativeEpoch',
      'revision',g.accepted_revision::text)) item
   FROM app.match m
   JOIN app.format f ON f.id=m.format_id AND f.key='commander'
   JOIN app.game g ON g.id=m.current_game_id AND g.match_id=m.id AND g.state='active'
   JOIN app.operational_state op ON op.game_id=g.id AND op.owner_instance_ref=p_owner
     AND op.availability='available'
   JOIN app.game_checkpoint cp ON cp.game_id=g.id AND cp.accepted_revision=g.accepted_revision
   JOIN app.game_context ctx ON ctx.id=cp.context_id AND ctx.game_id=g.id AND ctx.release_id=p_release
   JOIN app.command_receipt o ON o.principal_key='native-opening:'||g.id::text
     AND o.request_ref=g.id::text AND o.target_ref=g.id::text
     AND o.scope='native_opening_v1' AND o.state='accepted'
   CROSS JOIN LATERAL app.native_opening_wait_window(g.id,g.accepted_revision) wait
   CROSS JOIN LATERAL app.read_native_opening_response_wait(g.id) observed
   WHERE m.accepted_release_id=p_release AND m.state='active' AND m.availability='available'
     AND (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)=3
     AND (p_after IS NULL OR g.id>p_after)
     AND observed#>>'{data,seatId}'=wait.seat_id::text
     AND observed#>>'{data,decisionRef}'=wait.decision_ref
     AND observed#>>'{data,due}'='true'
   ORDER BY g.id LIMIT p_limit
  ) job
 ),'[]'::jsonb));
END $$;
REVOKE ALL ON FUNCTION app.read_commander_clock_page(uuid,text,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_commander_clock_page(uuid,text,uuid,integer) TO service_role;

-- A scheduled wake and the pinned Machine query the same owner-scoped work.
CREATE FUNCTION app.commander_worker_has_due_work(p_release uuid,p_owner text)
RETURNS boolean LANGUAGE sql STABLE SECURITY INVOKER SET search_path='' AS $$
 SELECT app.commander_worker_has_due_work(p_release)
    OR CASE WHEN p_owner IS NULL THEN false
       ELSE jsonb_array_length(app.read_commander_clock_page(p_release,p_owner,NULL,1)->'items')>0 END
$$;
REVOKE ALL ON FUNCTION app.commander_worker_has_due_work(uuid,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.commander_worker_has_due_work(uuid,text) TO service_role;

CREATE FUNCTION app.enqueue_commander_match_worker_wake(p_release uuid,p_project_ref text,p_owner text)
RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY INVOKER SET search_path='' AS $$
DECLARE wake_token text;
BEGIN
 IF p_release IS NULL OR p_project_ref IS NULL OR p_project_ref !~ '^[a-z0-9]{20}$'
    OR (p_owner IS NOT NULL AND btrim(p_owner)='') THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander worker schedule';
 END IF;
 IF NOT app.commander_worker_has_due_work(p_release,p_owner) THEN RETURN NULL; END IF;
 SELECT secret.decrypted_secret INTO wake_token
 FROM vault.decrypted_secrets secret WHERE secret.name='spellwake_commander_worker_wake';
 IF wake_token IS NULL OR wake_token !~ '^[a-f0-9]{64}$' THEN
   RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='Commander worker wake credential unavailable';
 END IF;
 RETURN net.http_post(
   url := 'https://'||p_project_ref||'.supabase.co/functions/v1/wake-commander-worker',
   headers := jsonb_build_object('Content-Type','application/json','x-spellwake-worker-wake',wake_token),
   body := '{}'::jsonb, timeout_milliseconds := 12000
 );
END $$;
REVOKE ALL ON FUNCTION app.enqueue_commander_match_worker_wake(uuid,text,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- The opening and handoff-only callers keep their existing contract.
CREATE OR REPLACE FUNCTION app.enqueue_commander_match_worker_wake(p_release uuid,p_project_ref text)
RETURNS bigint LANGUAGE sql VOLATILE SECURITY INVOKER SET search_path='' AS $$
 SELECT app.enqueue_commander_match_worker_wake(p_release,p_project_ref,NULL::text)
$$;
REVOKE ALL ON FUNCTION app.enqueue_commander_match_worker_wake(uuid,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
