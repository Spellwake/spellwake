BEGIN;
CREATE FUNCTION app.read_my_quests(p_after uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;daily jsonb;today text;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 today:=to_char(statement_timestamp() AT TIME ZONE 'UTC','YYYY-MM-DD');
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT q.id,jsonb_build_object('id',q.id,'definitionId',q.definition_id,'sourceDay',q.source_day,'assignedAt',q.assigned_at,'progress',q.progress::text,'completedAt',q.completed_at,'replacedById',q.replaced_by_id,'objective',CASE WHEN d.id IS NOT NULL THEN jsonb_build_object('definitionKey',d.definition_key,'revision',d.revision,'objectiveRef',d.objective_ref,'target',d.target::text,'rewardCoins',d.reward_coins::text) END) body
 FROM app.quest_assignment q LEFT JOIN app.quest_definition_revision d ON d.id=q.definition_id WHERE q.account_id=actor AND(p_after IS NULL OR q.id>p_after) ORDER BY q.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 SELECT jsonb_build_object('generatedAssignmentId',generated_assignment_id,'rerollsUsed',rerolls_used::text,'revision',revision::text) INTO daily FROM app.daily_quest_state WHERE account_id=actor AND utc_day=today;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'utcDay',today,'daily',daily,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_ranks(p_after uuid,p_limit integer,p_format uuid,p_season uuid) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_after,p_format,p_season) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'formatId',format_id,'seasonId',season_id,'tier',tier,'rating',rating::text,'revision',revision::text,'finalized',finalized) body FROM app.rank WHERE account_id=actor AND(p_after IS NULL OR id>p_after) AND(p_format IS NULL OR format_id=p_format) AND(p_season IS NULL OR season_id=p_season) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_seasons(p_after uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT id,jsonb_build_object('id',id,'year',year::text,'quarter',quarter::text,'startAt',start_at,'endAt',end_at,'state',state) body FROM app.season WHERE p_after IS NULL OR id>p_after ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
REVOKE ALL ON FUNCTION app.read_my_quests(uuid,integer),app.read_my_ranks(uuid,integer,uuid,uuid),app.read_my_seasons(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_quests(uuid,integer),app.read_my_ranks(uuid,integer,uuid,uuid),app.read_my_seasons(uuid,integer) TO authenticated;
COMMIT;
