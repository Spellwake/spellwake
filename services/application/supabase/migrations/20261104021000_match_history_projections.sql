BEGIN;
-- Participant metadata only. Native views, artifacts and replay content stay private.
CREATE FUNCTION app.read_my_matches(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT m.id,jsonb_build_object('id',m.id,'formatId',m.format_id,'profileId',m.profile_id,'mode',m.mode,'competitive',m.competitive,'structure',m.structure,'state',m.state,'revision',m.revision::text,'startedAt',m.started_at,'currentGameId',m.current_game_id,'reservedGameId',m.reserved_game_id,'availability',m.availability,'availabilityRevision',m.availability_revision::text) body FROM app.match m WHERE(p_after IS NULL OR m.id>p_after) ORDER BY m.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_match_metadata(p_match uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;metadata jsonb;result jsonb;own_seat uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT jsonb_build_object('id',m.id,'formatId',m.format_id,'profileId',m.profile_id,'mode',m.mode,'competitive',m.competitive,'structure',m.structure,'state',m.state,'revision',m.revision::text,'startedAt',m.started_at,'currentGameId',m.current_game_id,'reservedGameId',m.reserved_game_id,'availability',m.availability,'availabilityRevision',m.availability_revision::text) INTO metadata FROM app.match m WHERE id=p_match;
 SELECT id INTO own_seat FROM app.match_seat WHERE match_id=p_match AND controller_kind='human' AND account_id=actor;
 SELECT jsonb_build_object('id',id,'outcome',outcome,'committedAt',committed_at,'rankingFormatId',ranking_format_id,'rankSequence',rank_sequence::text,'winnerSeatIds',winner_seat_ids,'terminalKind',terminal_kind) INTO result FROM app.match_result WHERE match_id=p_match;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'match',metadata,'ownSeatId',own_seat,'result',result));
END $$;
CREATE FUNCTION app.read_match_games(p_match uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT g.id,jsonb_build_object('id',g.id,'gameIndex',g.game_index::text,'state',g.state,'acceptedRevision',g.accepted_revision::text,'result',CASE WHEN r.id IS NOT NULL THEN jsonb_build_object('id',r.id,'outcome',r.outcome,'winnerSeatIds',r.winner_seat_ids,'endReason',r.end_reason) END) body FROM app.game g LEFT JOIN app.game_result r ON r.game_id=g.id WHERE g.match_id=p_match AND(p_after IS NULL OR g.id>p_after) ORDER BY g.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_match_seats(p_match uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT s.id,jsonb_build_object('id',s.id,'seatIndex',s.seat_index::text,'controllerKind',s.controller_kind,'person',CASE WHEN s.controller_kind='human' THEN jsonb_build_object('id',a.id,'username',CASE WHEN a.state='deleted' THEN 'Deleted player' ELSE a.username END,'isDeleted',a.state='deleted') END,'clock',CASE WHEN c.id IS NOT NULL THEN jsonb_build_object('remainingMs',c.remaining_ms::text,'revision',c.clock_revision::text,'policyRef',c.policy_ref,'paused',c.paused,'lastAccountedAt',c.last_accounted_at) END) body FROM app.match_seat s LEFT JOIN app.account a ON a.id=s.account_id LEFT JOIN app.match_clock c ON c.match_seat_id=s.id WHERE s.match_id=p_match AND(p_after IS NULL OR s.id>p_after) ORDER BY s.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_match_decisions(p_match uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT id,jsonb_build_object('id',id,'seatId',seat_id,'decisionKind',decision_kind,'basisRevision',basis_revision::text,'deadlineAt',deadline_at,'clockRevision',clock_revision::text,'state',state,'afterGameId',after_game_id,'selectedDeckRevisionId',selected_deck_revision_id,'openedAt',opened_at,'deadlinePolicyRef',deadline_policy_ref,'deadlineRevision',deadline_revision::text,'deadlineAvailabilityRevision',deadline_availability_revision::text,'deadlineRemainingUs',deadline_remaining_us::text,'deadlineAccountedAt',deadline_accounted_at) body FROM app.between_game_decision WHERE match_id=p_match AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_competitive_results(p_after uuid,p_limit integer,p_format uuid,p_season uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF '00000000-0000-0000-0000-000000000000'::uuid IN(p_format,p_season) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT id,jsonb_build_object('id',id,'matchResultId',match_result_id,'formatId',format_id,'seasonId',season_id,'win',win,'completedAt',completed_at,'ratingBefore',rating_before::text,'ratingDelta',rating_delta::text,'ratingAfter',rating_after::text) body FROM app.competitive_result WHERE account_id=actor AND(p_after IS NULL OR id>p_after) AND(p_format IS NULL OR format_id=p_format) AND(p_season IS NULL OR season_id=p_season) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
CREATE FUNCTION app.read_my_daily_wins(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT id,jsonb_build_object('id',id,'utcDay',utc_day,'wins',wins::text,'issuedCoins',issued_coins::text,'revision',revision::text) body FROM app.daily_win_bucket WHERE account_id=actor AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;
REVOKE ALL ON FUNCTION app.read_my_matches(uuid,integer),app.read_match_metadata(uuid),app.read_match_games(uuid,uuid,integer),app.read_match_seats(uuid,uuid,integer),app.read_my_match_decisions(uuid,uuid,integer),app.read_my_competitive_results(uuid,integer,uuid,uuid),app.read_my_daily_wins(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_matches(uuid,integer),app.read_match_metadata(uuid),app.read_match_games(uuid,uuid,integer),app.read_match_seats(uuid,uuid,integer),app.read_my_match_decisions(uuid,uuid,integer),app.read_my_competitive_results(uuid,integer,uuid,uuid),app.read_my_daily_wins(uuid,integer) TO authenticated;
COMMIT;
