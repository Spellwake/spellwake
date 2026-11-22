BEGIN;

-- The trusted draft service seals the printing multiset assigned to one
-- occupied seat. Room commands use this authority independently of collection ownership.
CREATE TABLE app.limited_match_seat_pool (
  slot_id uuid NOT NULL REFERENCES app.room_slot(id) ON DELETE CASCADE,
  room_id uuid NOT NULL REFERENCES app.room(id) ON DELETE CASCADE,
  account_id uuid NOT NULL REFERENCES app.account(id),
  format_id uuid NOT NULL REFERENCES app.format(id),
  source_revision_id uuid NOT NULL,
  release_id uuid NOT NULL REFERENCES app.qualified_release(id),
  printing_counts jsonb NOT NULL CHECK (jsonb_typeof(printing_counts)='object'),
  PRIMARY KEY(room_id,account_id)
);
ALTER TABLE app.limited_match_seat_pool ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON TABLE app.limited_match_seat_pool FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.seal_limited_match_seat_pool(
 p_room uuid,p_slot uuid,p_source_revision uuid,p_release uuid,p_printing_counts jsonb
) RETURNS uuid LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; slot app.room_slot; saved app.limited_match_seat_pool; owner uuid;
BEGIN
 IF p_source_revision IS NULL OR p_source_revision='00000000-0000-0000-0000-000000000000'::uuid
    OR p_release IS NULL OR jsonb_typeof(p_printing_counts) IS DISTINCT FROM 'object'
    OR p_printing_counts='{}'::jsonb OR octet_length(p_printing_counts::text)>65536
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid limited pool envelope';END IF;
 SELECT s.account_id INTO owner FROM app.room_slot s
  WHERE s.id=p_slot AND s.room_id=p_room AND s.retired_at IS NULL;
 PERFORM id FROM app.account WHERE id=owner AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='limited pool account unavailable';END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 SELECT * INTO slot FROM app.room_slot WHERE id=p_slot AND room_id=p_room AND retired_at IS NULL;
 IF lobby.id IS NULL OR slot.account_id IS DISTINCT FROM owner OR slot.controller_kind<>'human' OR slot.team_ref IS NOT NULL
    OR lobby.mode<>'open' OR lobby.structure<>'bo1' OR lobby.competitive OR lobby.visibility<>'private'
    OR NOT EXISTS(SELECT 1 FROM app.format WHERE id=lobby.format_id AND key IN ('draft','cube'))
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='limited pool seat unavailable';END IF;

 SELECT * INTO saved FROM app.limited_match_seat_pool WHERE room_id=p_room AND account_id=slot.account_id;
 IF FOUND THEN
   IF (saved.room_id,saved.account_id,saved.format_id,saved.source_revision_id,saved.release_id,saved.printing_counts)
      IS DISTINCT FROM (p_room,slot.account_id,lobby.format_id,p_source_revision,p_release,p_printing_counts)
   THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='limited seat pool terms conflict';END IF;
   RETURN p_slot;
 END IF;
 IF EXISTS(SELECT 1 FROM app.limited_match_seat_pool pool WHERE pool.room_id=p_room
      AND (pool.format_id,pool.source_revision_id,pool.release_id)
          IS DISTINCT FROM (lobby.format_id,p_source_revision,p_release))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='limited room pool source conflict';END IF;
 IF lobby.state<>'lobby' OR NOT EXISTS(SELECT 1 FROM app.qualified_release WHERE id=p_release)
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='limited pool release or lobby unavailable';END IF;
 IF EXISTS(SELECT 1 FROM jsonb_each(p_printing_counts) item
    WHERE item.key !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
       OR jsonb_typeof(item.value) IS DISTINCT FROM 'string'
       OR item.value#>>'{}' !~ '^[1-9][0-9]{0,2}$')
 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid limited printing count';END IF;
 IF EXISTS(SELECT 1 FROM jsonb_each(p_printing_counts) item WHERE NOT EXISTS(
   SELECT 1 FROM app.printing p JOIN app.card_definition_revision body ON body.definition_id=p.definition_id
   JOIN app.release_card_program program ON program.definition_revision_id=body.id
   WHERE p.id=item.key::uuid AND program.release_id=p_release))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='limited pool printing lacks a release program';END IF;
 INSERT INTO app.limited_match_seat_pool
 VALUES(p_slot,p_room,slot.account_id,lobby.format_id,p_source_revision,p_release,p_printing_counts);
 RETURN p_slot;
END $$;
REVOKE ALL ON FUNCTION app.seal_limited_match_seat_pool(uuid,uuid,uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.seal_limited_match_seat_pool(uuid,uuid,uuid,uuid,jsonb) TO service_role;

-- Prove exact printing counts across all sections of a selected list.
-- A release-designated free basic land may be added without a drafted copy.
CREATE FUNCTION app.limited_match_seat_pool_proof(
 p_room uuid,p_slot uuid,p_account uuid,p_format uuid,p_release uuid,p_revision uuid
) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('slotId',s.id,'sourceRevisionId',pool.source_revision_id,
    'poolHash',app.auth_json_hash(pool.printing_counts))
 FROM app.limited_match_seat_pool pool
 JOIN app.room r ON r.id=pool.room_id
 JOIN app.room_slot s ON s.id=p_slot AND s.room_id=r.id
 WHERE pool.room_id=p_room AND pool.account_id=p_account
   AND pool.format_id=p_format AND pool.release_id=p_release AND r.format_id=p_format
   AND r.mode='open' AND r.structure='bo1' AND NOT r.competitive AND r.visibility='private'
   AND s.account_id=p_account AND s.retired_at IS NULL AND s.controller_kind='human' AND s.team_ref IS NULL
   AND s.deck_revision_id=p_revision
   AND NOT EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=p_revision AND e.section NOT IN ('main','sideboard'))
   AND coalesce((SELECT sum(e.quantity) FROM app.deck_entry e WHERE e.revision_id=p_revision AND e.section='main'),0)>=40
   AND NOT EXISTS(
     SELECT 1 FROM app.deck_entry e WHERE e.revision_id=p_revision AND NOT EXISTS(
       SELECT 1 FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id=program.definition_revision_id
       WHERE program.release_id=p_release AND body.definition_id=e.definition_id AND program.unlimited_basic_land)
     GROUP BY e.printing_id HAVING e.printing_id IS NULL
       OR sum(e.quantity)>coalesce((pool.printing_counts->>e.printing_id::text)::numeric,0))
$$;
REVOKE ALL ON FUNCTION app.limited_match_seat_pool_proof(uuid,uuid,uuid,uuid,uuid,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Alternate private admission entry points must also enforce pool ownership.
CREATE FUNCTION app.guard_limited_accepted_match() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE matched app.match;
BEGIN
 SELECT m.* INTO STRICT matched FROM app.match m WHERE m.id=NEW.target_ref::uuid;
 IF EXISTS(SELECT 1 FROM app.format f WHERE f.id=matched.format_id AND f.key IN ('draft','cube'))
 AND (matched.mode<>'open' OR matched.structure<>'bo1' OR matched.competitive OR EXISTS(
   SELECT 1 FROM app.match_seat seat
   WHERE seat.match_id=matched.id AND (seat.controller_kind<>'human' OR seat.team_ref IS NOT NULL OR NOT EXISTS(
     SELECT 1 FROM app.room_slot s WHERE s.room_id=matched.room_id AND s.account_id=seat.account_id
       AND app.limited_match_seat_pool_proof(matched.room_id,s.id,seat.account_id,matched.format_id,
         matched.accepted_release_id,seat.source_revision_id) IS NOT NULL))))
 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match deck lacks a sealed limited seat pool';END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION app.guard_limited_accepted_match()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE TRIGGER limited_accepted_match AFTER UPDATE OF state ON app.command_receipt
 FOR EACH ROW WHEN (NEW.scope='match_decks_v1' AND OLD.state='pending' AND NEW.state='accepted')
 EXECUTE FUNCTION app.guard_limited_accepted_match();

CREATE OR REPLACE FUNCTION app.native_constructed_room_policy(p_terms jsonb,p_prepared jsonb,p_kind text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE format_id_ uuid:=(p_terms->'room'->>'formatId')::uuid;
 legality_id_ uuid:=(p_prepared->>'legalityRevisionId')::uuid; selected uuid[];
 current_key text; proof text:=p_prepared->>'qualificationRef'; statuses jsonb; pool_rows jsonb; item jsonb; proof_row jsonb;
BEGIN
 SELECT f.key INTO current_key FROM app.format f WHERE f.id=format_id_ FOR SHARE;
 IF current_key IS NULL OR current_key NOT IN ('standard','pioneer','modern','legacy','vintage','draft','cube')
 OR split_part(proof,'/',2) IS DISTINCT FROM current_key
 OR split_part(proof,'/',1) IS DISTINCT FROM (CASE WHEN p_kind='ready' THEN 'native-constructed-ready-v1' ELSE 'native-constructed-ffa-candidate-v1' END)
 THEN RETURN NULL; END IF;

 IF p_kind='ready' THEN selected:=ARRAY[(p_prepared->>'selectedRevisionId')::uuid];
 ELSE SELECT array_agg(DISTINCT (s->>'selectedRevisionId')::uuid) INTO selected FROM jsonb_array_elements(p_terms->'slots') s;
 END IF;
 IF current_key IN ('draft','cube') THEN
  pool_rows:='[]'::jsonb;
  FOR item IN SELECT s FROM jsonb_array_elements(p_terms->'slots') s
    WHERE p_kind='countdown' OR (s->>'selectedRevisionId')::uuid=ANY(selected)
  LOOP
   IF item->>'controllerKind' IS DISTINCT FROM 'human' THEN RETURN NULL;END IF;
   proof_row:=app.limited_match_seat_pool_proof((p_terms->'room'->>'id')::uuid,
     (item->>'slotId')::uuid,(item->>'accountId')::uuid,format_id_,
     (p_prepared->>'releaseId')::uuid,(item->>'selectedRevisionId')::uuid);
   IF proof_row IS NULL THEN RETURN NULL;END IF;
   pool_rows:=pool_rows||jsonb_build_array(proof_row);
  END LOOP;
  IF jsonb_array_length(pool_rows)=0 THEN RETURN NULL;END IF;
 END IF;

 IF selected IS NULL OR array_position(selected,NULL) IS NOT NULL THEN RETURN NULL; END IF;
 -- The reference dictionary writer locks this revision before inserting legality.
 PERFORM id FROM app.legality_revision WHERE id=legality_id_ FOR SHARE;
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
 OR (s->>'status'='restricted' AND (s->>'quantity')::numeric>1)) THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('formatId',format_id_,'formatKey',current_key,'legalityRevisionId',legality_id_,'cards',statuses) ||
   CASE WHEN pool_rows IS NULL THEN '{}'::jsonb ELSE jsonb_build_object('seatPools',pool_rows) END;
END $$;

CREATE OR REPLACE FUNCTION app.qualify_native_room_command(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb,p_kind text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;parent app.command_receipt;child app.command_receipt;lobby app.room;
 request_id uuid;basis numeric;release_id uuid;legality_id uuid;revision_id uuid;scope_name text;
 rejected_ready boolean; full_prepared jsonb; full_policy jsonb; policy_rejected boolean:=false;observed_policy jsonb;source jsonb;terms jsonb;response jsonb;own_slot jsonb;key text;
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
   OR p_prepared->>'countdownQualificationRef' !~ '^native-constructed-ffa-candidate-v1/(standard|pioneer|modern|legacy|vintage|draft|cube)/sha256:[0-9a-f]{64}$'
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

COMMIT;
