BEGIN;

DO $predecessor$
BEGIN
 IF EXISTS (
    SELECT 1 FROM (VALUES
    ('app.read_native_match_delivery_page(text,text,uuid,integer)', 'aa77d457917f9dc01ac968349123cb77fea9463fb946a5e4185573aa28dbc42a'),
    ('app.enqueue_native_match_worker_wake()', '482b057c60581e00ff32cea69d465dec943e4c544a634dc02d55ac568105b3bb'),
    ('app.read_commander_opening_page(uuid,uuid,integer)', 'e53098051ffe4f9ae17e265d9d77dc1876f1727e5b61f14085529aa459709941'),
    ('app.read_commander_handoff_page(uuid,uuid,integer)', '99e2ce2e960da598d0bad3213d9c9241334954c38767d1cee7a2a6803912e4a1'),
    ('app.read_commander_clock_page(uuid,text,uuid,integer)', '80bb2e5c6fc9642c2871e4b9c47ead66f6d6169b49d91176fdd3a903bcbc1f66')
    ) expected(signature, body_hash)
    LEFT JOIN pg_catalog.pg_proc prior ON prior.oid=pg_catalog.to_regprocedure(expected.signature)
    WHERE pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(prior.prosrc,'UTF8')),'hex')
          IS DISTINCT FROM expected.body_hash
 ) THEN
    RAISE EXCEPTION 'private pilot worker predecessor differs from the reviewed schema';
 END IF;
END
$predecessor$;


CREATE FUNCTION app.read_private_pilot_opening_page(p_release uuid, p_after uuid, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private pilot opening page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT g.id FROM app.game g
     JOIN app.match m ON m.reserved_game_id=g.id AND m.id=g.match_id
     JOIN app.private_pilot_profile pilot ON pilot.profile_id=m.profile_id
       AND pilot.format_id=m.format_id AND pilot.release_id=m.accepted_release_id AND NOT pilot.commander
     JOIN app.native_opening_selection selection ON selection.game_id=g.id
     WHERE m.accepted_release_id=p_release AND g.state='reserved'
       AND m.state='ready' AND m.availability='available' AND selection.first_id IS NOT NULL
       AND (p_after IS NULL OR g.id>p_after)
     ORDER BY g.id LIMIT p_limit
   ) job
 ),'[]'::jsonb));
END $function$;
REVOKE ALL ON FUNCTION app.read_private_pilot_opening_page(uuid,uuid,integer) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_private_pilot_opening_page(uuid,uuid,integer) TO service_role;

CREATE FUNCTION app.read_private_pilot_handoff_page(p_release uuid, p_after uuid, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private pilot handoff page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT h.id FROM app.durable_handoff h
     JOIN app.match m ON m.id=h.match_id
     JOIN app.private_pilot_profile pilot ON pilot.profile_id=m.profile_id
       AND pilot.format_id=m.format_id AND pilot.release_id=m.accepted_release_id AND NOT pilot.commander
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
END $function$;
REVOKE ALL ON FUNCTION app.read_private_pilot_handoff_page(uuid,uuid,integer) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_private_pilot_handoff_page(uuid,uuid,integer) TO service_role;

CREATE FUNCTION app.read_private_pilot_clock_page(p_release uuid, p_owner text, p_after uuid, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
BEGIN
 IF p_release IS NULL OR p_owner IS NULL OR btrim(p_owner)='' OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private pilot clock page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
  SELECT jsonb_agg(job.item ORDER BY job.id)
  FROM (
   SELECT g.id,jsonb_build_object('id',g.id,'requestId',cp.id,'basis',jsonb_build_object(
      'gameId',g.id,'contextId',cp.context_id,'epoch',o.normalized_source->>'nativeEpoch',
      'revision',g.accepted_revision::text)) item
   FROM app.match m
   JOIN app.private_pilot_profile pilot ON pilot.profile_id=m.profile_id
       AND pilot.format_id=m.format_id AND pilot.release_id=m.accepted_release_id AND NOT pilot.commander
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
     AND (SELECT count(*) FROM app.match_seat WHERE match_id=m.id)=2
     AND (p_after IS NULL OR g.id>p_after)
     AND observed#>>'{data,seatId}'=wait.seat_id::text
     AND observed#>>'{data,decisionRef}'=wait.decision_ref
     AND observed#>>'{data,due}'='true'
   ORDER BY g.id LIMIT p_limit
  ) job
 ),'[]'::jsonb));
END $function$;
REVOKE ALL ON FUNCTION app.read_private_pilot_clock_page(uuid,text,uuid,integer) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_private_pilot_clock_page(uuid,text,uuid,integer) TO service_role;

CREATE OR REPLACE FUNCTION app.read_native_match_delivery_page(p_owner text, p_lane text, p_after uuid, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE rows jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_owner IS NULL OR btrim(p_owner)='' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery page'; END IF;
 IF p_lane='handoff' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT h.id,jsonb_build_object('id',h.id,'kind',h.kind,'gameId',h.game_id) body FROM app.durable_handoff h
   JOIN app.match m ON m.id=h.match_id
   JOIN app.format f ON f.id=m.format_id
   WHERE f.key<>'commander' AND h.state='pending' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id) AND h.kind IN ('ready_match','reserved_game','game_outcome','match_outcome','between_deadline')
    AND (p_after IS NULL OR h.id>p_after) ORDER BY h.id LIMIT p_limit+1) page;
 ELSIF p_lane='between' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT d.id,jsonb_build_object('id',d.id,'matchRevision',m.revision::text,'profileId',m.profile_id,'releaseId',m.accepted_release_id,
    'releaseProof',r.evidence_ref,'selectedRevisionId',coalesce(previous.selected_deck_revision_id,seat.source_revision_id),
    'deadlineRevision',d.deadline_revision::text,'requestId',coalesce(d.deadline_source_receipt_id,d.id)) body
   FROM app.between_game_decision d JOIN app.match m ON m.id=d.match_id JOIN app.match_seat seat ON seat.id=d.seat_id
    JOIN app.qualified_release r ON r.id=m.accepted_release_id JOIN app.game g ON g.id=d.after_game_id
    LEFT JOIN app.game prev ON prev.match_id=m.id AND prev.game_index=g.game_index-1
    LEFT JOIN app.between_game_decision previous ON previous.after_game_id=prev.id AND previous.seat_id=d.seat_id AND previous.state='answered'
   WHERE (p_after IS NULL OR d.id>p_after) AND d.state='pending' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id) AND m.state='between_games' AND m.availability='available'
    AND (d.deadline_policy_ref IS NULL OR (d.deadline_policy_ref='native-between-keep-deck-v1' AND d.deadline_at<=statement_timestamp()))
   ORDER BY d.id LIMIT p_limit+1) page;
 ELSIF p_lane='clock' THEN
  SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
   SELECT g.id,jsonb_build_object('id',g.id,'requestId',c.id,'basis',jsonb_build_object('gameId',g.id,'contextId',c.context_id,
    'epoch',o.normalized_source->>'nativeEpoch','revision',g.accepted_revision::text)) body
   FROM app.game g JOIN app.match m ON m.id=g.match_id AND m.current_game_id=g.id JOIN app.operational_state op ON op.game_id=g.id
    JOIN app.game_checkpoint c ON c.game_id=g.id AND c.accepted_revision=g.accepted_revision
    JOIN app.command_receipt o ON o.principal_key='native-opening:'||g.id::text AND o.scope='native_opening_v1' AND o.state='accepted'
   WHERE op.owner_instance_ref=p_owner AND op.availability='available' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id) AND g.state='active' AND m.state='active' AND m.availability='available'
    AND (p_after IS NULL OR g.id>p_after) ORDER BY g.id LIMIT p_limit+1) page;
 ELSE RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid delivery lane'; END IF;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $function$;

CREATE OR REPLACE FUNCTION app.enqueue_native_match_worker_wake()
 RETURNS bigint
 LANGUAGE plpgsql
 SET search_path TO ''
AS $function$BEGIN
 PERFORM app.dispatch_native_opening_choices(16);
 RETURN (WITH work AS (WITH settings AS (
  SELECT 'spellwake-fly-worker'::text AS host
)
SELECT (
  EXISTS (
    SELECT 1
    FROM app.durable_handoff h
    JOIN app.match m ON m.id = h.match_id
    JOIN app.format f ON f.id = m.format_id
    WHERE f.key <> 'commander' AND h.state = 'pending' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id)
      AND (
        (h.destination, h.kind) IN (
          ('match_lifecycle', 'game_outcome'), ('match_settlement', 'match_outcome')
        )
        OR (
          h.destination = 'game_host'
          AND h.kind IN ('ready_match', 'reserved_game')
          AND (
            EXISTS (
              SELECT 1
              FROM app.command_receipt c
              WHERE c.scope = 'native_opening_v1'
                AND c.state = 'accepted'
                AND c.principal_key = 'native-opening:' || h.game_id::text
                AND c.target_ref = h.game_id::text
                AND c.normalized_source->>'matchId' = h.match_id::text
            )
            OR EXISTS (
              SELECT 1
              FROM app.native_opening_selection s
              JOIN app.game g ON g.id = s.game_id AND g.match_id = h.match_id
              JOIN app.match m ON m.id = g.match_id
              WHERE s.game_id = h.game_id AND s.first_id IS NOT NULL
                AND g.state = 'reserved' AND m.state = 'ready'
                AND m.availability = 'available'
            )
          )
        )
        OR (
          h.destination = 'match_lifecycle' AND h.kind = 'between_deadline'
          AND NOT EXISTS (
            SELECT 1
            FROM app.between_game_decision d
            CROSS JOIN LATERAL app.handoff_source_manifest(h.id) manifest
            WHERE d.id = (manifest->'timer'->>'decisionId')::uuid
              AND d.deadline_source_receipt_id::text = manifest->'timer'->>'sourceReceiptId'
              AND d.state = 'pending'
          )
        )
      )
  )
  OR EXISTS (
    SELECT 1
    FROM app.between_game_decision d
    JOIN app.match m ON m.id = d.match_id
    JOIN app.format f ON f.id = m.format_id
    WHERE f.key <> 'commander' AND d.state = 'pending' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id) AND m.state = 'between_games' AND m.availability = 'available'
      AND (d.deadline_policy_ref IS NULL OR
        (d.deadline_policy_ref = 'native-between-keep-deck-v1'
          AND d.deadline_at <= statement_timestamp()))
  )
  OR EXISTS (
    SELECT 1
    FROM settings
    JOIN app.operational_state op ON op.owner_instance_ref = settings.host
    JOIN app.game g ON g.id = op.game_id AND g.state = 'active'
    JOIN app.match m ON m.current_game_id = g.id AND m.state = 'active'
    JOIN app.format f ON f.id = m.format_id AND f.key <> 'commander'
    JOIN app.game_checkpoint c ON c.game_id = g.id AND c.accepted_revision = g.accepted_revision
    JOIN app.command_receipt o ON o.principal_key = 'native-opening:' || g.id::text
      AND o.scope = 'native_opening_v1' AND o.state = 'accepted'
    CROSS JOIN LATERAL app.observe_native_clock_job(
      g.id, jsonb_build_object(
        'gameId', g.id, 'contextId', c.context_id,
        'epoch', o.normalized_source->>'nativeEpoch', 'revision', g.accepted_revision::text
      ), settings.host, c.id
    ) observation
    WHERE op.availability = 'available' AND m.availability = 'available' AND NOT EXISTS (SELECT 1 FROM app.private_pilot_profile pilot WHERE pilot.profile_id=m.profile_id AND pilot.release_id=m.accepted_release_id)
      AND CASE observation->>'status'
        WHEN 'bank' THEN true
        WHEN 'response' THEN app.read_native_opening_response_wait(g.id)#>>'{data,due}' = 'true'
        ELSE false
      END
  )
) AS due)
    SELECT net.http_post(
      url := 'https://yiuaajmqiofpgczhqeho.supabase.co/functions/v1/wake-match-worker',
      headers := jsonb_build_object('Content-Type','application/json',
         'x-spellwake-worker-wake',credential.decrypted_secret),
      body := '{}'::jsonb,
      timeout_milliseconds := 12000)
    FROM work CROSS JOIN vault.decrypted_secrets credential
    WHERE work.due AND credential.id='0d6fc74d-4eef-4937-a27b-2346f9faf528'::uuid);
END$function$;


CREATE FUNCTION app.enqueue_private_eight_format_pilot_wake(
    p_ordinary uuid,p_commander uuid,p_owner text,p_project_ref text
) RETURNS bigint LANGUAGE plpgsql SET search_path='' AS $$
DECLARE wake_token text;
BEGIN
    IF p_ordinary IS NULL OR p_commander IS NULL OR p_owner IS NULL OR btrim(p_owner)=''
       OR p_project_ref IS NULL OR p_project_ref !~ '^[a-z0-9]{20}$' THEN
        RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid private pilot worker schedule';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM app.private_pilot_profile WHERE release_id=p_ordinary AND NOT commander
    ) OR NOT EXISTS (
        SELECT 1 FROM app.private_pilot_profile WHERE release_id=p_commander AND commander
    ) THEN RETURN NULL; END IF;
    IF NOT (app.commander_worker_has_due_work(p_commander,p_owner)
        OR jsonb_array_length(app.read_private_pilot_opening_page(p_ordinary,NULL,1)->'items')>0
        OR jsonb_array_length(app.read_private_pilot_handoff_page(p_ordinary,NULL,1)->'items')>0
        OR jsonb_array_length(app.read_private_pilot_clock_page(p_ordinary,p_owner,NULL,1)->'items')>0)
    THEN RETURN NULL; END IF;
    SELECT decrypted_secret INTO wake_token FROM vault.decrypted_secrets
    WHERE name='spellwake_eight_format_pilot_wake';
    IF wake_token IS NULL OR wake_token !~ '^[a-f0-9]{64}$' THEN
        RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='private pilot worker wake credential unavailable';
    END IF;
    RETURN net.http_post(
        url := 'https://'||p_project_ref||'.supabase.co/functions/v1/wake-eight-format-pilot',
        headers := jsonb_build_object('Content-Type','application/json','x-spellwake-worker-wake',wake_token),
        body := '{}'::jsonb, timeout_milliseconds := 12000
    );
END $$;
REVOKE ALL ON FUNCTION app.enqueue_private_eight_format_pilot_wake(uuid,uuid,text,text) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

COMMIT;
