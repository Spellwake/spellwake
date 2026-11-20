BEGIN;
-- Exact reviewed trusted server capabilities. No browser/native installation
-- receives a service-role key. Subject/session and controller authority wrappers
-- still independently verify their actor; worker operations retain canonical
-- original/source/fence ownership. Raw writers, helpers and all table DML stay shut.

GRANT EXECUTE ON FUNCTION app.accept_native_opening(uuid,numeric,uuid,text,jsonb,text,text,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.claim_matchmaking(jsonb,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.commit_ai_ready_confirmation(uuid,numeric,uuid,text,text,uuid,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.commit_native_host_input(uuid,uuid,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.consume_game_outcome(uuid,numeric) TO service_role;
GRANT EXECUTE ON FUNCTION app.expire_between_game_deadline(uuid,numeric,uuid,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.finalize_unrecoverable_match(uuid,numeric,numeric,uuid,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.issue_ai_grant(uuid,numeric,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.issue_native_cursor_source(uuid,uuid,text,text,uuid,numeric,text,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.observe_ai_grant_delivery(uuid,numeric,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.observe_ai_provider(uuid,numeric,uuid,text,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.observe_ai_stop(uuid,numeric,uuid,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.pause_game_incident(uuid,text,numeric,numeric,timestamptz,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.pause_match_between_games(uuid,numeric,numeric,timestamptz,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.publish_game_replay(uuid,numeric,uuid,numeric,text,text,text,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.publish_native_view_source(uuid,uuid,text,text,uuid,text,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.qualify_between_game_command(uuid,uuid,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.qualify_controller_between_game_command(uuid,numeric,text,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.qualify_room_countdown_command(uuid,uuid,uuid,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.qualify_room_ready_command(uuid,uuid,uuid,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_ai_start_work(uuid,numeric) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_ai_stop_work_page(uuid,uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_due_between_game_deadlines(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_ai_binding_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_archive_source(uuid,uuid,uuid,numeric,integer,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_controller_access(uuid,numeric,text,uuid,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_controller_receipts(uuid,numeric,text,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_countdown_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_cursor_source(uuid,uuid,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_host_receipt(uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_human_access(uuid,uuid,uuid,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_human_receipts(uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_match_recovery_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_opening_source(uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_queue_intent_source(uuid,numeric,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_queue_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_recovery_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_unscheduled_decision_work(uuid,integer) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_view_source(uuid,uuid,text,text,uuid,text) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_qualified_between_game_command(uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_qualified_controller_between_game_command(uuid,numeric,text,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_qualified_room_countdown_command(uuid,uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_qualified_room_ready_command(uuid,uuid,uuid,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.rebind_ai_runner(uuid,numeric,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.restore_game_checkpoint(uuid,numeric,numeric,uuid,text,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.resume_match_between_games(uuid,numeric,numeric,uuid,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.schedule_between_game_deadline(uuid,numeric,numeric,numeric,numeric,uuid,text,timestamptz,text,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.start_room_match(uuid,numeric,uuid,uuid,text,jsonb,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.submit_native_controller_command(uuid,numeric,text,jsonb,jsonb) TO service_role;
GRANT EXECUTE ON FUNCTION app.submit_native_human_command(uuid,uuid,jsonb,jsonb) TO service_role;

GRANT EXECUTE ON FUNCTION app.read_native_room_source(uuid,numeric,uuid,uuid) TO service_role;
GRANT EXECUTE ON FUNCTION app.read_native_between_source(uuid,numeric,uuid) TO service_role;

COMMIT;
