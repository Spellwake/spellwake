BEGIN;
-- Authorization is rechecked after account-lock waits. statement_timestamp() is
-- frozen before that wait and can incorrectly admit a now-expired session.
-- Capture one actual observation time per invocation and read current Auth rows.
CREATE OR REPLACE FUNCTION app.auth_session_is_live(p_subject uuid,p_session uuid,p_class text) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE observed_at timestamptz:=clock_timestamp();
BEGIN
 RETURN EXISTS(
  SELECT 1 FROM auth.sessions s JOIN auth.users u ON u.id=s.user_id
  WHERE s.id=p_session AND s.user_id=p_subject AND p_class IN ('otp','oauth')
   AND u.deleted_at IS NULL AND (u.banned_until IS NULL OR u.banned_until<=observed_at)
   AND (s.not_after IS NULL OR s.not_after>observed_at)
   AND EXISTS(SELECT 1 FROM auth.mfa_amr_claims c WHERE c.session_id=s.id AND c.authentication_method=p_class)
   AND NOT EXISTS(SELECT 1 FROM auth.mfa_amr_claims c WHERE c.session_id=s.id AND c.authentication_method IS DISTINCT FROM p_class)
   AND (
    (s.refresh_token_hmac_key IS NULL AND s.refresh_token_counter IS NULL
     AND EXISTS(SELECT 1 FROM auth.refresh_tokens r WHERE r.session_id=s.id AND r.user_id=s.user_id::text AND r.revoked=false))
    OR (length(s.refresh_token_hmac_key)>0 AND s.refresh_token_counter>=0)
   )
 );
END $$;
REVOKE ALL ON FUNCTION app.auth_session_is_live(uuid,uuid,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
