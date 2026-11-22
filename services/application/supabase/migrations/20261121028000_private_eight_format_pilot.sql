BEGIN;

DO $predecessor$
BEGIN
 IF EXISTS (
    SELECT 1 FROM (VALUES
    ('app.native_constructed_room_policy(jsonb,jsonb,text)', '111aa296c278409b6333029008de922f89685c18eae8466a6aac1e5a62cd7e13'),
    ('app.read_table_profiles(uuid,integer)', '2baeaabc5efb7a0fa92ffe71b31195503afb1efab51e92ad7e84cadf43c7f766'),
    ('app.read_table_profile_configurations(uuid,integer,integer)', '5d25d9d33901c462dc9633020172b37a6d1f299d867d81497ce91ead08f1027c'),
    ('app.create_room(uuid,uuid,uuid,text,boolean,text,uuid)', '68d3cda2ef8efa0feef41456299150c19eb7b3831976bc488bc6fc252ade1464'),
    ('app.join_room(uuid,uuid,text,uuid)', '0a26a9d92095336fffdfc2a5194c193c67e1df09e8a7c8a3ef0c6a89e08cdaf8')
    ) EXPECTED(signature, body_hash)
    LEFT JOIN pg_catalog.pg_proc prior ON prior.oid=pg_catalog.to_regprocedure(expected.signature)
    WHERE pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(prior.prosrc,'UTF8')),'hex')
          IS DISTINCT FROM expected.body_hash OR prior.prosecdef IS DISTINCT FROM true
 ) THEN
    RAISE EXCEPTION 'private pilot predecessor differs from the reviewed schema';
 END IF;
END
$predecessor$;

CREATE TABLE app.private_pilot_profile (
    profile_id uuid PRIMARY KEY REFERENCES app.table_profile_revision(id),
    format_id uuid NOT NULL REFERENCES app.format(id),
    release_id uuid NOT NULL REFERENCES app.qualified_release(id),
    hide_format boolean NOT NULL,
    commander boolean NOT NULL
);
CREATE INDEX private_pilot_profile_format ON app.private_pilot_profile(format_id);
ALTER TABLE app.private_pilot_profile ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.private_pilot_profile FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

CREATE FUNCTION app.private_pilot_has_grant(p_release uuid,p_account uuid,p_commander boolean)
RETURNS boolean LANGUAGE sql STABLE SET search_path='' AS $$
    SELECT CASE WHEN p_commander THEN EXISTS (
        SELECT 1 FROM app.commander_pilot_member
        WHERE release_id=p_release AND account_id=p_account
    ) ELSE EXISTS (
        SELECT 1 FROM app.limited_pilot_member
        WHERE release_id=p_release AND account_id=p_account
    ) END
$$;
REVOKE ALL ON FUNCTION app.private_pilot_has_grant(uuid,uuid,boolean) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

CREATE FUNCTION app.private_pilot_profile_visible(p_profile uuid,p_account uuid)
RETURNS boolean LANGUAGE sql STABLE SET search_path='' AS $$
    SELECT NOT EXISTS (
        SELECT 1 FROM app.private_pilot_profile profile WHERE profile.profile_id=p_profile
        AND NOT app.private_pilot_has_grant(profile.release_id,p_account,profile.commander)
    )
$$;
REVOKE ALL ON FUNCTION app.private_pilot_profile_visible(uuid,uuid) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

CREATE FUNCTION app.private_pilot_format_visible(p_format uuid)
RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
    SELECT NOT EXISTS (
        SELECT 1 FROM app.private_pilot_profile WHERE format_id=p_format AND hide_format
    ) OR EXISTS (
        SELECT 1 FROM app.private_pilot_profile profile WHERE profile.format_id=p_format AND profile.hide_format
        AND app.private_pilot_has_grant(profile.release_id,app.current_account_id(),profile.commander)
    )
$$;
REVOKE ALL ON FUNCTION app.private_pilot_format_visible(uuid) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.private_pilot_format_visible(uuid) TO authenticated;
ALTER POLICY format_catalogue_read ON app.format USING (
    app.current_account_id() IS NOT NULL
    AND (key NOT IN ('draft','cube') OR app.private_limited_format_visible(id))
    AND app.private_pilot_format_visible(id)
);

-- Called under the room write's lock. Hold existing grants through commit so a
-- concurrent revocation either precedes admission or waits for the accepted write.
CREATE FUNCTION app.lock_private_pilot_grant(p_release uuid,p_account uuid,p_commander boolean)
RETURNS boolean LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
    IF p_commander THEN
        PERFORM 1 FROM app.commander_pilot_member
        WHERE release_id=p_release AND account_id=p_account FOR KEY SHARE;
    ELSE
        PERFORM 1 FROM app.limited_pilot_member
        WHERE release_id=p_release AND account_id=p_account FOR KEY SHARE;
    END IF;
    RETURN FOUND;
END $$;
REVOKE ALL ON FUNCTION app.lock_private_pilot_grant(uuid,uuid,boolean) FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.native_constructed_room_policy(p_terms jsonb, p_prepared jsonb, p_kind text)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE format_id_ uuid:=(p_terms->'room'->>'formatId')::uuid;
 legality_id_ uuid:=(p_prepared->>'legalityRevisionId')::uuid; selected uuid[];
 current_key text; proof text:=p_prepared->>'qualificationRef'; statuses jsonb;
BEGIN
 -- A hidden format always requires its own private profile. Public format rows
 -- can still be used by their pre-existing profiles and releases.
 IF EXISTS (
   SELECT 1 FROM app.room lobby JOIN app.private_pilot_profile private_format
     ON private_format.format_id=lobby.format_id AND private_format.hide_format
   WHERE lobby.id=(p_terms->'room'->>'id')::uuid AND NOT EXISTS (
     SELECT 1 FROM app.private_pilot_profile profile
     WHERE profile.profile_id=lobby.profile_id AND profile.format_id=lobby.format_id
   )
 ) THEN RETURN NULL; END IF;
 IF EXISTS (
   SELECT 1 FROM app.room lobby JOIN app.private_pilot_profile profile ON profile.profile_id=lobby.profile_id
   WHERE lobby.id=(p_terms->'room'->>'id')::uuid AND (
     profile.format_id<>lobby.format_id OR profile.release_id IS DISTINCT FROM (p_prepared->>'releaseId')::uuid
     OR lobby.visibility<>'private' OR lobby.mode<>'open' OR lobby.structure<>'bo1' OR lobby.competitive
     OR EXISTS (
       SELECT 1 FROM (
         SELECT slot->>'controllerKind' AS controller, (slot->>'accountId')::uuid AS account
         FROM jsonb_array_elements(p_terms->'slots') slot
         ORDER BY slot->>'accountId'
       ) member
       WHERE member.controller IS DISTINCT FROM 'human' OR
       (member.account IS NOT NULL
        AND NOT app.lock_private_pilot_grant(profile.release_id,member.account,profile.commander))
     )
   )
 ) THEN RETURN NULL; END IF;

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
END $function$;

CREATE OR REPLACE FUNCTION app.read_table_profiles(p_after uuid, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'key',key,'minimumSeats',minimum_seats::text,'seatCapacity',seat_capacity::text,'hasMatchmaking',matchmaking_config IS NOT NULL) body FROM app.table_profile_revision WHERE admission_qualification_ref IS NOT NULL AND app.private_limited_profile_visible(id,actor) AND app.private_pilot_profile_visible(id,actor) AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $function$;

CREATE OR REPLACE FUNCTION app.read_table_profile_configurations(p_profile uuid, p_after integer, p_limit integer)
 RETURNS jsonb
 LANGUAGE plpgsql
 STABLE SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE actor uuid:=app.current_account_id();authority jsonb;configuration jsonb;items jsonb;cursor_index text;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_profile IS NULL OR p_profile='00000000-0000-0000-0000-000000000000'::uuid OR p_after NOT BETWEEN 0 AND 511 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT matchmaking_config INTO configuration FROM app.table_profile_revision WHERE id=p_profile AND admission_qualification_ref IS NOT NULL AND matchmaking_config IS NOT NULL AND app.private_limited_profile_visible(p_profile,actor) AND app.private_pilot_profile_visible(p_profile,actor);
 IF configuration IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('profile','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY ordinal),'[]'::jsonb) INTO items FROM(
 SELECT ordinality-1 ordinal,jsonb_build_object('index',(ordinality-1)::text,'key',value->'key','formatId',value->'formatId','seatCount',value->'seatCount','structure',value->'structure') body FROM jsonb_array_elements(configuration->'configurations') WITH ORDINALITY source(value,ordinality) WHERE p_after IS NULL OR ordinality-1>p_after ORDER BY ordinality LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_index:=items->-1->>'index';END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'profileId',p_profile,'items',items,'nextIndex',cursor_index));
END $function$;

CREATE OR REPLACE FUNCTION app.create_room(p_actor uuid, p_format uuid, p_profile uuid, p_mode text, p_competitive boolean, p_visibility text, p_request uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE receipt app.command_receipt; room_id uuid;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_create',p_actor::text,
  jsonb_build_object('format',p_format,'profile',p_profile,'mode',p_mode,'competitive',p_competitive,'visibility',p_visibility));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF p_mode IS NULL OR p_mode NOT IN ('open','collection') OR p_competitive IS NULL
 OR (p_competitive AND p_mode<>'collection') OR p_visibility IS NULL OR p_visibility NOT IN ('public','private') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','invalid')); END IF;
 PERFORM id FROM app.format WHERE id=p_format FOR KEY SHARE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('format','unavailable')); END IF;
 PERFORM id FROM app.table_profile_revision WHERE id=p_profile FOR KEY SHARE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unavailable')); END IF;
 IF EXISTS (
   SELECT 1 FROM app.private_pilot_profile profile WHERE profile.profile_id=p_profile
   AND (profile.format_id<>p_format OR p_mode<>'open' OR p_competitive OR p_visibility<>'private'
        OR NOT app.lock_private_pilot_grant(profile.release_id,p_actor,profile.commander))
 ) OR EXISTS (
   SELECT 1 FROM app.private_pilot_profile profile WHERE profile.format_id=p_format AND profile.hide_format
 ) AND NOT EXISTS (
   SELECT 1 FROM app.private_pilot_profile profile WHERE profile.format_id=p_format AND profile.profile_id=p_profile
 ) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unavailable')); END IF;
 INSERT INTO app.room(host_id,format_id,profile_id,mode,competitive,visibility,state,revision)
 VALUES(p_actor,p_format,p_profile,p_mode,p_competitive,p_visibility,'lobby',0) RETURNING id INTO room_id;
 INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready)
 VALUES(room_id,p_actor,0,'human',false);
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',room_id));
END $function$;

CREATE OR REPLACE FUNCTION app.join_room(p_actor uuid, p_room uuid, p_token_hash text, p_request uuid)
 RETURNS jsonb
 LANGUAGE plpgsql
 SECURITY DEFINER
 SET search_path TO ''
AS $function$
DECLARE receipt app.command_receipt; lobby app.room; original_host uuid; profile app.table_profile_revision; slot app.room_slot; chosen bigint;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid lobby source request'; END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='lobby and queue sources require read committed retry'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'room_join',coalesce(p_room::text,''),jsonb_build_object('tokenHash',p_token_hash));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 SELECT host_id INTO original_host FROM app.room WHERE id=p_room;
 PERFORM id FROM app.account WHERE id IN (p_actor,original_host) ORDER BY id FOR NO KEY UPDATE;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active') OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=original_host AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF EXISTS (
   SELECT 1 FROM app.private_pilot_profile profile WHERE profile.profile_id=lobby.profile_id
   AND NOT app.lock_private_pilot_grant(profile.release_id,p_actor,profile.commander)
 ) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF lobby.id IS NULL OR lobby.state NOT IN ('lobby','countdown') THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','unavailable')); END IF;
 IF lobby.host_id<>original_host THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room host changed before account fence'; END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id=p_actor AND controller_kind='human';
 IF slot.id IS NOT NULL THEN RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id)); END IF;
 IF lobby.visibility<>'public' AND NOT EXISTS(SELECT 1 FROM app.invitation WHERE room_id=p_room AND token_hash=p_token_hash AND state='active') THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('invitation','unavailable')); END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 IF profile.admission_qualification_ref IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','unqualified')); END IF;
 SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='human' AND account_id IS NULL ORDER BY seat_index LIMIT 1 FOR NO KEY UPDATE;
 IF slot.id IS NULL THEN
  SELECT min(candidate) INTO chosen FROM (SELECT 0::bigint candidate UNION SELECT seat_index+1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND seat_index<profile.seat_capacity-1) candidates
   WHERE candidate<profile.seat_capacity AND NOT EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND seat_index=candidate);
  IF chosen IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('capacity','full')); END IF;
  INSERT INTO app.room_slot(room_id,account_id,seat_index,controller_kind,ready) VALUES(p_room,p_actor,chosen,'human',false) RETURNING * INTO slot;
 ELSE UPDATE app.room_slot SET account_id=p_actor,sponsor_id=NULL,deck_revision_id=NULL,ready=false WHERE id=slot.id;
 END IF;
 UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_room,'slotId',slot.id));
END $function$;

COMMIT;
