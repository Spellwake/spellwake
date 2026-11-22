BEGIN;

-- Apply only over the reviewed hosted Commander admission wrappers and delegates.
DO $limited_predecessor$
BEGIN
 IF EXISTS (
   SELECT 1
   FROM (VALUES
    ('app.native_constructed_room_policy(jsonb,jsonb,text)', '111aa296c278409b6333029008de922f89685c18eae8466a6aac1e5a62cd7e13'),
    ('app.qualify_native_room_command(uuid,uuid,uuid,jsonb,jsonb,text)', '840c50146ffd191e195a2b227a7a9d5d26db0a68174dacca3e3b7a97bcc2b583'),
    ('app.native_room_policy_before_commander(jsonb,jsonb,text)', 'fcddd6e0c4cfcf7c526b54865c66ed9b76cb85c22b46dda85ddb74d4d8a651fa'),
    ('app.qualify_native_room_before_commander(uuid,uuid,uuid,jsonb,jsonb,text)', 'b8d68b2cd6a8042f6bcb00ff93524a50220d9dd1f012f3a54699d68167d1bff3')
   ) AS expected(signature,body_hash)
   LEFT JOIN pg_catalog.pg_proc installed
     ON installed.oid=pg_catalog.to_regprocedure(expected.signature)
   WHERE pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(installed.prosrc,'UTF8')),'hex')
         IS DISTINCT FROM expected.body_hash
 ) THEN
   RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='private Limited predecessor differs from reviewed hosted admission';
 END IF;
END
$limited_predecessor$;

-- An operator admits fixture accounts to one immutable Limited release.
CREATE TABLE app.limited_pilot_member (
 release_id uuid NOT NULL REFERENCES app.qualified_release(id),
 account_id uuid NOT NULL REFERENCES app.account(id),
 PRIMARY KEY(release_id,account_id)
);
ALTER TABLE app.limited_pilot_member ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.limited_pilot_member FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

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
    OR NOT EXISTS(SELECT 1 FROM app.limited_pilot_member member
      WHERE member.release_id=p_release AND member.account_id=owner)
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
   AND EXISTS(SELECT 1 FROM app.limited_pilot_member member
     WHERE member.release_id=p_release AND member.account_id=p_account)
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

CREATE OR REPLACE FUNCTION app.native_room_policy_before_commander(p_terms jsonb,p_prepared jsonb,p_kind text)
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

CREATE OR REPLACE FUNCTION app.qualify_native_room_before_commander(p_subject uuid,p_session uuid,p_room uuid,p_original jsonb,p_prepared jsonb,p_kind text) RETURNS jsonb
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

CREATE TABLE app.limited_draft_source (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 format_id uuid NOT NULL REFERENCES app.format(id),
 release_id uuid NOT NULL REFERENCES app.qualified_release(id),
 legality_revision_id uuid NOT NULL REFERENCES app.legality_revision(id),
 pack_size integer NOT NULL CHECK (pack_size BETWEEN 1 AND 20),
 booster_recipe_id uuid REFERENCES app.booster_recipe(id),
 cube_printings uuid[],
 CHECK ((booster_recipe_id IS NULL) <> (cube_printings IS NULL)),
 CHECK (cube_printings IS NULL OR
   (cardinality(cube_printings) BETWEEN 1 AND 1000 AND array_position(cube_printings,NULL) IS NULL))
);

CREATE TABLE app.limited_draft (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 room_id uuid NOT NULL UNIQUE REFERENCES app.room(id) ON DELETE CASCADE,
 source_id uuid NOT NULL REFERENCES app.limited_draft_source(id),
 seat_count integer NOT NULL CHECK (seat_count BETWEEN 2 AND 8),
 step integer NOT NULL DEFAULT 0 CHECK (step BETWEEN 0 AND 60)
);

CREATE TABLE app.limited_draft_seat (
 draft_id uuid NOT NULL REFERENCES app.limited_draft(id) ON DELETE CASCADE,
 seat integer NOT NULL,
 slot_id uuid NOT NULL,
 account_id uuid NOT NULL REFERENCES app.account(id),
 PRIMARY KEY(draft_id,seat),
 UNIQUE(draft_id,account_id)
);

CREATE TABLE app.limited_draft_pack (
 draft_id uuid NOT NULL REFERENCES app.limited_draft(id) ON DELETE CASCADE,
 round_index integer NOT NULL CHECK (round_index BETWEEN 0 AND 2),
 origin integer NOT NULL,
 seed bytea CHECK (seed IS NULL OR octet_length(seed)=32),
 PRIMARY KEY(draft_id,round_index,origin),
 FOREIGN KEY(draft_id,origin) REFERENCES app.limited_draft_seat(draft_id,seat)
);

CREATE TABLE app.limited_draft_item (
 id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
 draft_id uuid NOT NULL,
 round_index integer NOT NULL,
 origin integer NOT NULL,
 printing_id uuid NOT NULL REFERENCES app.printing(id),
 FOREIGN KEY(draft_id,round_index,origin) REFERENCES app.limited_draft_pack(draft_id,round_index,origin) ON DELETE CASCADE,
 UNIQUE(draft_id,id)
);
CREATE INDEX limited_draft_item_pack ON app.limited_draft_item(draft_id,round_index,origin);

CREATE TABLE app.limited_draft_pick (
 draft_id uuid NOT NULL,
 step integer NOT NULL CHECK (step BETWEEN 0 AND 59),
 seat integer NOT NULL,
 item_id uuid NOT NULL UNIQUE,
 PRIMARY KEY(draft_id,step,seat),
 FOREIGN KEY(draft_id,seat) REFERENCES app.limited_draft_seat(draft_id,seat) ON DELETE CASCADE,
 FOREIGN KEY(draft_id,item_id) REFERENCES app.limited_draft_item(draft_id,id) ON DELETE CASCADE
);

ALTER TABLE app.limited_draft_source ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_seat ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_pack ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_item ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.limited_draft_pick ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.limited_draft_source,app.limited_draft,app.limited_draft_seat,
 app.limited_draft_pack,app.limited_draft_item,app.limited_draft_pick
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.reject_limited_draft_source_change() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='limited draft source revision is immutable';
END $$;
REVOKE ALL ON FUNCTION app.reject_limited_draft_source_change()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Published source revisions are append-only. A correction receives a new ID.
CREATE TRIGGER limited_draft_source_immutable BEFORE UPDATE OR DELETE ON app.limited_draft_source
 FOR EACH ROW EXECUTE FUNCTION app.reject_limited_draft_source_change();

CREATE FUNCTION app.limited_draft_actor() RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE claims jsonb:=app.read_current_auth_context(); player app.account;
BEGIN
 IF claims IS NULL THEN RETURN NULL; END IF;
 player:=app.lock_session_account((claims->>'subjectId')::uuid,(claims->>'sessionId')::uuid);
 IF player.state='active' THEN RETURN player.id; END IF;
 RETURN NULL;
END $$;

-- The caller holds the room lock for changes. The sealed roster uses exact
-- room-slot identities, including when an account leaves and joins again.
CREATE FUNCTION app.limited_draft_roster_valid(p_draft uuid,p_room uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS (
   SELECT 1 FROM app.limited_draft d
   JOIN app.limited_draft_source source ON source.id=d.source_id
   JOIN app.room lobby ON lobby.id=d.room_id AND lobby.format_id=source.format_id
   WHERE d.id=p_draft AND lobby.id=p_room AND lobby.mode='open' AND lobby.structure='bo1'
     AND NOT lobby.competitive AND lobby.visibility='private'
 ) AND NOT EXISTS (
   SELECT 1 FROM app.limited_draft_seat member
   WHERE member.draft_id=p_draft AND NOT EXISTS(
     SELECT 1 FROM app.room_slot slot
     JOIN app.limited_draft draft ON draft.id=member.draft_id
     JOIN app.limited_draft_source source ON source.id=draft.source_id
     JOIN app.limited_pilot_member grant_row
       ON grant_row.release_id=source.release_id AND grant_row.account_id=member.account_id
     WHERE slot.id=member.slot_id AND slot.room_id=p_room AND slot.account_id=member.account_id
       AND slot.retired_at IS NULL AND slot.controller_kind='human' AND slot.team_ref IS NULL
   )
 ) AND NOT EXISTS (
   SELECT 1 FROM app.room_slot slot WHERE slot.room_id=p_room AND slot.retired_at IS NULL
   AND NOT EXISTS(SELECT 1 FROM app.limited_draft_seat member WHERE member.draft_id=p_draft AND member.slot_id=slot.id)
 )
$$;

CREATE FUNCTION app.limited_draft_origin(p_seat integer,p_count integer,p_step integer,p_pack_size integer)
RETURNS integer LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT mod(mod(p_seat + CASE WHEN mod(p_step/p_pack_size,2)=0 THEN -1 ELSE 1 END
   * mod(p_step,p_pack_size),p_count)+p_count,p_count)
$$;

CREATE FUNCTION app.start_limited_draft(p_room uuid,p_source uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.limited_draft_actor(); command app.command_receipt; lobby app.room;
 source app.limited_draft_source; kind text; draft_id_ uuid; seat_count_ integer; pack record; cards jsonb;
 failure jsonb:='{"draft":"unavailable"}';
BEGIN
 IF actor IS NULL OR p_room IS NULL OR p_source IS NULL OR p_request IS NULL
    OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',failure);
 END IF;
 command:=app.begin_account_command(actor,p_request,'limited_draft_start_v1',p_room::text,jsonb_build_object('source',p_source));
 IF command.state<>'pending' THEN RETURN command.response_payload;END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 SELECT * INTO source FROM app.limited_draft_source WHERE id=p_source;
 SELECT key INTO kind FROM app.format WHERE id=lobby.format_id;
 IF lobby.host_id IS DISTINCT FROM actor OR lobby.state<>'lobby' OR lobby.mode<>'open' OR lobby.competitive
    OR lobby.structure<>'bo1' OR lobby.visibility<>'private' OR source.format_id IS DISTINCT FROM lobby.format_id
    OR kind NOT IN ('draft','cube') OR (kind='draft') IS DISTINCT FROM (source.booster_recipe_id IS NOT NULL)
    OR EXISTS(SELECT 1 FROM app.limited_draft d WHERE d.room_id=p_room)
    OR EXISTS(SELECT 1 FROM app.limited_match_seat_pool p WHERE p.room_id=p_room)
 THEN RETURN app.finish_account_command(command.id,false,failure); END IF;
 SELECT count(*) INTO seat_count_ FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL;
 IF seat_count_ NOT BETWEEN 2 AND 8 OR EXISTS (
   SELECT 1 FROM app.room_slot slot LEFT JOIN app.account a ON a.id=slot.account_id
   WHERE slot.room_id=p_room AND slot.retired_at IS NULL
    AND (slot.controller_kind<>'human' OR slot.team_ref IS NOT NULL OR slot.ready OR a.state IS DISTINCT FROM 'active'
      OR NOT EXISTS(SELECT 1 FROM app.limited_pilot_member member
        WHERE member.release_id=source.release_id AND member.account_id=slot.account_id))
 ) THEN RETURN app.finish_account_command(command.id,false,failure); END IF;

 -- Validate every possible outcome, including correlated collation groups.
 -- Qualification bounds each plan and makes its slots immutable.
 IF kind='draft' AND NOT EXISTS(SELECT 1 FROM app.booster_recipe recipe WHERE recipe.id=source.booster_recipe_id
     AND recipe.qualified_at IS NOT NULL AND recipe.opening_plan IS NOT NULL)
 THEN RETURN app.finish_account_command(command.id,false,failure); END IF;
 IF kind='cube' AND cardinality(source.cube_printings)<3*seat_count_*source.pack_size
 THEN RETURN app.finish_account_command(command.id,false,failure); END IF;
 PERFORM id FROM app.legality_revision WHERE id=source.legality_revision_id FOR SHARE;
 IF EXISTS (
   SELECT 1 FROM (
     SELECT unnest(source.cube_printings) AS printing_id
     UNION
     SELECT card.printing_id FROM app.booster_slot_card card
       JOIN app.booster_slot slot ON slot.id=card.slot_id WHERE slot.recipe_id=source.booster_recipe_id
     UNION
     SELECT (card.value->>'printingId')::uuid FROM app.booster_recipe recipe
       CROSS JOIN LATERAL jsonb_array_elements(recipe.opening_plan->'groups') g
       CROSS JOIN LATERAL jsonb_array_elements(g.value->'outcomes') o
       CROSS JOIN LATERAL jsonb_array_elements(o.value->'cards') card
     WHERE recipe.id=source.booster_recipe_id
   ) possible WHERE NOT EXISTS (
     SELECT 1 FROM app.printing p JOIN app.card_definition_revision body ON body.definition_id=p.definition_id
     JOIN app.release_card_program program ON program.definition_revision_id=body.id AND program.release_id=source.release_id
     JOIN app.card_legality legality ON legality.definition_id=p.definition_id AND legality.format_id=source.format_id
       AND legality.legality_revision_id=source.legality_revision_id AND legality.status='legal'
     WHERE p.id=possible.printing_id
   )
 ) THEN RETURN app.finish_account_command(command.id,false,failure); END IF;

 INSERT INTO app.limited_draft(room_id,source_id,seat_count) VALUES(p_room,p_source,seat_count_) RETURNING id INTO draft_id_;
 INSERT INTO app.limited_draft_seat(draft_id,seat,slot_id,account_id)
 SELECT draft_id_,(row_number() OVER(ORDER BY seat_index)-1)::integer,id,account_id
 FROM app.room_slot WHERE room_id=p_room AND retired_at IS NULL;
 INSERT INTO app.limited_draft_pack(draft_id,round_index,origin,seed)
 SELECT draft_id_,r,s,CASE WHEN kind='draft' THEN app.new_pack_seed_v1() END
 FROM generate_series(0,2) r CROSS JOIN generate_series(0,seat_count_-1) s;
 IF kind='draft' THEN
   FOR pack IN SELECT * FROM app.limited_draft_pack WHERE draft_id=draft_id_ ORDER BY round_index,origin LOOP
     cards:=app.draw_booster_recipe_v1(source.booster_recipe_id,pack.seed);
     IF (SELECT sum((card->>'quantity')::integer) FROM jsonb_array_elements(cards) card) IS DISTINCT FROM source.pack_size::bigint
     THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='limited draft pack size differs from source';END IF;
     INSERT INTO app.limited_draft_item(draft_id,round_index,origin,printing_id)
     SELECT draft_id_,pack.round_index,pack.origin,(card->>'printingId')::uuid
     FROM jsonb_array_elements(cards) card CROSS JOIN LATERAL generate_series(1,(card->>'quantity')::integer);
   END LOOP;
 ELSE
   -- Each element represents one physical cube copy. Store the allocation once.
   INSERT INTO app.limited_draft_item(draft_id,round_index,origin,printing_id)
   SELECT draft_id_,(n/(seat_count_*source.pack_size))::integer,
          (mod(n/source.pack_size,seat_count_))::integer,printing_id
   FROM (SELECT printing_id,row_number() OVER(ORDER BY gen_random_uuid())-1 AS n
     FROM unnest(source.cube_printings) printing_id) shuffled
   WHERE n<3*seat_count_*source.pack_size;
 END IF;
 RETURN app.finish_account_command(command.id,true,jsonb_build_object('draftId',draft_id_));
END $$;

CREATE FUNCTION app.read_my_limited_draft(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); draft app.limited_draft; member app.limited_draft_seat;
 pack_size_ integer; finished boolean; picked boolean; pack jsonb:='[]'; pool jsonb;
BEGIN
 SELECT * INTO draft FROM app.limited_draft WHERE room_id=p_room;
 SELECT * INTO member FROM app.limited_draft_seat WHERE draft_id=draft.id AND account_id=actor;
 IF member.account_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=actor AND state='active')
    OR NOT EXISTS(SELECT 1 FROM app.room_slot WHERE id=member.slot_id AND retired_at IS NULL AND account_id=actor)
    OR NOT app.limited_draft_roster_valid(draft.id,p_room)
 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('draft','unavailable'));END IF;
 SELECT pack_size INTO STRICT pack_size_ FROM app.limited_draft_source WHERE id=draft.source_id;
 finished:=draft.step=3*pack_size_;
 picked:=EXISTS(SELECT 1 FROM app.limited_draft_pick WHERE draft_id=draft.id AND step=draft.step AND seat=member.seat);
 IF NOT finished AND NOT picked THEN
   SELECT coalesce(jsonb_agg(jsonb_build_object('itemId',item.id,'printingId',item.printing_id) ORDER BY item.id),'[]') INTO pack
   FROM app.limited_draft_item item
   WHERE item.draft_id=draft.id AND item.round_index=draft.step/pack_size_
     AND item.origin=app.limited_draft_origin(member.seat,draft.seat_count,draft.step,pack_size_)
     AND NOT EXISTS(SELECT 1 FROM app.limited_draft_pick pick WHERE pick.item_id=item.id);
 END IF;
 SELECT coalesce(jsonb_agg(item.printing_id ORDER BY pick.step),'[]') INTO pool
 FROM app.limited_draft_pick pick JOIN app.limited_draft_item item ON item.id=pick.item_id
 WHERE pick.draft_id=draft.id AND pick.seat=member.seat;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object(
   'draftId',draft.id,'step',draft.step,'complete',finished,'picked',picked,'pack',pack,'pool',pool));
END $$;

CREATE FUNCTION app.pick_limited_draft_card(p_room uuid,p_draft uuid,p_step integer,p_item uuid,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.limited_draft_actor(); command app.command_receipt; lobby app.room;
 draft app.limited_draft; member app.limited_draft_seat; pack_size_ integer;
 failure jsonb:='{"draft":"unavailable"}';
BEGIN
 IF actor IS NULL OR p_room IS NULL OR p_draft IS NULL OR p_item IS NULL OR p_request IS NULL OR p_step IS NULL
    OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',failure);
 END IF;
 command:=app.begin_account_command(actor,p_request,'limited_draft_pick_v1',p_room::text,
    jsonb_build_object('draft',p_draft,'step',p_step,'item',p_item));
 IF command.state<>'pending' THEN RETURN command.response_payload;END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 SELECT * INTO draft FROM app.limited_draft WHERE id=p_draft AND room_id=p_room FOR NO KEY UPDATE;
 SELECT * INTO member FROM app.limited_draft_seat WHERE draft_id=draft.id AND account_id=actor;
 IF lobby.state IS DISTINCT FROM 'lobby' OR member.account_id IS NULL
    OR NOT app.limited_draft_roster_valid(draft.id,p_room)
 THEN RETURN app.finish_account_command(command.id,false,failure);END IF;
 SELECT pack_size INTO STRICT pack_size_ FROM app.limited_draft_source WHERE id=draft.source_id;
 IF draft.step<>p_step OR draft.step>=3*pack_size_
   OR EXISTS(SELECT 1 FROM app.limited_draft_pick WHERE draft_id=draft.id AND seat=member.seat AND step=p_step)
 THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('step','conflict'));END IF;
 IF NOT EXISTS(SELECT 1 FROM app.limited_draft_item item
    WHERE item.id=p_item AND item.draft_id=draft.id AND item.round_index=p_step/pack_size_
      AND item.origin=app.limited_draft_origin(member.seat,draft.seat_count,p_step,pack_size_)
      AND NOT EXISTS(SELECT 1 FROM app.limited_draft_pick pick WHERE pick.item_id=item.id))
 THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('item','unavailable'));END IF;
 INSERT INTO app.limited_draft_pick(draft_id,step,seat,item_id) VALUES(draft.id,p_step,member.seat,p_item);
 IF (SELECT count(*) FROM app.limited_draft_pick WHERE draft_id=draft.id AND step=p_step)=draft.seat_count THEN
   UPDATE app.limited_draft SET step=step+1 WHERE id=draft.id;
 END IF;
 RETURN app.finish_account_command(command.id,true,jsonb_build_object('draftId',draft.id,'step',p_step::text,'itemId',p_item));
END $$;

CREATE FUNCTION app.seal_my_limited_draft_pool(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.limited_draft_actor(); lobby app.room; draft app.limited_draft;
 member app.limited_draft_seat; source app.limited_draft_source; counts jsonb; total bigint;
BEGIN
 SELECT * INTO lobby FROM app.room WHERE id=p_room AND actor IS NOT NULL FOR NO KEY UPDATE;
 SELECT * INTO draft FROM app.limited_draft WHERE room_id=lobby.id;
 SELECT * INTO member FROM app.limited_draft_seat WHERE draft_id=draft.id AND account_id=actor;
 SELECT * INTO source FROM app.limited_draft_source WHERE id=draft.source_id;
 IF member.account_id IS NULL OR draft.step<>3*source.pack_size
 OR NOT app.limited_draft_roster_valid(draft.id,p_room)
 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('draft','unavailable'));END IF;
 SELECT jsonb_object_agg(items.printing_id,items.quantity::text),sum(items.quantity) INTO counts,total
 FROM (
   SELECT item.printing_id,count(*) quantity FROM app.limited_draft_pick pick
   JOIN app.limited_draft_item item ON item.id=pick.item_id
   WHERE pick.draft_id=draft.id AND pick.seat=member.seat GROUP BY item.printing_id
 ) items;
 IF total IS DISTINCT FROM (3*source.pack_size)::bigint THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='completed draft lacks all seat picks';
 END IF;
 PERFORM app.seal_limited_match_seat_pool(p_room,member.slot_id,draft.id,source.release_id,counts);
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('draftId',draft.id));
END $$;

REVOKE ALL ON FUNCTION app.limited_draft_actor(),app.limited_draft_roster_valid(uuid,uuid),
 app.limited_draft_origin(integer,integer,integer,integer),
 app.start_limited_draft(uuid,uuid,uuid),app.read_my_limited_draft(uuid),
 app.pick_limited_draft_card(uuid,uuid,integer,uuid,uuid),app.seal_my_limited_draft_pool(uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.start_limited_draft(uuid,uuid,uuid),app.read_my_limited_draft(uuid),
 app.pick_limited_draft_card(uuid,uuid,integer,uuid,uuid),app.seal_my_limited_draft_pool(uuid)
 TO authenticated;

COMMIT;
