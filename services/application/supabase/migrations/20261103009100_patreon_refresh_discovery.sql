BEGIN;
-- Candidates grant no admission authority. Every provider GET still needs090 preparation.
CREATE FUNCTION app.read_patreon_refresh_work(p_campaign text,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_campaign IS NULL OR length(p_campaign) NOT BETWEEN 1 AND 128
 OR p_campaign COLLATE "C" !~ '^[A-Za-z0-9_-]+$'
 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100
 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid refresh discovery bounds'; END IF;
 RETURN (SELECT coalesce(jsonb_agg(candidate.id ORDER BY candidate.id),'[]'::jsonb)
 FROM (SELECT m.id FROM app.patreon_membership m JOIN app.account a ON a.id=m.account_id
  WHERE a.state='active' AND m.patreon_campaign_ref=p_campaign AND m.patreon_user_ref IS NOT NULL
  AND (p_after IS NULL OR m.id>p_after) ORDER BY m.id LIMIT p_limit) candidate);
END $$;

-- Historical acknowledgment, not current membership or financial authority.
-- The facade's reads are statement-stable. Inherited OAuth validation includes a
-- VOLATILE helper and instead relies on immutable admitted source/receipt provenance.
CREATE FUNCTION app.read_patreon_refresh_result(p_association uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE m app.patreon_membership; e app.provider_event;
BEGIN
 PERFORM app.patreon_notification_id(p_association);
 PERFORM app.patreon_notification_id(p_request);
 SELECT * INTO m FROM app.patreon_membership WHERE id=p_association;
 IF m.id IS NULL OR m.patreon_user_ref IS NULL OR m.patreon_campaign_ref IS NULL THEN RETURN NULL; END IF;
 SELECT * INTO e FROM app.provider_event WHERE provider='patreon'
 AND external_event_ref='refresh:'||p_association::text||':'||p_request::text;
 IF e.id IS NULL THEN RETURN NULL; END IF;
 m.patreon_member_ref:=e.capture->>'memberId'; m.source_event_id:=e.id;
 PERFORM app.patreon_refresh_source(m,e);
 RETURN jsonb_build_object('associationId',m.id,'sourceEventId',e.id,'outcome','applied');
END $$;

REVOKE ALL ON FUNCTION app.read_patreon_refresh_work(text,uuid,integer),app.read_patreon_refresh_result(uuid,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_patreon_refresh_work(text,uuid,integer),app.read_patreon_refresh_result(uuid,uuid) TO service_role;
COMMIT;
