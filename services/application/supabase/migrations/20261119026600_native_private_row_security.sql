BEGIN;

ALTER TABLE app.native_response_adoption FORCE ROW LEVEL SECURITY;
ALTER TABLE app.native_opening_selection FORCE ROW LEVEL SECURITY;
ALTER TABLE app.native_legacy_chooser_report FORCE ROW LEVEL SECURITY;
ALTER TABLE app.native_entropy_seed FORCE ROW LEVEL SECURITY;
ALTER TABLE app.native_host_watch FORCE ROW LEVEL SECURITY;

COMMIT;
