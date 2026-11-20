BEGIN;

CREATE OR REPLACE FUNCTION app.qualify_native_room_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb,p_kind text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;parent app.command_receipt;child app.command_receipt;lobby app.room;
 request_id uuid;basis numeric;release_id uuid;legality_id uuid;revision_id uuid;scope_name text;
 full_prepared jsonb; full_policy jsonb; policy_rejected boolean:=false;observed_policy jsonb;source jsonb;terms jsonb;response jsonb;own_slot jsonb;key text;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified room command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_kind IS NULL OR p_kind NOT IN('ready','countdown') OR NOT app.native_room_intent_is_valid(p_room,p_original) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(p_original->>'requestId')::uuid;basis:=(p_original->>'expectedRevision')::numeric;
 scope_name:=CASE p_kind WHEN 'ready' THEN 'qualified_ready_intent_v1' ELSE 'qualified_countdown_intent_v1' END;
 parent:=app.begin_account_command(player.id,request_id,scope_name,p_room::text,jsonb_build_object('room',p_room,'original',p_original));
 -- Current caller authority, then exact original before new preparation/source.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152
  OR p_prepared->>'version' IS DISTINCT FROM (CASE p_kind WHEN 'ready' THEN 'native-ready-prepared-v1' ELSE 'native-countdown-prepared-v1' END)
  OR EXISTS(SELECT 1 FROM unnest(ARRAY['sourceHash','releaseId','legalityRevisionId','qualificationRef']) k WHERE jsonb_typeof(p_prepared->k) IS DISTINCT FROM 'string')
  OR p_prepared->>'sourceHash' !~ '^[0-9a-f]{64}$'
  OR length(btrim(p_prepared->>'qualificationRef')) NOT BETWEEN 1 AND 2048
  OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_prepared) k WHERE k NOT IN('version','sourceHash','releaseId','legalityRevisionId','qualificationRef','selectedRevisionId','countdownQualificationRef'))
  OR(p_kind='countdown' AND (p_prepared ? 'selectedRevisionId' OR p_prepared ? 'countdownQualificationRef')) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted room preparation';END IF;
 FOREACH key IN ARRAY (CASE p_kind WHEN 'ready' THEN ARRAY['releaseId','legalityRevisionId','selectedRevisionId'] ELSE ARRAY['releaseId','legalityRevisionId'] END) LOOP
  IF jsonb_typeof(p_prepared->key) IS DISTINCT FROM 'string' OR p_prepared->>key !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR p_prepared->>key='00000000-0000-0000-0000-000000000000' THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid trusted room source identity';END IF;
 END LOOP;
 release_id:=(p_prepared->>'releaseId')::uuid;legality_id:=(p_prepared->>'legalityRevisionId')::uuid;
 IF p_kind='ready' THEN
  revision_id:=(p_prepared->>'selectedRevisionId')::uuid;
  child:=app.claim_set_room_ready(player.id,p_room,basis,true,revision_id,p_prepared->>'qualificationRef',parent.id);
 ELSE child:=app.claim_start_room_countdown(player.id,p_room,basis,p_prepared->>'qualificationRef',parent.id);END IF;
 -- Both identities precede the competing room lock. Canonical apply owns writes.
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 source:=app.read_native_room_source(p_room,basis,release_id,legality_id);
 IF source->>'status' IS DISTINCT FROM 'success' OR source->'data'->>'sourceHash' IS DISTINCT FROM p_prepared->>'sourceHash' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='trusted room source changed; reprepare original';END IF;
 terms:=source->'data'->'terms';
 IF split_part(p_prepared->>'qualificationRef','/',1) IN ('native-constructed-ready-v1','native-constructed-ffa-candidate-v1') THEN
  observed_policy:=app.native_constructed_room_policy(terms,p_prepared,p_kind);
  policy_rejected:=observed_policy IS NULL;
 END IF;
 IF p_kind='ready' THEN
  SELECT x INTO own_slot FROM jsonb_array_elements(terms->'slots') x WHERE x->>'controllerKind'='human' AND x->>'accountId'=player.id::text;
  IF own_slot IS NULL OR own_slot->>'sourceStatus'<>'available' OR own_slot->>'selectedRevisionId' IS DISTINCT FROM revision_id::text THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='trusted Ready preparation lacks own selected source';END IF;
 ELSE
  IF EXISTS(SELECT 1 FROM jsonb_array_elements(terms->'slots') x WHERE x->>'sourceStatus'<>'available') THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='trusted countdown preparation has incomplete source';END IF;
 END IF;
 -- An automatic or revised countdown must validate every deck. Store its
 -- distinct room-wide proof while the original is still pending and locked.
 IF p_prepared ? 'countdownQualificationRef' THEN
  IF jsonb_typeof(p_prepared->'countdownQualificationRef') IS DISTINCT FROM 'string'
   OR p_prepared->>'countdownQualificationRef' !~ '^native-constructed-ffa-candidate-v1/(standard|pioneer|modern|legacy|vintage)/sha256:[0-9a-f]{64}$'
   OR observed_policy IS NULL OR EXISTS(SELECT 1 FROM jsonb_array_elements(terms->'slots') s
      WHERE s->>'sourceStatus' IS DISTINCT FROM 'available' OR s->>'teamRef' IS NOT NULL)
  THEN policy_rejected:=true;
  ELSE
   full_prepared:=(p_prepared-'countdownQualificationRef'-'selectedRevisionId') || jsonb_build_object(
      'version','native-countdown-prepared-v1','qualificationRef',p_prepared->'countdownQualificationRef');
   full_policy:=app.native_constructed_room_policy(terms,full_prepared,'countdown');
   policy_rejected:=full_policy IS NULL;
  END IF;
 END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','qualified-human-v1','operation',p_kind,
  'original',p_original,'targetId',p_room,'sourceTerms',terms,'sourceHash',source->'data'->>'sourceHash',
  'prepared',p_prepared,'childReceiptId',child.id) || CASE WHEN observed_policy IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('constructedPolicy',observed_policy) END
  || CASE WHEN full_policy IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('roomStartPolicy',full_policy,'roomStartPrepared',full_prepared) END
  WHERE id=parent.id;
 IF child.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new room original has preexisting canonical child';END IF;
 -- A non-host countdown goes through the canonical authority check first.
 IF policy_rejected AND player.state='active' AND (p_kind='ready' OR lobby.host_id=player.id) THEN
  response:=app.finish_account_command(child.id,false,CASE WHEN p_kind='ready' THEN
   jsonb_build_object('deck','qualified selected revision required') ELSE jsonb_build_object('decks','qualification required') END);
 ELSIF p_kind='ready' THEN response:=app.apply_set_room_ready(child.id,player.id,p_room,basis,true,revision_id,p_prepared->>'qualificationRef',parent.id);
 ELSE response:=app.apply_start_room_countdown(child.id,player.id,p_room,basis,p_prepared->>'qualificationRef',parent.id);END IF;
 -- A partial Ready may be accepted in an incomplete lobby. If this specific
 -- Ready actually advanced a countdown it must leave a usable full-roster proof.
 IF p_kind='ready' AND observed_policy IS NOT NULL AND full_policy IS NULL
    AND response->>'status'='success' AND (response->'data'->>'revision')::numeric>basis
    AND EXISTS(SELECT 1 FROM app.room WHERE id=p_room AND state='countdown')
 THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='countdown requires validated roster'; END IF;
 -- A revocation during any canonical room/slot/deck/refresh wait rolls back the
 -- whole function, including parent and child, before returning unauthorized.
 player:=app.lock_session_account(p_subject,p_session);
 RETURN app.finish_account_command(parent.id,response->>'status'='success',response->'data');
EXCEPTION WHEN invalid_authorization_specification THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;RAISE;
END $$;


-- Start the exact countdown accepted by Ready or host force-start. Resolve
-- every seat from its retained source, including on a retry after the room starts.
CREATE FUNCTION app.start_validated_countdown(p_room uuid, p_expected numeric, p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE
  original app.command_receipt;
  prepared jsonb;
  slots jsonb;
  inputs jsonb;
BEGIN
  IF p_room IS NULL OR p_room='00000000-0000-0000-0000-000000000000'::uuid OR
     p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR
     p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
  THEN RAISE EXCEPTION USING ERRCODE='22023', MESSAGE='invalid validated countdown request'; END IF;
  SELECT * INTO original FROM app.command_receipt r
   WHERE ((r.scope='qualified_countdown_intent_v1' AND r.normalized_source->'sourceTerms'->'room'->>'state'='lobby'
           AND r.normalized_source ? 'constructedPolicy') OR
          (r.scope='qualified_ready_intent_v1' AND r.normalized_source ? 'roomStartPolicy'))
     AND r.target_ref=p_room::text AND r.state='accepted'
     AND r.response_payload->'data'->>'revision'=p_expected::text
     AND r.response_payload->'data'->>'resourceId'=p_room::text
     AND r.normalized_source->'sourceTerms'->'room'->>'id'=p_room::text
     AND r.normalized_source->'sourceTerms'->'room'->>'revision'=(p_expected-1)::text;
  IF NOT FOUND THEN
    IF EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.scope='room_start_v1'
       AND r.principal_key='room-start:'||p_room::text AND r.request_ref=p_request::text)
    THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room start request terms conflict'; END IF;
    -- Without a qualified source the worker cannot submit an original to SQL185.
    RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified countdown source unavailable';
  END IF;
  prepared:=coalesce(original.normalized_source->'roomStartPrepared',original.normalized_source->'prepared');
  slots:=original.normalized_source->'sourceTerms'->'slots';
  SELECT jsonb_agg(jsonb_build_object(
    'slotId',s->'slotId','revisionId',s->'selectedRevisionId',
    'snapshotRef','native-constructed-deck-v1/' || original.id::text || '/' ||
      (s->>'slotId') || '/sha256:' || app.auth_json_hash(jsonb_build_object(
        'release',prepared->'releaseId','revision',s->'selectedRevisionId','entries',s->'entries')))
    ORDER BY (s->>'seatIndex')::numeric) INTO inputs FROM jsonb_array_elements(slots) s;
  RETURN app.start_room_match(p_room,p_expected,(prepared->>'releaseId')::uuid,
    (prepared->>'legalityRevisionId')::uuid,prepared->>'qualificationRef',inputs,p_request);
END $$;
-- Run the dictionary check only when SQL185 first accepts the reservation.
-- The unique-key winner owns this transition. Readers of its original never
-- repeat the check against a subsequently changed format or legality revision.
CREATE FUNCTION app.guard_validated_countdown_start() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE
  original app.command_receipt;
  snapshot text;
  result_id uuid := (NEW.response_payload->'data'->>'resourceId')::uuid;
BEGIN
  SELECT s.deck_snapshot_ref INTO snapshot FROM app.match_seat s
    WHERE s.match_id=result_id AND s.deck_snapshot_ref LIKE 'native-constructed-deck-v1/%' LIMIT 1;
  IF NOT FOUND THEN RETURN NEW; END IF;
  SELECT * INTO STRICT original FROM app.command_receipt WHERE id=split_part(snapshot,'/',2)::uuid
    AND target_ref=NEW.target_ref AND scope IN ('qualified_countdown_intent_v1','qualified_ready_intent_v1') AND state='accepted';
  IF coalesce(original.normalized_source->'roomStartPolicy',original.normalized_source->'constructedPolicy') IS NULL OR
    app.native_constructed_room_policy(original.normalized_source->'sourceTerms',
      coalesce(original.normalized_source->'roomStartPrepared',original.normalized_source->'prepared'),'countdown')
    IS DISTINCT FROM coalesce(original.normalized_source->'roomStartPolicy',original.normalized_source->'constructedPolicy')
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='countdown deck policy changed'; END IF;
  RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION app.guard_validated_countdown_start()
 FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
CREATE TRIGGER validated_countdown_start AFTER UPDATE OF state ON app.command_receipt
 FOR EACH ROW WHEN (NEW.scope='room_start_v1' AND OLD.state='pending' AND NEW.state='accepted')
 EXECUTE FUNCTION app.guard_validated_countdown_start();

REVOKE ALL ON FUNCTION app.start_validated_countdown(uuid,numeric,uuid)
 FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.start_validated_countdown(uuid,numeric,uuid) TO service_role;
CREATE OR REPLACE FUNCTION app.guard_constructed_between_admission() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE
  terms jsonb := NEW.normalized_source->'sourceTerms';
  prepared jsonb := NEW.normalized_source->'prepared';
  accepted jsonb;
  roster jsonb;
  m app.match;
  format_key text;
BEGIN
  SELECT * INTO STRICT m FROM app.match WHERE id=(terms->'match'->>'id')::uuid;
  SELECT f.key INTO STRICT format_key FROM app.format f WHERE f.id=m.format_id FOR SHARE;
  IF prepared->>'qualificationRef' IS DISTINCT FROM
       'native-constructed-between-v1/' || format_key || '/' || (NEW.normalized_source->>'sourceHash')
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game selection lacks bound format'; END IF;
  SELECT r.normalized_source INTO accepted
    FROM app.command_receipt r
   WHERE (r.scope='qualified_countdown_intent_v1' OR
          (r.scope='qualified_ready_intent_v1' AND r.normalized_source ? 'roomStartPolicy')) AND r.state='accepted'
     AND r.target_ref=m.room_id::text
     AND coalesce(r.normalized_source->'roomStartPrepared',r.normalized_source->'prepared')->>'qualificationRef'=m.admission_qualification_ref
     AND coalesce(r.normalized_source->'roomStartPrepared',r.normalized_source->'prepared')->>'releaseId'=m.accepted_release_id::text
     AND coalesce(r.normalized_source->'roomStartPolicy',r.normalized_source->'constructedPolicy')->>'legalityRevisionId'=m.legality_revision_id::text
     AND coalesce(r.normalized_source->'roomStartPolicy',r.normalized_source->'constructedPolicy')->>'formatId'=m.format_id::text
     AND coalesce(r.normalized_source->'roomStartPolicy',r.normalized_source->'constructedPolicy')->>'formatKey'=format_key
     AND r.normalized_source->'sourceTerms'->'room' @> jsonb_build_object(
       'profileId',m.profile_id,'mode',m.mode,'competitive',m.competitive,'structure',m.structure)
   ORDER BY r.id LIMIT 1;
  roster:=accepted->'sourceTerms'->'slots';
  IF roster IS NULL OR jsonb_array_length(roster)<>(SELECT count(*) FROM app.match_seat WHERE match_id=m.id)
     OR EXISTS (
       SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND
         (s.team_ref IS NOT NULL OR NOT EXISTS (
           SELECT 1 FROM jsonb_array_elements(roster) slot
            WHERE slot @> jsonb_build_object(
              'seatIndex',s.seat_index::text,'controllerKind',s.controller_kind,
              'accountId',s.account_id,'sponsorId',s.sponsor_id,
              'teamRef',NULL,'selectedRevisionId',s.source_revision_id))))
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game selection lacks accepted room policy'; END IF;
  RETURN NEW;
END $$;

COMMIT;
