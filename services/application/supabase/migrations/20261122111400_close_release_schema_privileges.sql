BEGIN;

-- The deferred trigger still runs after the settlement RPC has returned to its
-- caller. Keep its invariant checks able to read private settlement records.
ALTER FUNCTION app.check_exchange_conservation() SECURITY DEFINER;
REVOKE ALL ON FUNCTION app.check_exchange_conservation()
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Stripe customer identifiers are exposed through the caller-scoped RPC.
REVOKE SELECT ON app.account FROM service_role;
GRANT SELECT (id, auth_subject, username, sign_in_method, created_at, state,
  deletion_requested_at, retirement_operation_id, deletion_finalized_at,
  social_revision, appearance_revision, auth_cleanup_subject)
  ON app.account TO service_role;

-- Apply the existing application RLS boundary to the newly added tables.
ALTER TABLE app.commander_pilot_member FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_item FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_pack FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_pick FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_seat FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_source FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_match_seat_pool FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_pilot_member FORCE ROW LEVEL SECURITY;
ALTER TABLE app.limited_private_profile FORCE ROW LEVEL SECURITY;
ALTER TABLE app.private_pilot_profile FORCE ROW LEVEL SECURITY;
ALTER TABLE app.trade_session FORCE ROW LEVEL SECURITY;
ALTER TABLE app.trade_session_item FORCE ROW LEVEL SECURITY;

COMMIT;
