BEGIN;
-- Keep only the existing minimal immutable provider routing/accounting IDs.
-- Internal retirement is a distinct one-way source, never a provider payment.
CREATE FUNCTION app.valid_patreon_retirement_source(p_member app.patreon_membership,p_allow_pending boolean DEFAULT false) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.provider_event e JOIN app.provider_event parent ON parent.id::text=e.capture->>'parentEventId' JOIN app.command_receipt r ON r.id::text=e.capture->>'retirementReceiptId' JOIN app.account a ON a.id=p_member.account_id JOIN app.ledger_operation operation ON operation.id=a.retirement_operation_id
 WHERE e.id=p_member.source_event_id AND NOT p_member.active AND p_member.active_tier_id IS NULL AND p_member.patreon_user_ref IS NOT NULL AND e.provider='patreon' AND e.account_id=a.id AND e.event_type='application.retirement' AND e.processing_state='processed' AND e.raw_evidence_state='none'
 AND a.state='deleted' AND operation.state='committed' AND operation.kind='retirement' AND operation.source_scope='account_retirement' AND operation.source_ref=a.id::text AND operation.actor_account_id=a.id
 AND r.scope='account_retirement_v1' AND r.principal_key='human:'||a.id::text AND r.account_id=a.id AND r.target_ref=a.id::text AND r.request_ref=a.id::text AND r.operation_id=operation.id AND (r.state='accepted' OR (p_allow_pending AND r.state='pending'))
 AND parent.provider='patreon' AND parent.account_id=a.id AND parent.processing_state='processed' AND parent.event_type IN('oauth.association','api.membership_refresh') AND parent.payload_ref='sha256:'||app.auth_json_hash(parent.capture)
 AND (app.patreon_proof(parent.capture) IS NOT NULL OR app.patreon_refresh_proof(parent.capture) IS NOT NULL)
 AND parent.capture->>'userId'=p_member.patreon_user_ref AND parent.capture->>'memberId'=p_member.patreon_member_ref AND parent.capture->>'campaignId'=p_member.patreon_campaign_ref
 AND e.capture=jsonb_build_object('version','patreon-retirement-v1','accountId',a.id,'associationId',p_member.id,'retirementReceiptId',r.id,'retirementOperationId',operation.id,'parentEventId',parent.id,'parentProofHash',app.auth_json_hash(parent.capture))
 AND e.payload_ref='sha256:'||app.auth_json_hash(e.capture) AND e.external_event_ref='retirement:'||r.id::text||':'||p_member.id::text
 AND e.effect=jsonb_build_object('kind','patreon_retirement_v1','associationId',p_member.id,'retirementReceiptId',r.id,'parentEventId',parent.id))
$$;
CREATE FUNCTION app.retire_patreon_memberships(p_account uuid,p_retirement uuid) RETURNS uuid[] LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE member app.patreon_membership; parent app.provider_event; source jsonb; event_id uuid; events uuid[]:='{}'::uuid[]; operation uuid;
BEGIN
 SELECT retirement_operation_id INTO operation FROM app.account WHERE id=p_account FOR UPDATE;
 PERFORM app.check_registered_auth_retirement(p_account,operation);
 FOR member IN SELECT * FROM app.patreon_membership WHERE account_id=p_account ORDER BY id FOR UPDATE LOOP
 SELECT * INTO STRICT parent FROM app.provider_event WHERE id=member.source_event_id;
 IF parent.event_type='application.retirement' AND parent.capture->>'retirementReceiptId'=p_retirement::text AND app.valid_patreon_retirement_source(member,true) THEN events:=array_append(events,parent.id);CONTINUE;END IF;
 IF parent.provider<>'patreon' OR parent.account_id IS DISTINCT FROM p_account OR parent.processing_state<>'processed' OR parent.event_type NOT IN('oauth.association','api.membership_refresh') OR member.patreon_user_ref IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement requires qualified retained Patreon association';END IF;
 source:=jsonb_build_object('version','patreon-retirement-v1','accountId',p_account,'associationId',member.id,'retirementReceiptId',p_retirement,'retirementOperationId',operation,'parentEventId',parent.id,'parentProofHash',app.auth_json_hash(parent.capture));
 INSERT INTO app.provider_event(provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state,capture,raw_evidence_state,effect,effect_key)
 VALUES('patreon','retirement:'||p_retirement::text||':'||member.id::text,p_account,clock_timestamp(),'application.retirement','sha256:'||app.auth_json_hash(source),'processed',source,'none',jsonb_build_object('kind','patreon_retirement_v1','associationId',member.id,'retirementReceiptId',p_retirement,'parentEventId',parent.id),'patreon-retirement:'||p_retirement::text||':'||member.id::text) RETURNING id INTO event_id;
 UPDATE app.patreon_membership SET active=false,active_tier_id=NULL,source_event_id=event_id WHERE id=member.id;
 events:=array_append(events,event_id);
 END LOOP;
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO events FROM unnest(events) id;RETURN events;
END $$;
CREATE OR REPLACE FUNCTION app.guard_membership_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event; ticket jsonb; tier uuid;
BEGIN
 IF TG_OP='DELETE' THEN RETURN app.patreon_oauth_membership_source(NEW,OLD,TG_OP); END IF;
 SELECT * INTO e FROM app.provider_event WHERE id=NEW.source_event_id;
 IF e.event_type='application.retirement' THEN
 IF TG_OP<>'UPDATE' OR (NEW.id,NEW.account_id,NEW.patreon_member_ref,NEW.patreon_user_ref,NEW.patreon_campaign_ref) IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.patreon_member_ref,OLD.patreon_user_ref,OLD.patreon_campaign_ref) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='retirement cannot create or reassign a membership';END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 IF NEW IS NOT DISTINCT FROM OLD AND app.valid_patreon_retirement_source(NEW,true) THEN RETURN NEW;END IF;
 IF e.capture->>'parentEventId' IS DISTINCT FROM OLD.source_event_id::text OR NOT app.valid_patreon_retirement_source(NEW,true) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon retirement requires its exact one-way source';END IF;RETURN NEW;
 END IF;

 IF e.event_type IS DISTINCT FROM 'api.membership_refresh' THEN RETURN app.patreon_oauth_membership_source(NEW,OLD,TG_OP); END IF;
 IF TG_OP<>'UPDATE' OR (NEW.id,NEW.account_id,NEW.patreon_member_ref,NEW.patreon_user_ref,NEW.patreon_campaign_ref)
 IS DISTINCT FROM (OLD.id,OLD.account_id,OLD.patreon_member_ref,OLD.patreon_user_ref,OLD.patreon_campaign_ref) OR OLD.patreon_user_ref IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh cannot change membership identity'; END IF;
 PERFORM id FROM app.account WHERE id=NEW.account_id FOR NO KEY UPDATE;
 PERFORM app.check_provider_source(e.id,NEW.account_id,'patreon');
 ticket:=app.patreon_refresh_source(NEW,e,true); tier:=app.patreon_entitled_tier(e.capture);
 IF ticket->>'expectedSourceId' IS DISTINCT FROM OLD.source_event_id::text OR NEW.active_tier_id IS DISTINCT FROM tier
 OR NEW.active IS DISTINCT FROM (tier IS NOT NULL) OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=NEW.account_id AND state='active') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh requires exact current parent and tier'; END IF;
 RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION app.check_patreon_association_commit() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE e app.provider_event;
BEGIN
 SELECT * INTO e FROM app.provider_event WHERE id=NEW.source_event_id;
 IF e.event_type='application.retirement' THEN
 IF NOT app.valid_patreon_retirement_source(NEW) OR (app.read_account_retirement_proof(NEW.account_id,(e.capture->>'retirementOperationId')::uuid)).id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='Patreon retirement requires complete canonical minimization';END IF;RETURN NULL;
 END IF;

 IF e.event_type='api.membership_refresh' THEN
  PERFORM app.patreon_refresh_source(NEW,e);
  IF NEW.active_tier_id IS DISTINCT FROM app.patreon_entitled_tier(e.capture)
  OR NEW.active IS DISTINCT FROM (NEW.active_tier_id IS NOT NULL) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='refresh mapped tier changed before commit'; END IF;
 ELSE PERFORM app.patreon_oauth_commit_source(NEW); END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION app.valid_patreon_retirement_source(app.patreon_membership,boolean),app.retire_patreon_memberships(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
