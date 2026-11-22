BEGIN;

-- The worker and its wake predicate use the same runnable reservation page.
CREATE OR REPLACE FUNCTION app.read_commander_opening_page(p_release uuid,p_after uuid,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander opening page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT g.id FROM app.game g
     JOIN app.match m ON m.reserved_game_id=g.id AND m.id=g.match_id
     JOIN app.format f ON f.id=m.format_id AND f.key='commander'
     JOIN app.native_opening_selection selection ON selection.game_id=g.id
     WHERE m.accepted_release_id=p_release AND g.state='reserved'
       AND m.state='ready' AND m.availability='available' AND selection.first_id IS NOT NULL
       AND (p_after IS NULL OR g.id>p_after)
     ORDER BY g.id LIMIT p_limit
   ) job
 ),'[]'::jsonb));
END $$;

-- An opening handoff becomes runnable once the opening receipt is accepted.
CREATE OR REPLACE FUNCTION app.read_commander_handoff_page(p_release uuid,p_after uuid,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander handoff page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT h.id FROM app.durable_handoff h
     JOIN app.match m ON m.id=h.match_id
     JOIN app.format f ON f.id=m.format_id AND f.key='commander'
     WHERE m.accepted_release_id=p_release AND h.state='pending'
       AND (
         (h.destination,h.kind) IN (('match_lifecycle','game_outcome'),('match_settlement','match_outcome'))
         OR ((h.destination,h.kind) IN (('game_host','ready_match'),('game_host','reserved_game')) AND EXISTS (
           SELECT 1 FROM app.command_receipt receipt
           WHERE receipt.scope='native_opening_v1' AND receipt.state='accepted'
             AND receipt.principal_key='native-opening:'||h.game_id::text
             AND receipt.target_ref=h.game_id::text
             AND receipt.normalized_source->>'matchId'=h.match_id::text
         ))
       )
       AND (p_after IS NULL OR h.id>p_after)
     ORDER BY h.id LIMIT p_limit
   ) job
 ),'[]'::jsonb));
END $$;

CREATE FUNCTION app.commander_worker_has_due_work(p_release uuid)
RETURNS boolean LANGUAGE sql STABLE SECURITY INVOKER SET search_path='' AS $$
 SELECT jsonb_array_length(app.read_commander_opening_page(p_release,NULL,1)->'items') > 0
     OR jsonb_array_length(app.read_commander_handoff_page(p_release,NULL,1)->'items') > 0
$$;

REVOKE ALL ON FUNCTION app.commander_worker_has_due_work(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.commander_worker_has_due_work(uuid) TO service_role;

COMMIT;
