BEGIN;

-- Keep172's complete internal source for retirement/status EXISTS predicates.
-- This private transport projection bounds returned rows, not source evaluation cost.
CREATE FUNCTION app.read_ai_stop_work_page(p_binding uuid,p_after uuid,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE items jsonb; cursor_id uuid;
BEGIN
 IF p_binding IS NULL OR p_binding='00000000-0000-0000-0000-000000000000'::uuid
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid
 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid bounded AI stop discovery';
 END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY (body->>'stopId')::uuid),'[]'::jsonb) INTO items
 FROM (SELECT pending.body FROM app.read_ai_stop_work(p_binding) AS pending(body)
       WHERE p_after IS NULL OR (pending.body->>'stopId')::uuid>p_after
       ORDER BY (pending.body->>'stopId')::uuid LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'stopId')::uuid;END IF;
 RETURN jsonb_build_object('bindingId',p_binding,'items',items,'nextCursor',cursor_id);
END $$;
REVOKE ALL ON FUNCTION app.read_ai_stop_work_page(uuid,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
