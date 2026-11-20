BEGIN;
ALTER TABLE app.match ADD COLUMN accepted_release_id uuid REFERENCES app.qualified_release(id) ON DELETE RESTRICT;
ALTER TABLE app.match ADD COLUMN admission_qualification_ref text;
ALTER TABLE app.match ADD CONSTRAINT accepted_release_source_tuple CHECK(
 (accepted_release_id IS NULL AND admission_qualification_ref IS NULL)
 OR (accepted_release_id IS NOT NULL AND admission_qualification_ref IS NOT NULL AND length(btrim(admission_qualification_ref))>0));
CREATE FUNCTION app.accept_match_decks(p_match uuid,p_expected numeric,p_release uuid,p_qualification text,p_inputs jsonb,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; source_match app.match; profile app.table_profile_revision;
 actors uuid[]; actor uuid; item jsonb; seat app.match_seat; revision app.deck_revision; library app.deck;
 accepted_at timestamptz; fingerprint text; required_count bigint;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='match deck acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid
 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_release IS NULL OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_inputs) IS DISTINCT FROM 'array' OR octet_length(p_inputs::text)>1048576 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid admission identity or artifact envelope'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('basis',p_expected::text,'release',p_release,'qualification',p_qualification,'inputs',p_inputs)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('admission:'||p_match::text,p_request::text,'match_decks_v1',p_match::text,fingerprint,'pending','')
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='admission:'||p_match::text AND scope='match_decks_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_match::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='admission request terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='admission disposition is pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 SELECT array_agg(DISTINCT a ORDER BY a) INTO actors FROM(
  SELECT account_id a FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL
  UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO source_match FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 IF source_match.id IS NULL OR source_match.state<>'admitting' OR source_match.revision<>p_expected OR EXISTS(SELECT 1 FROM app.game WHERE match_id=p_match)
 OR EXISTS(SELECT 1 FROM app.match_result WHERE match_id=p_match) OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=p_match AND deck_accepted_at IS NOT NULL) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('match','stale, accepted or started')); END IF;
 IF EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=p_match AND
  ((account_id IS NOT NULL AND NOT (account_id=ANY(coalesce(actors,'{}'::uuid[]))))
   OR (sponsor_id IS NOT NULL AND NOT(sponsor_id=ANY(coalesce(actors,'{}'::uuid[])))))) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='admission roster changed before account fencing'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=members.actor_id AND a.state='active')) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO STRICT profile FROM app.table_profile_revision WHERE id=source_match.profile_id;
 SELECT count(*) INTO required_count FROM app.match_seat WHERE match_id=p_match;
 IF profile.admission_qualification_ref IS NULL OR required_count NOT BETWEEN profile.minimum_seats AND profile.seat_capacity
 OR jsonb_array_length(p_inputs)<>required_count OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=p_match AND (seat_index<0 OR seat_index>=profile.seat_capacity)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('profile','qualified roster and complete inputs required')); END IF;
 PERFORM id FROM app.qualified_release WHERE id=p_release FOR SHARE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('release','unavailable')); END IF;
 -- Freeze all dictionary additions during qualification and acceptance.
 PERFORM id FROM app.legality_revision WHERE id=source_match.legality_revision_id FOR SHARE;
 FOR item IN SELECT v FROM jsonb_array_elements(p_inputs) v ORDER BY v->>'seatId' LOOP
  IF jsonb_typeof(item) IS DISTINCT FROM 'object'
   OR jsonb_typeof(item->'seatId') IS DISTINCT FROM 'string' OR (item->>'seatId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
   OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
   OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('seatId','revisionId','snapshotRef')) THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid accepted seat input'; END IF;
  SELECT * INTO seat FROM app.match_seat WHERE id=(item->>'seatId')::uuid AND match_id=p_match FOR NO KEY UPDATE;
  IF seat.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('seat','unavailable')); END IF;
  SELECT * INTO revision FROM app.deck_revision WHERE id=(item->>'revisionId')::uuid FOR SHARE;
  SELECT * INTO library FROM app.deck WHERE id=revision.deck_id FOR SHARE;
  IF revision.id IS NULL OR revision.sealed_at IS NULL OR library.id IS NULL
   OR library.mode<>source_match.mode OR library.format_id<>source_match.format_id
   OR (seat.controller_kind='human' AND (seat.sponsor_id IS NOT NULL OR library.source_kind<>'account' OR library.account_id IS DISTINCT FROM seat.account_id))
   OR (seat.controller_kind='ai' AND NOT(library.source_kind='system_pool' OR (library.source_kind='account' AND library.account_id=seat.sponsor_id)))
   OR NOT EXISTS(SELECT 1 FROM app.deck_entry WHERE revision_id=revision.id) THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable or incompatible')); END IF;
  IF EXISTS(SELECT 1 FROM app.deck_entry e WHERE e.revision_id=revision.id AND
   ((SELECT count(*) FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id=program.definition_revision_id WHERE program.release_id=p_release AND body.definition_id=e.definition_id)<>1
    OR NOT EXISTS(SELECT 1 FROM app.card_legality l WHERE l.definition_id=e.definition_id AND l.format_id=source_match.format_id AND l.legality_revision_id=source_match.legality_revision_id AND l.status IN ('legal','restricted')))) THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('cards','unsupported or not legal in accepted revision')); END IF;
 END LOOP;
 IF (SELECT count(DISTINCT v->>'seatId') FROM jsonb_array_elements(p_inputs) v)<>required_count THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('seat','duplicate input')); END IF;
 -- Account-first and inventory-row fences serialize against new holds/transfers.
 PERFORM i.id FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id
 WHERE o.account_id=ANY(actors) ORDER BY i.id FOR NO KEY UPDATE OF i;
 IF source_match.mode='collection' AND EXISTS(
  SELECT 1 FROM jsonb_array_elements(p_inputs) input JOIN app.match_seat s ON s.id=(input->>'seatId')::uuid
  JOIN app.deck_entry e ON e.revision_id=(input->>'revisionId')::uuid
  WHERE s.controller_kind='human' AND NOT EXISTS(
   SELECT 1 FROM app.release_card_program program JOIN app.card_definition_revision body ON body.id=program.definition_revision_id
   WHERE program.release_id=p_release AND body.definition_id=e.definition_id AND program.unlimited_basic_land)
  GROUP BY s.account_id,e.printing_id HAVING e.printing_id IS NULL OR sum(e.quantity)>
   coalesce((SELECT i.owned_quantity-coalesce((SELECT sum(line.quantity) FROM app.hold_line line JOIN app.asset_hold h ON h.id=line.hold_id WHERE line.inventory_asset_id=i.id AND h.state='held'),0)
    FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_definition d ON d.id=i.asset_definition_id
    WHERE o.account_id=s.account_id AND o.state='active' AND d.kind='card' AND d.printing_id=e.printing_id),0)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('collection','not enough available copies')); END IF;
 accepted_at:=clock_timestamp();
 FOR item IN SELECT v FROM jsonb_array_elements(p_inputs) v ORDER BY v->>'seatId' LOOP
  UPDATE app.match_seat SET source_revision_id=(item->>'revisionId')::uuid,deck_snapshot_ref=item->>'snapshotRef',deck_accepted_at=accepted_at WHERE id=(item->>'seatId')::uuid;
 END LOOP;
 UPDATE app.match m SET accepted_release_id=p_release,admission_qualification_ref=p_qualification,revision=m.revision+1 WHERE m.id=p_match;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_match,'revision',(p_expected+1)::text,'releaseId',p_release,'qualificationRef',p_qualification));
END $$;

CREATE FUNCTION app.guard_accepted_match_inputs() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE targets uuid[];
BEGIN
 IF TG_TABLE_NAME='match' THEN
  IF (NEW.id,NEW.room_id,NEW.format_id,NEW.profile_id,NEW.legality_revision_id,NEW.mode,NEW.competitive,NEW.structure) IS NOT DISTINCT FROM
     (OLD.id,OLD.room_id,OLD.format_id,OLD.profile_id,OLD.legality_revision_id,OLD.mode,OLD.competitive,OLD.structure) THEN RETURN NULL; END IF;
  targets:=ARRAY[OLD.id,NEW.id];
 ELSE
  IF TG_OP='UPDATE' AND (NEW.id,NEW.match_id,NEW.account_id,NEW.sponsor_id,NEW.seat_index,NEW.team_ref,NEW.controller_kind) IS NOT DISTINCT FROM
      (OLD.id,OLD.match_id,OLD.account_id,OLD.sponsor_id,OLD.seat_index,OLD.team_ref,OLD.controller_kind) THEN RETURN NULL; END IF;
  IF TG_OP<>'INSERT' AND OLD.deck_accepted_at IS NOT NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted match configuration and roster are immutable'; END IF;
  IF TG_OP='INSERT' THEN targets:=ARRAY[NEW.match_id]; ELSIF TG_OP='DELETE' THEN targets:=ARRAY[OLD.match_id]; ELSE targets:=ARRAY[OLD.match_id,NEW.match_id]; END IF;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='accepted match input changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(targets) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=ANY(targets) AND deck_accepted_at IS NOT NULL) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted match configuration and roster are immutable'; END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER accepted_match_configuration AFTER UPDATE ON app.match FOR EACH ROW EXECUTE FUNCTION app.guard_accepted_match_inputs();
CREATE TRIGGER accepted_match_roster AFTER INSERT OR UPDATE OR DELETE ON app.match_seat FOR EACH ROW EXECUTE FUNCTION app.guard_accepted_match_inputs();

CREATE FUNCTION app.guard_match_release_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF OLD.accepted_release_id IS NOT NULL AND (NEW.accepted_release_id,NEW.admission_qualification_ref) IS DISTINCT FROM (OLD.accepted_release_id,OLD.admission_qualification_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted match release source is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER match_release_history BEFORE UPDATE ON app.match FOR EACH ROW EXECUTE FUNCTION app.guard_match_release_history();
CREATE FUNCTION app.check_match_release_source() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE current_match app.match;
BEGIN
 SELECT * INTO current_match FROM app.match WHERE id=NEW.id;
 IF current_match.accepted_release_id IS NULL THEN RETURN NULL; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=current_match.id)
 OR EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=current_match.id AND deck_accepted_at IS NULL)
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE principal_key='admission:'||current_match.id::text
   AND scope='match_decks_v1' AND target_ref=current_match.id::text AND state='accepted'
   AND response_payload->'data'->>'releaseId'=current_match.accepted_release_id::text
   AND response_payload->'data'->>'qualificationRef'=current_match.admission_qualification_ref) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted match requires its complete source-qualified deck acceptance'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER match_release_source AFTER INSERT OR UPDATE ON app.match
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_match_release_source();
CREATE FUNCTION app.guard_admitted_game_context() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_match uuid;
BEGIN
 IF TG_TABLE_NAME='game_context' THEN
  SELECT match_id INTO source_match FROM app.game WHERE id=NEW.game_id FOR SHARE;
 ELSIF TG_TABLE_NAME='game' THEN
  IF TG_OP='UPDATE' AND NEW.match_id IS NOT DISTINCT FROM OLD.match_id THEN RETURN NULL; END IF;
  source_match:=NEW.match_id;
 ELSE
  IF TG_OP='UPDATE' AND (NEW.profile_id,NEW.accepted_release_id) IS NOT DISTINCT FROM (OLD.profile_id,OLD.accepted_release_id) THEN RETURN NULL; END IF;
  source_match:=NEW.id;
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='context admission binding requires read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=source_match FOR SHARE;
 IF EXISTS(SELECT 1 FROM app.game_context context JOIN app.game g ON g.id=context.game_id JOIN app.match m ON m.id=g.match_id
  WHERE g.match_id=source_match AND (context.profile_id<>m.profile_id OR (m.accepted_release_id IS NOT NULL AND context.release_id<>m.accepted_release_id))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game context must preserve its admitted profile and executable release'; END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER admitted_game_context AFTER INSERT ON app.game_context
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_admitted_game_context();
CREATE CONSTRAINT TRIGGER admitted_game_parent AFTER INSERT OR UPDATE ON app.game
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_admitted_game_context();

CREATE CONSTRAINT TRIGGER admitted_match_context AFTER INSERT OR UPDATE ON app.match
 DEFERRABLE INITIALLY IMMEDIATE FOR EACH ROW EXECUTE FUNCTION app.guard_admitted_game_context();

CREATE OR REPLACE FUNCTION app.guard_reference_dictionary() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='reference publication requires read committed retry'; END IF;
 IF TG_TABLE_NAME='release_card_program' THEN
  PERFORM id FROM app.qualified_release WHERE id=NEW.release_id FOR UPDATE;
  IF EXISTS(SELECT 1 FROM app.game_context WHERE release_id=NEW.release_id)
   OR EXISTS(SELECT 1 FROM app.command_receipt WHERE scope='match_decks_v1' AND state='accepted' AND response_payload->'data'->>'releaseId'=NEW.release_id::text) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted release program set is immutable'; END IF;
 ELSE
  PERFORM id FROM app.legality_revision WHERE id=NEW.legality_revision_id FOR UPDATE;
  IF EXISTS(SELECT 1 FROM app.match WHERE legality_revision_id=NEW.legality_revision_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted legality set is immutable'; END IF;
 END IF;
 RETURN NEW;
END $$;

REVOKE ALL ON FUNCTION app.guard_match_release_history(),app.check_match_release_source(),app.guard_admitted_game_context(),app.accept_match_decks(uuid,numeric,uuid,text,jsonb,uuid),app.guard_accepted_match_inputs()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
