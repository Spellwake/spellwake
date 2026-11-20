BEGIN;
CREATE FUNCTION app.reply_between_games(p_decision uuid,p_expected numeric,p_revision uuid,p_reply text,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match; seat app.match_seat; chosen app.deck_revision; original app.deck_revision; receipt app.command_receipt;
 fingerprint text; game_id uuid; prior_index numeric; current_clock numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='between-game selection requires read committed retry'; END IF;
 IF p_decision IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_revision IS NULL OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_reply IS NULL OR length(btrim(p_reply)) NOT BETWEEN 1 AND 2048 OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized between-game selection'; END IF;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 IF d.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='between-game decision unavailable'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('decision',p_decision,'basis',p_expected::text,'revision',p_revision,'reply',p_reply,'qualification',p_qualification));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-seat:'||d.seat_id::text,'between_game_reply_v1',d.id::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-seat:'||d.seat_id::text AND scope='between_game_reply_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>d.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game reply terms changed'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='between-game reply pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=d.match_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT d FROM app.between_game_decision WHERE id=d.id FOR NO KEY UPDATE;
 SELECT * INTO STRICT seat FROM app.match_seat WHERE id=d.seat_id FOR SHARE;
 SELECT clock_revision INTO current_clock FROM app.match_clock WHERE match_seat_id=seat.id FOR SHARE;
 IF m.revision<>p_expected OR m.state<>'between_games' OR d.state<>'pending' OR d.after_game_id IS NULL OR current_clock IS DISTINCT FROM d.clock_revision
 OR EXISTS(SELECT 1 FROM app.game g JOIN app.game prior ON prior.id=d.after_game_id WHERE g.match_id=m.id AND g.game_index>prior.game_index)
 OR (d.deadline_at IS NOT NULL AND d.deadline_at<=clock_timestamp()) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable between-game decision')); END IF;
 SELECT * INTO chosen FROM app.deck_revision WHERE id=p_revision FOR SHARE;
 SELECT * INTO original FROM app.deck_revision WHERE id=seat.source_revision_id FOR SHARE;
 IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.deck deck WHERE deck.id=chosen.deck_id AND ((seat.controller_kind='human' AND deck.source_kind='account' AND deck.account_id=seat.account_id) OR (seat.controller_kind='ai' AND (deck.source_kind='system_pool' OR deck.account_id=seat.sponsor_id)))) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable selected revision')); END IF;
 IF EXISTS((SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id)
 UNION ALL (SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id)) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','selection changes the immutable original card pool')); END IF;
 -- Fixed legality and execution-release validation are supplied by the trusted
 -- normalized validator. Current collection ownership is deliberately not read.
 UPDATE app.between_game_decision SET state='answered',selected_deck_revision_id=p_revision,reply_ref=p_reply WHERE id=d.id;
 IF NOT EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND after_game_id=d.after_game_id AND state='pending') THEN
 SELECT game_index INTO STRICT prior_index FROM app.game WHERE id=d.after_game_id;
 INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES(m.id,prior_index+1,'reserved',0) RETURNING id INTO game_id;
 UPDATE app.match SET state='ready',reserved_game_id=game_id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(m.id,game_id,game_id::text,'game_host','reserved_game','pending');
 ELSE UPDATE app.match SET revision=revision+1 WHERE id=m.id; END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',d.id,'revision',(m.revision+1)::text,'reservedGameId',coalesce(game_id::text,''),'qualificationRef',p_qualification));
END $$;
CREATE FUNCTION app.guard_between_game_history() RETURNS trigger
LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF TG_OP='INSERT' THEN
  IF NEW.after_game_id IS NOT NULL AND (NEW.state<>'pending' OR NEW.reply_ref IS NOT NULL OR NEW.selected_deck_revision_id IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new normalized decision must be unanswered'; END IF;
  RETURN NEW;
 END IF;
 IF OLD.after_game_id IS NULL THEN IF TG_OP='DELETE' THEN RETURN OLD; END IF; RETURN NEW; END IF;
 IF TG_OP='DELETE' OR OLD.state<>'pending' OR (NEW.id,NEW.match_id,NEW.seat_id,NEW.decision_kind,NEW.basis_revision,NEW.after_game_id,NEW.clock_revision) IS DISTINCT FROM (OLD.id,OLD.match_id,OLD.seat_id,OLD.decision_kind,OLD.basis_revision,OLD.after_game_id,OLD.clock_revision)
 OR NEW.state NOT IN ('pending','answered','expired') OR (NEW.state='answered' AND (NEW.selected_deck_revision_id IS NULL OR NEW.reply_ref IS NULL))
 OR (NEW.state<>'answered' AND (NEW.selected_deck_revision_id IS NOT NULL OR NEW.reply_ref IS NOT NULL)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game source and completed decisions are immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER between_game_history BEFORE INSERT OR UPDATE OR DELETE ON app.between_game_decision FOR EACH ROW EXECUTE FUNCTION app.guard_between_game_history();
REVOKE ALL ON FUNCTION app.reply_between_games(uuid,numeric,uuid,text,text,uuid),app.guard_between_game_history()
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
