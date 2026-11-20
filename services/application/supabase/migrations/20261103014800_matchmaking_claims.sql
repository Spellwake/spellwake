BEGIN;
ALTER TABLE app.room ADD COLUMN target_seat_count bigint CHECK(target_seat_count IS NULL OR target_seat_count BETWEEN 2 AND 6);
ALTER TABLE app.room ADD COLUMN queue_claim_receipt_id uuid REFERENCES app.command_receipt(id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX room_queue_claim_source ON app.room(queue_claim_receipt_id) WHERE queue_claim_receipt_id IS NOT NULL;
CREATE FUNCTION app.claim_matchmaking(p_selection jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; fingerprint text; item jsonb; profile app.table_profile_revision; configuration jsonb;
 human_ids uuid[]; intent_ids uuid[]; actors uuid[]; row_count integer; total_count integer; ai_count integer; host_entry uuid; host_id uuid;
 entry app.queue_entry; sponsor app.queue_entry; choice jsonb; mode_value text; competitive_value boolean; format_value uuid; lobby uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue claim requires read committed retry'; END IF;
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR jsonb_typeof(p_selection) IS DISTINCT FROM 'object' OR octet_length(p_selection::text)>262144
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['profileId','configurationKey','hostEntryId']) k WHERE jsonb_typeof(p_selection->k) IS DISTINCT FROM 'string')
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['profileId','hostEntryId']) k WHERE p_selection->>k !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$')
 OR jsonb_typeof(p_selection->'humanSeats') IS DISTINCT FROM 'array' OR jsonb_typeof(p_selection->'aiSeats') IS DISTINCT FROM 'array'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_selection) k WHERE k NOT IN ('profileId','configurationKey','hostEntryId','humanSeats','aiSeats')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid concrete queue selection'; END IF;
 IF jsonb_array_length(p_selection->'humanSeats') NOT BETWEEN 1 AND 6 OR jsonb_array_length(p_selection->'aiSeats') NOT BETWEEN 0 AND 5 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid selection size'; END IF;
 FOR item IN SELECT * FROM jsonb_array_elements(p_selection->'humanSeats') LOOP
 IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'queueEntryId') IS DISTINCT FROM 'string' OR item->>'queueEntryId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(item->'seatIndex') IS DISTINCT FROM 'string' OR item->>'seatIndex' !~ '^[0-5]$' OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('queueEntryId','seatIndex')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid human selection'; END IF;
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_selection->'aiSeats') LOOP
 IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'sponsorEntryId') IS DISTINCT FROM 'string' OR item->>'sponsorEntryId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(item->'seatIndex') IS DISTINCT FROM 'string' OR item->>'seatIndex' !~ '^[0-5]$' OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('sponsorEntryId','seatIndex')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid AI selection'; END IF;
 END LOOP;
 fingerprint:=encode(sha256(convert_to(p_selection::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('queue-matcher',p_request::text,'queue_claim_v1','queue-matcher',fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='queue-matcher' AND scope='queue_claim_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue claim request terms conflict'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue claim pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT array_agg((x->>'queueEntryId')::uuid ORDER BY x->>'queueEntryId') INTO human_ids FROM jsonb_array_elements(p_selection->'humanSeats') x;
 row_count:=cardinality(human_ids);ai_count:=jsonb_array_length(p_selection->'aiSeats');total_count:=row_count+ai_count;
 IF (SELECT count(DISTINCT id) FROM unnest(human_ids) id)<>row_count OR (SELECT count(DISTINCT x->>'seatIndex') FROM jsonb_array_elements((p_selection->'humanSeats')||(p_selection->'aiSeats')) x)<>total_count
 OR EXISTS(SELECT 1 FROM jsonb_array_elements((p_selection->'humanSeats')||(p_selection->'aiSeats')) x WHERE (x->>'seatIndex')::integer>=total_count)
 OR NOT((p_selection->>'hostEntryId')::uuid=ANY(human_ids)) OR EXISTS(SELECT 1 FROM jsonb_array_elements(p_selection->'aiSeats') x WHERE NOT((x->>'sponsorEntryId')::uuid=ANY(human_ids))) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('selection','duplicate, missing or foreign seat')); END IF;
 SELECT array_agg(DISTINCT account_id ORDER BY account_id),array_agg(DISTINCT intent_receipt_id ORDER BY intent_receipt_id) INTO actors,intent_ids FROM app.queue_entry WHERE id=ANY(human_ids);
 IF cardinality(actors) IS DISTINCT FROM row_count OR cardinality(intent_ids) IS DISTINCT FROM row_count THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('selection','one typed intent per human required')); END IF;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.queue_entry WHERE intent_receipt_id=ANY(intent_ids) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.account WHERE id=ANY(actors) AND state<>'active') OR (SELECT count(*) FROM app.queue_entry WHERE id=ANY(human_ids) AND state='queued' AND intent_receipt_id IS NOT NULL)<>row_count
 OR EXISTS(SELECT 1 FROM app.queue_entry WHERE intent_receipt_id=ANY(intent_ids) AND state IN ('claimed','reserved')) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('queue','intent unavailable')); END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=(p_selection->>'profileId')::uuid FOR SHARE;
 SELECT x INTO configuration FROM jsonb_array_elements(profile.matchmaking_config->'configurations') x WHERE x->>'key'=p_selection->>'configurationKey';
 IF configuration IS NULL OR (configuration->>'seatCount')::integer<>total_count THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','unsupported')); END IF;
 host_entry:=(p_selection->>'hostEntryId')::uuid;
 SELECT account_id,mode,competitive,format_id INTO host_id,mode_value,competitive_value,format_value FROM app.queue_entry WHERE id=host_entry;
 IF format_value::text IS DISTINCT FROM configuration->>'formatId' OR (competitive_value AND ai_count<>0) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('configuration','format or competitive mismatch')); END IF;
 FOR entry IN SELECT * FROM app.queue_entry WHERE id=ANY(human_ids) ORDER BY id LOOP
 PERFORM app.check_queue_intent(entry.id);
 SELECT x INTO choice FROM jsonb_array_elements(entry.preferences->'choices') x WHERE x->>'profileId'=profile.id::text AND x->>'configurationKey'=p_selection->>'configurationKey';
 IF entry.mode<>mode_value OR entry.competitive<>competitive_value OR entry.format_id<>format_value OR choice IS NULL OR choice->'configuration' IS DISTINCT FROM configuration
 OR choice->'influence' IS DISTINCT FROM profile.matchmaking_config->'influence' OR choice->'planechase' IS DISTINCT FROM profile.matchmaking_config->'planechase'
 OR choice->'qualificationRef' IS DISTINCT FROM profile.matchmaking_config->'qualificationRef' OR ai_count>(choice->>'maximumAiSeats')::integer
 OR (SELECT count(*) FROM jsonb_array_elements(p_selection->'aiSeats') x WHERE x->>'sponsorEntryId'=entry.id::text)>(choice->>'maximumSponsoredAiSeats')::integer THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('preferences','selection outside an accepted intent')); END IF;
 PERFORM r.id FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=entry.deck_revision_id AND r.sealed_at IS NOT NULL AND d.source_kind='account' AND d.account_id=entry.account_id AND d.mode=entry.mode AND d.format_id=entry.format_id FOR SHARE OF r,d;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','source changed')); END IF;
 END LOOP;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','queue-claim-v1','selection',p_selection,'configuration',configuration,'intentIds',to_jsonb(intent_ids),'mode',mode_value,'competitive',competitive_value) WHERE id=receipt.id;
 INSERT INTO app.room(host_id,format_id,profile_id,mode,competitive,visibility,state,revision,structure,target_seat_count,queue_claim_receipt_id)
 VALUES(host_id,format_value,profile.id,mode_value,competitive_value,'private','lobby',0,configuration->>'structure',total_count,receipt.id) RETURNING id INTO lobby;
 FOR item IN SELECT * FROM jsonb_array_elements(p_selection->'humanSeats') LOOP
 SELECT * INTO STRICT entry FROM app.queue_entry WHERE id=(item->>'queueEntryId')::uuid;
 INSERT INTO app.room_slot(room_id,account_id,deck_revision_id,seat_index,team_ref,controller_kind,ready)
 VALUES(lobby,entry.account_id,entry.deck_revision_id,(item->>'seatIndex')::bigint,configuration->'teams'->>((item->>'seatIndex')::integer),'human',false);
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(p_selection->'aiSeats') LOOP
 SELECT * INTO STRICT sponsor FROM app.queue_entry WHERE id=(item->>'sponsorEntryId')::uuid;
 INSERT INTO app.room_slot(room_id,sponsor_id,seat_index,team_ref,controller_kind,ready)
 VALUES(lobby,sponsor.account_id,(item->>'seatIndex')::bigint,configuration->'teams'->>((item->>'seatIndex')::integer),'ai',false);
 END LOOP;
 UPDATE app.queue_entry SET state='claimed',claimed_room_id=lobby,claim_receipt_id=receipt.id WHERE id=ANY(human_ids);
 UPDATE app.queue_entry SET state='canceled' WHERE intent_receipt_id=ANY(intent_ids) AND NOT(id=ANY(human_ids)) AND state='queued';
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',lobby));
END $$;
REVOKE ALL ON FUNCTION app.claim_matchmaking(jsonb,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.refresh_room_countdown(p_room uuid,p_force boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE lobby app.room; profile app.table_profile_revision; occupied bigint;
BEGIN
 IF p_force IS NULL THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='countdown trigger kind required'; END IF;
 SELECT * INTO STRICT lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.state='countdown' THEN RETURN false; END IF;
 IF lobby.state<>'lobby' THEN RETURN false; END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=lobby.profile_id;
 SELECT count(*) INTO occupied FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai');
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN RETURN false; END IF;
 IF profile.admission_qualification_ref IS NULL OR occupied NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
 OR EXISTS(SELECT 1 FROM app.room_slot s LEFT JOIN app.deck_revision revision ON revision.id=s.deck_revision_id LEFT JOIN app.deck d ON d.id=revision.deck_id
  WHERE s.retired_at IS NULL AND s.room_id=p_room AND (s.account_id IS NOT NULL OR s.controller_kind='ai') AND
   (revision.id IS NULL OR revision.sealed_at IS NULL OR d.mode<>lobby.mode OR d.format_id<>lobby.format_id
    OR (s.controller_kind='human' AND (d.source_kind<>'account' OR d.account_id IS DISTINCT FROM s.account_id))
    OR (s.controller_kind='ai' AND NOT(d.source_kind='system_pool' OR (d.source_kind='account' AND d.account_id=s.sponsor_id))))) THEN RETURN false; END IF;
 IF occupied<>coalesce(lobby.target_seat_count,profile.seat_capacity) THEN RETURN false; END IF;
 IF NOT p_force AND (EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') AND NOT ready)) THEN RETURN false; END IF;
 UPDATE app.room SET state='countdown',countdown_at=clock_timestamp()+interval '10 seconds' WHERE id=p_room;
 RETURN true;
END $$;
COMMIT;
