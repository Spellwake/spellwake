BEGIN;

-- Each pilot account is explicitly granted access to one immutable release.
CREATE TABLE app.commander_pilot_member (
 release_id uuid NOT NULL REFERENCES app.qualified_release(id),
 account_id uuid NOT NULL REFERENCES app.account(id),
 PRIMARY KEY(release_id,account_id)
);
ALTER TABLE app.commander_pilot_member ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.commander_pilot_member FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

ALTER FUNCTION app.native_constructed_room_policy(jsonb,jsonb,text)
 RENAME TO native_room_policy_before_commander;
ALTER FUNCTION app.qualify_native_room_command(uuid,uuid,uuid,jsonb,jsonb,text)
 RENAME TO qualify_native_room_before_commander;

CREATE FUNCTION app.native_constructed_room_policy(p_terms jsonb,p_prepared jsonb,p_kind text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE format_id_ uuid:=(p_terms->'room'->>'formatId')::uuid;
 legality_id_ uuid:=(p_prepared->>'legalityRevisionId')::uuid; selected uuid[];
 current_key text; proof text:=p_prepared->>'qualificationRef'; statuses jsonb;
BEGIN
 SELECT f.key INTO current_key FROM app.format f WHERE f.id=format_id_ FOR SHARE;
 IF current_key IS DISTINCT FROM 'commander' THEN
   RETURN app.native_room_policy_before_commander(p_terms,p_prepared,p_kind);
 END IF;
 IF split_part(proof,'/',2) IS DISTINCT FROM current_key
 OR split_part(proof,'/',1) IS DISTINCT FROM (CASE WHEN p_kind='ready' THEN 'native-constructed-ready-v1' ELSE 'native-constructed-ffa-candidate-v1' END)
 THEN RETURN NULL; END IF;

 IF p_kind='ready' THEN selected:=ARRAY[(p_prepared->>'selectedRevisionId')::uuid];
 ELSE SELECT array_agg(DISTINCT (s->>'selectedRevisionId')::uuid) INTO selected FROM jsonb_array_elements(p_terms->'slots') s;
 END IF;
 IF selected IS NULL OR array_position(selected,NULL) IS NOT NULL THEN RETURN NULL; END IF;
 IF current_key='commander' THEN
   IF split_part(proof,'/',3) IS DISTINCT FROM
        ('sha256:'||encode(sha256(convert_to(p_prepared->>'sourceHash','UTF8')),'hex'))
   THEN RETURN NULL; END IF;
   IF NOT EXISTS(SELECT 1 FROM app.room room WHERE room.id=(p_terms->'room'->>'id')::uuid
       AND room.visibility='private' AND room.mode='open' AND room.structure='bo1' AND NOT room.competitive)
     OR jsonb_array_length(p_terms->'slots')>3
     OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_terms->'slots') s
       WHERE NOT EXISTS(SELECT 1 FROM app.commander_pilot_member member
         WHERE member.release_id=(p_prepared->>'releaseId')::uuid
           AND member.account_id=(s->>'accountId')::uuid))
     OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_terms->'slots') s
       WHERE s->>'controllerKind' IS DISTINCT FROM 'human' OR s->>'teamRef' IS NOT NULL)
     OR EXISTS(
       SELECT 1 FROM unnest(selected) r(revision)
       CROSS JOIN LATERAL (
         SELECT coalesce(sum(e.quantity),0) AS cards,
                count(*) FILTER(WHERE e.section='commander' AND e.quantity=1) AS commanders,
                count(*) FILTER(WHERE e.section<>'main' AND (e.section<>'commander' OR e.quantity<>1)) AS invalid
         FROM app.deck_entry e WHERE e.revision_id=r.revision
       ) sizes
       WHERE sizes.cards<>100 OR sizes.commanders NOT BETWEEN 1 AND 2 OR sizes.invalid<>0
     ) THEN RETURN NULL; END IF;
 END IF;
 -- The reference dictionary writer locks this revision before inserting legality.
 PERFORM id FROM app.legality_revision WHERE id=legality_id_
 AND (current_key<>'commander' OR effective_at<=statement_timestamp()) FOR SHARE;
 IF NOT FOUND THEN RETURN NULL; END IF;
 PERFORM l.id FROM app.card_legality l WHERE l.format_id=format_id_ AND l.legality_revision_id=legality_id_
 AND l.definition_id IN (SELECT e.definition_id FROM app.deck_entry e WHERE e.revision_id=ANY(selected))
 ORDER BY l.id FOR SHARE;
 SELECT jsonb_agg(jsonb_build_object('revisionId',q.revision_id,'definitionId',q.definition_id,
  'quantity',q.copies::text,'status',l.status) ORDER BY q.revision_id,q.definition_id) INTO statuses
 FROM (SELECT e.revision_id,e.definition_id,sum(e.quantity) copies FROM app.deck_entry e
       WHERE e.revision_id=ANY(selected) GROUP BY e.revision_id,e.definition_id) q
 LEFT JOIN app.card_legality l ON l.definition_id=q.definition_id AND l.format_id=format_id_ AND l.legality_revision_id=legality_id_;
 IF statuses IS NULL OR EXISTS(SELECT 1 FROM jsonb_array_elements(statuses) s WHERE coalesce(s->>'status','') NOT IN ('legal','restricted')
 OR (s->>'status'='restricted' AND (current_key='commander' OR (s->>'quantity')::numeric>1))) THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('formatId',format_id_,'formatKey',current_key,'legalityRevisionId',legality_id_,'cards',statuses);
END $$;

CREATE FUNCTION app.qualify_native_room_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb,p_kind text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;parent app.command_receipt;child app.command_receipt;lobby app.room;
 request_id uuid;basis numeric;release_id uuid;legality_id uuid;revision_id uuid;scope_name text;
 rejected_ready boolean; full_prepared jsonb; full_policy jsonb; policy_rejected boolean:=false;observed_policy jsonb;source jsonb;terms jsonb;response jsonb;own_slot jsonb;key text;
BEGIN
 IF NOT EXISTS(
   SELECT 1 FROM app.room room JOIN app.format f ON f.id=room.format_id
   WHERE room.id=p_room AND f.key='commander'
 ) THEN
   RETURN app.qualify_native_room_before_commander(p_subject,p_session,p_room,p_original,p_prepared,p_kind);
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified room command requires read committed retry';END IF;
 player:=app.lock_session_account(p_subject,p_session);
 IF p_kind IS NULL OR p_kind NOT IN('ready','countdown') OR NOT app.native_room_intent_is_valid(p_room,p_original) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 request_id:=(p_original->>'requestId')::uuid;basis:=(p_original->>'expectedRevision')::numeric;
 scope_name:=CASE p_kind WHEN 'ready' THEN 'qualified_ready_intent_v1' ELSE 'qualified_countdown_intent_v1' END;
 parent:=app.begin_account_command(player.id,request_id,scope_name,p_room::text,jsonb_build_object('room',p_room,'original',p_original));
 -- Current caller authority, then exact original before new preparation/source.
 IF parent.state<>'pending' THEN RETURN parent.response_payload;END IF;
 rejected_ready:=p_kind='ready' AND coalesce(p_prepared->>'version'='native-ready-rejected-v1',false);
 IF jsonb_typeof(p_prepared) IS DISTINCT FROM 'object' OR octet_length(p_prepared::text)>2097152
  OR (NOT rejected_ready AND p_prepared->>'version' IS DISTINCT FROM
     (CASE p_kind WHEN 'ready' THEN 'native-ready-prepared-v1' ELSE 'native-countdown-prepared-v1' END))
  OR (rejected_ready AND (p_prepared->>'qualificationRef' IS DISTINCT FROM 'native-ready-deck-rejected-v1'
       OR p_prepared ? 'countdownQualificationRef'))
  OR (NOT rejected_ready AND p_prepared->>'qualificationRef'='native-ready-deck-rejected-v1')
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
   OR p_prepared->>'countdownQualificationRef' !~ '^native-constructed-ffa-candidate-v1/(standard|pioneer|modern|legacy|vintage|draft|cube|commander)/sha256:[0-9a-f]{64}$'
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
 IF rejected_ready THEN
  response:=app.finish_account_command(child.id,false,CASE WHEN player.state='active'
    THEN jsonb_build_object('deck','qualified selected revision required')
    ELSE jsonb_build_object('account','unavailable') END);
 ELSIF policy_rejected AND player.state='active' AND (p_kind='ready' OR lobby.host_id=player.id) THEN
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


REVOKE ALL ON FUNCTION app.native_constructed_room_policy(jsonb,jsonb,text),
 app.qualify_native_room_command(uuid,uuid,uuid,jsonb,jsonb,text)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Limit this worker lane to its configured Commander release. Other releases
-- retain their existing scheduling and recovery owners.
CREATE FUNCTION app.read_commander_opening_page(p_release uuid,p_after uuid,p_limit integer)
RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_release IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 32 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid Commander opening page';
 END IF;
 RETURN jsonb_build_object('items',coalesce((
   SELECT jsonb_agg(job.id ORDER BY job.id)
   FROM (
     SELECT g.id FROM app.game g
     JOIN app.match m ON m.reserved_game_id=g.id AND m.id=g.match_id
     JOIN app.format f ON f.id=m.format_id AND f.key='commander'
     WHERE m.accepted_release_id=p_release AND g.state='reserved'
       AND (p_after IS NULL OR g.id>p_after)
     ORDER BY g.id LIMIT p_limit
   ) job
 ),'[]'::jsonb));
END $$;
REVOKE ALL ON FUNCTION app.read_commander_opening_page(uuid,uuid,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_commander_opening_page(uuid,uuid,integer) TO service_role;

COMMIT;
