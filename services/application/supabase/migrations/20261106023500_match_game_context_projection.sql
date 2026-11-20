BEGIN;
-- Existing participant history exposes only the public context UUID locator.
-- UNIQUE(game_id) preserves one page row per game. No source refs or grants change.
CREATE OR REPLACE FUNCTION app.read_match_games(p_match uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 IF p_match IS NULL OR p_match='00000000-0000-0000-0000-000000000000'::uuid OR NOT app.can_read_match(p_match) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(SELECT g.id,jsonb_build_object('id',g.id,'contextId',ctx.id,'gameIndex',g.game_index::text,'state',g.state,'acceptedRevision',g.accepted_revision::text,'result',CASE WHEN r.id IS NOT NULL THEN jsonb_build_object('id',r.id,'outcome',r.outcome,'winnerSeatIds',r.winner_seat_ids,'endReason',r.end_reason) END) body FROM app.game g LEFT JOIN app.game_context ctx ON ctx.game_id=g.id LEFT JOIN app.game_result r ON r.game_id=g.id WHERE g.match_id=p_match AND(p_after IS NULL OR g.id>p_after) ORDER BY g.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'matchId',p_match,'items',items,'nextCursor',cursor_id));
END $$;
COMMIT;
