BEGIN;

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
    AND (slot.controller_kind<>'human' OR slot.team_ref IS NOT NULL OR slot.ready OR a.state IS DISTINCT FROM 'active')
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
