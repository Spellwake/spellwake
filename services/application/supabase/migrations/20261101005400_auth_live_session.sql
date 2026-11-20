BEGIN;

-- Shared private predicate. It does not confer binding or enable client access.
-- Initial activation pins single-user/timebox/inactivity/low-AAL policies off.
CREATE FUNCTION app.auth_session_is_live(p_subject uuid,p_session uuid,p_class text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(
  SELECT 1 FROM auth.sessions s JOIN auth.users u ON u.id=s.user_id
  WHERE s.id=p_session AND s.user_id=p_subject AND p_class IN ('otp','oauth')
   AND u.deleted_at IS NULL AND (u.banned_until IS NULL OR u.banned_until<=statement_timestamp())
   AND (s.not_after IS NULL OR s.not_after>statement_timestamp())
   AND EXISTS(SELECT 1 FROM auth.mfa_amr_claims c WHERE c.session_id=s.id AND c.authentication_method=p_class)
   AND NOT EXISTS(SELECT 1 FROM auth.mfa_amr_claims c WHERE c.session_id=s.id AND c.authentication_method IS DISTINCT FROM p_class)
   AND (
    (s.refresh_token_hmac_key IS NULL AND s.refresh_token_counter IS NULL
     AND EXISTS(SELECT 1 FROM auth.refresh_tokens r WHERE r.session_id=s.id AND r.user_id=s.user_id::text AND r.revoked=false))
    OR (length(s.refresh_token_hmac_key)>0 AND s.refresh_token_counter>=0)
   )
 )
$$;
REVOKE EXECUTE ON FUNCTION app.auth_session_is_live(uuid,uuid,text) FROM PUBLIC,anon,authenticated,service_role;

COMMIT;
