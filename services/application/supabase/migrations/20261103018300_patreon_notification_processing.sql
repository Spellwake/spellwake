BEGIN;
-- Membership notifications are authenticated hints, never payment evidence.
-- A current-account refresh uses the notification UUID as its recovery key.
CREATE FUNCTION app.read_patreon_notification_route(p_event uuid) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; m app.patreon_membership; lifecycle text; refresh jsonb;
BEGIN
 PERFORM app.patreon_notification_id(p_event);
 SELECT * INTO e FROM app.provider_event WHERE id=p_event AND provider='patreon' AND event_type='member.notification';
 IF e.id IS NULL THEN RETURN NULL;END IF;
 PERFORM app.patreon_notification_capture(e.capture);
 IF e.processing_state='processed' THEN
 IF e.effect->>'kind' IS DISTINCT FROM 'patreon_notification_v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification lacks canonical processing source';END IF;
 RETURN jsonb_build_object('eventId',e.id,'action','done','result',e.effect->'result');END IF;
 IF e.processing_state<>'received' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification processing state is unavailable';END IF;
 SELECT * INTO m FROM app.patreon_membership WHERE patreon_user_ref=e.capture->>'userId' AND patreon_campaign_ref=e.capture->>'campaignId';
 IF m.id IS NULL THEN RETURN jsonb_build_object('eventId',e.id,'action','finish','associationId',NULL,'requestId',e.id,'ticket',NULL);END IF;
 SELECT state INTO lifecycle FROM app.account WHERE id=m.account_id;
 IF lifecycle<>'active' OR m.patreon_member_ref IS DISTINCT FROM e.capture->>'memberId' THEN RETURN jsonb_build_object('eventId',e.id,'action','finish','associationId',m.id,'requestId',e.id,'ticket',NULL);END IF;
 refresh:=app.read_patreon_refresh_result(m.id,e.id);
 IF refresh IS NOT NULL THEN RETURN jsonb_build_object('eventId',e.id,'action','finish','associationId',m.id,'requestId',e.id,'ticket',NULL);END IF;
 RETURN jsonb_build_object('eventId',e.id,'action','refresh','associationId',m.id,'requestId',e.id,'ticket',app.read_patreon_refresh(m.id));
END $$;
CREATE FUNCTION app.process_patreon_notification(p_event uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE e app.provider_event; m app.patreon_membership; discovered uuid; lifecycle text; refresh jsonb; refresh_source app.provider_event; disposition text; source_time timestamptz; result jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='notification processing requires read committed retry';END IF;
 PERFORM app.patreon_notification_id(p_event);
 SELECT * INTO e FROM app.provider_event WHERE id=p_event AND provider='patreon' AND event_type='member.notification';
 IF e.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='notification source unavailable';END IF;
 PERFORM app.patreon_notification_capture(e.capture);
 SELECT * INTO m FROM app.patreon_membership WHERE patreon_user_ref=e.capture->>'userId' AND patreon_campaign_ref=e.capture->>'campaignId';discovered:=m.id;
 IF m.id IS NOT NULL THEN SELECT state INTO lifecycle FROM app.account WHERE id=m.account_id FOR NO KEY UPDATE;END IF;
 -- Same stable user fence as088. The unlinked branch never acquires an account
 -- after this fence: a newly discovered association causes a whole retry.
 PERFORM pg_advisory_xact_lock(hashtextextended('patreon-user:'||(e.capture->>'userId'),0));
 SELECT * INTO m FROM app.patreon_membership WHERE patreon_user_ref=e.capture->>'userId' AND patreon_campaign_ref=e.capture->>'campaignId' FOR UPDATE;
 IF m.id IS DISTINCT FROM discovered THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='notification association changed before account fence';END IF;
 SELECT * INTO e FROM app.provider_event WHERE id=p_event FOR UPDATE;
 IF e.processing_state='processed' THEN
 IF e.effect->>'kind' IS DISTINCT FROM 'patreon_notification_v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification lacks canonical processing source';END IF;RETURN e.effect->'result';END IF;
 IF e.processing_state<>'received' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification processing state is unavailable';END IF;
 IF m.id IS NULL THEN disposition:='ignored_unlinked';
 ELSIF lifecycle<>'active' THEN disposition:='ignored_inactive';
 ELSIF m.patreon_member_ref IS DISTINCT FROM e.capture->>'memberId' THEN disposition:='ignored_stale_member';
 ELSE
 refresh:=app.read_patreon_refresh_result(m.id,e.id);
 IF refresh IS NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='notification waits for its fresh authoritative membership observation';END IF;
 SELECT * INTO refresh_source FROM app.provider_event WHERE id::text=refresh->>'sourceEventId';
 IF refresh_source.received_at<e.received_at OR refresh_source.capture->>'memberId' IS DISTINCT FROM e.capture->>'memberId' OR refresh_source.capture->>'userId' IS DISTINCT FROM e.capture->>'userId' OR refresh_source.capture->>'campaignId' IS DISTINCT FROM e.capture->>'campaignId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification refresh source differs';END IF;
 disposition:='refreshed';
 END IF;
 source_time:=clock_timestamp();
 IF source_time<e.received_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='notification source clock moved before intake';END IF;
 result:=jsonb_build_object('eventId',e.id,'disposition',disposition,'associationId',m.id,'refreshSourceId',refresh_source.id);
 UPDATE app.provider_event SET account_id=m.account_id,processing_state='processed',effect_key='patreon-notification:'||e.id::text,
 effect=jsonb_build_object('kind','patreon_notification_v1','processedAt',to_char(source_time AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'associationId',m.id,'associationSourceId',m.source_event_id,'accountState',lifecycle,'memberRef',m.patreon_member_ref,'disposition',disposition,'refreshSourceId',refresh_source.id,'result',result) WHERE id=e.id;
 RETURN result;
END $$;
CREATE FUNCTION app.check_patreon_notification_processing(p_event uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; m app.patreon_membership; parent app.provider_event; parent_capture jsonb; refresh app.provider_event; processed timestamptz; disposition text; expected jsonb; player app.account; observed_state text;
BEGIN
 SELECT * INTO e FROM app.provider_event WHERE id=p_event;
 IF e.id IS NULL THEN RETURN;END IF;
 IF e.provider='patreon' AND e.event_type='member.notification' AND e.processing_state='processed' AND e.effect->>'kind' IS DISTINCT FROM 'patreon_notification_v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='processed notification requires canonical disposition';END IF;
 IF e.effect->>'kind' IS DISTINCT FROM 'patreon_notification_v1' THEN RETURN;END IF;
 PERFORM app.patreon_notification_capture(e.capture);
 IF e.provider<>'patreon' OR e.event_type<>'member.notification' OR e.processing_state<>'processed' OR e.external_event_ref<>'notification:'||e.id::text OR e.effect_key<>'patreon-notification:'||e.id::text OR e.payload_ref<>'payments/patreon/'||e.id::text||'/'||(e.capture->>'bodyHash')||'.json' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification processing source envelope differs';END IF;
 processed:=app.financial_instant(e.effect->>'processedAt');disposition:=e.effect->>'disposition';
 IF processed<e.received_at OR processed>clock_timestamp() OR disposition NOT IN('ignored_unlinked','ignored_inactive','ignored_stale_member','refreshed') OR disposition IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification disposition or source time differs';END IF;
 SELECT * INTO m FROM app.patreon_membership WHERE id::text=e.effect->>'associationId';
 IF disposition='ignored_unlinked' THEN
 IF m.id IS NOT NULL OR e.account_id IS NOT NULL OR e.effect->>'associationId' IS NOT NULL OR e.effect->>'associationSourceId' IS NOT NULL OR e.effect->>'accountState' IS NOT NULL OR e.effect->>'memberRef' IS NOT NULL
 OR EXISTS(SELECT 1 FROM app.provider_event linked WHERE linked.provider='patreon' AND linked.event_type='oauth.association' AND linked.capture->>'userId'=e.capture->>'userId' AND linked.capture->>'campaignId'=e.capture->>'campaignId' AND linked.received_at<=processed) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unlinked notification contradicts association history';END IF;
 ELSE
 SELECT * INTO parent FROM app.provider_event WHERE id::text=e.effect->>'associationSourceId';parent_capture:=parent.capture;
 IF parent.event_type='application.retirement' THEN SELECT capture INTO parent_capture FROM app.provider_event WHERE id::text=parent.capture->>'parentEventId';END IF;
 IF m.id IS NULL OR e.account_id IS DISTINCT FROM m.account_id OR m.patreon_user_ref IS DISTINCT FROM e.capture->>'userId' OR m.patreon_campaign_ref IS DISTINCT FROM e.capture->>'campaignId' OR parent.provider IS DISTINCT FROM 'patreon' OR parent.account_id IS DISTINCT FROM m.account_id OR parent.processing_state IS DISTINCT FROM 'processed' OR parent.event_type NOT IN('oauth.association','api.membership_refresh','application.retirement') OR parent.effect->>'associationId' IS DISTINCT FROM m.id::text OR parent_capture->>'memberId' IS DISTINCT FROM e.effect->>'memberRef' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification association source differs';END IF;
 SELECT * INTO player FROM app.account WHERE id=m.account_id;
 observed_state:=CASE WHEN player.deletion_finalized_at IS NOT NULL AND player.deletion_finalized_at<=processed THEN 'deleted' WHEN player.deletion_requested_at IS NOT NULL AND player.deletion_requested_at<=processed THEN 'deletion_pending' ELSE 'active' END;
 IF e.effect->>'accountState' IS DISTINCT FROM observed_state THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification lifecycle contradicts account source time';END IF;
 IF disposition='ignored_inactive' AND (e.effect->>'accountState' IS NULL OR e.effect->>'accountState' NOT IN('deletion_pending','deleted')) OR disposition IN('refreshed','ignored_stale_member') AND e.effect->>'accountState' IS DISTINCT FROM 'active' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification lifecycle disposition differs';END IF;
 IF disposition='ignored_stale_member' AND e.effect->>'memberRef' IS NOT DISTINCT FROM e.capture->>'memberId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='stale notification requires a distinct member generation';END IF;
 END IF;
 SELECT * INTO refresh FROM app.provider_event WHERE id::text=e.effect->>'refreshSourceId';
 IF disposition='refreshed' THEN
 IF refresh.id IS NULL OR refresh.external_event_ref IS DISTINCT FROM 'refresh:'||m.id::text||':'||e.id::text OR refresh.received_at<e.received_at OR refresh.capture->>'memberId' IS DISTINCT FROM e.capture->>'memberId' OR refresh.capture->>'userId' IS DISTINCT FROM e.capture->>'userId' OR refresh.capture->>'campaignId' IS DISTINCT FROM e.capture->>'campaignId' OR (app.read_patreon_refresh_result(m.id,e.id)->>'sourceEventId') IS DISTINCT FROM refresh.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification lacks its own fresh accepted observation';END IF;
 ELSIF e.effect->>'refreshSourceId' IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ignored notification cannot invent a refresh source';END IF;
 expected:=jsonb_build_object('kind','patreon_notification_v1','processedAt',e.effect->>'processedAt','associationId',m.id,'associationSourceId',parent.id,'accountState',e.effect->>'accountState','memberRef',e.effect->>'memberRef','disposition',disposition,'refreshSourceId',refresh.id,'result',jsonb_build_object('eventId',e.id,'disposition',disposition,'associationId',m.id,'refreshSourceId',refresh.id));
 IF e.effect IS DISTINCT FROM expected THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='notification processing projection differs';END IF;
END $$;
CREATE FUNCTION app.guard_patreon_notification_processing() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN PERFORM app.check_patreon_notification_processing(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER patreon_notification_processing AFTER INSERT OR UPDATE ON app.provider_event DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_patreon_notification_processing();
REVOKE ALL ON FUNCTION app.read_patreon_notification_route(uuid),app.process_patreon_notification(uuid),app.check_patreon_notification_processing(uuid),app.guard_patreon_notification_processing() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.patreon_retirement_blocked(p_account uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.patreon_membership m JOIN app.provider_event source ON source.id=m.source_event_id WHERE m.account_id=p_account AND (m.patreon_user_ref IS NULL OR source.processing_state<>'processed' OR source.event_type NOT IN('oauth.association','api.membership_refresh') OR source.capture->>'userId' IS DISTINCT FROM m.patreon_user_ref OR source.capture->>'memberId' IS DISTINCT FROM m.patreon_member_ref OR source.capture->>'campaignId' IS DISTINCT FROM m.patreon_campaign_ref))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='patreon' AND e.processing_state IN('received','verified') AND (e.account_id=p_account OR EXISTS(SELECT 1 FROM app.patreon_membership m WHERE m.account_id=p_account AND (m.patreon_member_ref IN(e.capture->>'memberRef',e.capture->>'memberId',e.capture->>'targetId') OR m.patreon_user_ref=e.capture->>'userId'))))
 OR EXISTS(SELECT 1 FROM app.provider_event e WHERE e.provider='patreon' AND e.account_id=p_account AND e.processing_state='processed' AND (e.event_type NOT IN('oauth.association','api.membership_refresh','member.notification','financial.observation','application.retirement') OR e.capture IS NULL OR e.effect IS NULL OR (e.event_type='member.notification' AND e.effect->>'kind' IS DISTINCT FROM 'patreon_notification_v1')))
 OR EXISTS(SELECT 1 FROM app.patreon_payment payment JOIN app.patreon_membership member ON member.id=payment.membership_id WHERE member.account_id=p_account AND NOT EXISTS(SELECT 1 FROM app.provider_event e WHERE e.id=payment.provider_event_id AND e.provider='patreon' AND e.account_id=p_account AND e.processing_state='processed' AND e.event_type='financial.observation' AND e.capture->>'family'='patreon-financial-v1' AND e.capture->>'paymentRef'=payment.payment_ref AND e.capture->>'state'=payment.state AND e.effect->>'kind'='patreon_financial_v1' AND e.effect->>'membershipId'=member.id::text AND e.effect->'result'->>'paymentId'=payment.id::text AND e.effect->'result'->>'state'=payment.state))
$$;
COMMIT;
