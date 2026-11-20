BEGIN;
-- These predicates bind the live caller once and bypass membership recursion.
-- Public rooms, AI sponsorship and knowledge of a match UUID grant no match access.
CREATE FUNCTION app.can_read_match(p_match uuid) RETURNS boolean LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();
BEGIN
 IF actor IS NULL THEN RETURN false;END IF;
 RETURN EXISTS(SELECT 1 FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id WHERE m.id=p_match AND m.started_at IS NOT NULL AND m.accepted_release_id IS NOT NULL AND seat.account_id=actor AND seat.controller_kind='human');
END $$;
CREATE FUNCTION app.can_read_match_seat(p_seat uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.match_seat WHERE id=p_seat AND app.can_read_match(match_id))
$$;
CREATE FUNCTION app.owns_match_seat(p_seat uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.match_seat WHERE id=p_seat AND controller_kind='human' AND account_id=app.current_account_id() AND app.can_read_match(match_id))
$$;
REVOKE ALL ON FUNCTION app.can_read_match(uuid),app.can_read_match_seat(uuid),app.owns_match_seat(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.can_read_match(uuid),app.can_read_match_seat(uuid),app.owns_match_seat(uuid) TO authenticated;
CREATE POLICY participant_match_read ON app.match FOR SELECT TO authenticated USING(app.can_read_match(id));
GRANT SELECT(id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision,accepted_release_id,started_at,current_game_id,reserved_game_id,availability,availability_revision) ON app.match TO authenticated;
CREATE POLICY participant_game_read ON app.game FOR SELECT TO authenticated USING(app.can_read_match(match_id));
GRANT SELECT ON app.game TO authenticated;
CREATE POLICY participant_seat_read ON app.match_seat FOR SELECT TO authenticated USING(app.can_read_match(match_id));
GRANT SELECT(id,match_id,account_id,seat_index,controller_kind) ON app.match_seat TO authenticated;
CREATE POLICY participant_clock_read ON app.match_clock FOR SELECT TO authenticated USING(app.can_read_match_seat(match_seat_id));
GRANT SELECT(id,match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at) ON app.match_clock TO authenticated;
CREATE POLICY own_between_decision_read ON app.between_game_decision FOR SELECT TO authenticated USING(app.owns_match_seat(seat_id));
GRANT SELECT(id,match_id,seat_id,decision_kind,basis_revision,deadline_at,clock_revision,state,after_game_id,selected_deck_revision_id,opened_at,deadline_policy_ref,deadline_revision,deadline_availability_revision,deadline_remaining_us,deadline_accounted_at) ON app.between_game_decision TO authenticated;
CREATE POLICY participant_match_result_read ON app.match_result FOR SELECT TO authenticated USING(app.can_read_match(match_id));
GRANT SELECT(id,match_id,outcome,committed_at,ranking_format_id,rank_sequence,winner_seat_ids,final_game_ids,terminal_kind) ON app.match_result TO authenticated;
GRANT SELECT ON app.match_decision_deadline TO authenticated;

CREATE POLICY own_queue_read ON app.queue_entry FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT(id,account_id,deck_revision_id,mode,competitive,reserved_match_id,state,alternative_index,format_id,claimed_room_id) ON app.queue_entry TO authenticated;
CREATE POLICY issuer_invitation_read ON app.invitation FOR SELECT TO authenticated USING(issuer_id=app.current_account_id());
GRANT SELECT(id,room_id,issuer_id,state) ON app.invitation TO authenticated;
CREATE POLICY own_quest_read ON app.quest_assignment FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT(id,account_id,definition_id,source_day,progress,completed_at,replaced_by_id,assigned_at) ON app.quest_assignment TO authenticated;
CREATE POLICY own_daily_quest_read ON app.daily_quest_state FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT ON app.daily_quest_state TO authenticated;
CREATE POLICY qualified_quest_definition_read ON app.quest_definition_revision FOR SELECT TO authenticated USING(app.current_account_id() IS NOT NULL AND fact_kind IS NOT NULL AND fact_schema_revision IS NOT NULL AND objective_qualification_ref IS NOT NULL);
GRANT SELECT(id,definition_key,revision,objective_ref,target,reward_coins) ON app.quest_definition_revision TO authenticated;
CREATE POLICY own_rank_read ON app.rank FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT(id,account_id,format_id,season_id,tier,rating,revision,finalized) ON app.rank TO authenticated;
CREATE POLICY own_competitive_credit_read ON app.competitive_result FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT(id,account_id,match_result_id,format_id,win,completed_at,season_id,rating_before,rating_delta,rating_after) ON app.competitive_result TO authenticated;
CREATE POLICY own_win_bucket_read ON app.daily_win_bucket FOR SELECT TO authenticated USING(account_id=app.current_account_id());
GRANT SELECT ON app.daily_win_bucket TO authenticated;
CREATE POLICY season_calendar_read ON app.season FOR SELECT TO authenticated USING(app.current_account_id() IS NOT NULL);
GRANT SELECT(id,year,quarter,start_at,end_at,state) ON app.season TO authenticated;
GRANT SELECT ON app.quest_progress,app.season_rank_snapshot TO authenticated;
COMMIT;
