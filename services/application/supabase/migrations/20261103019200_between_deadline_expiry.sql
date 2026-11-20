BEGIN;
CREATE FUNCTION app.claim_between_game_reply(p_decision uuid,p_expected numeric,p_revision uuid,p_reply text,p_qualification text,p_request uuid) RETURNS app.command_receipt
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
 RETURN receipt;
 END IF;
 RETURN receipt;
END $$;
CREATE FUNCTION app.apply_between_game_reply(p_receipt uuid,p_decision uuid,p_expected numeric,p_revision uuid,p_reply text,p_qualification text,p_expiry uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; m app.match; seat app.match_seat; chosen app.deck_revision; original app.deck_revision; receipt app.command_receipt; expiry app.command_receipt;
 game_id uuid; prior_index numeric; current_clock numeric; input jsonb;accepted_at timestamptz;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='between-game selection requires read committed retry';END IF;
 input:=jsonb_build_object('decision',p_decision,'basis',p_expected::text,'revision',p_revision,'reply',p_reply,'qualification',p_qualification);
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt;
 IF receipt.state IS DISTINCT FROM 'pending' OR receipt.scope IS DISTINCT FROM 'between_game_reply_v1' OR receipt.principal_key IS DISTINCT FROM 'match-seat:'||d.seat_id::text OR receipt.target_ref IS DISTINCT FROM d.id::text OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(input) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='locked reply core requires its preclaimed exact identity';END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=d.match_id FOR NO KEY UPDATE;
 SELECT * INTO STRICT d FROM app.between_game_decision WHERE id=d.id FOR NO KEY UPDATE;
 SELECT * INTO STRICT seat FROM app.match_seat WHERE id=d.seat_id FOR SHARE;
 SELECT clock_revision INTO current_clock FROM app.match_clock WHERE match_seat_id=seat.id FOR SHARE;
 IF m.availability<>'available' OR m.revision<>p_expected OR m.state<>'between_games' OR d.state<>'pending' OR d.after_game_id IS NULL OR current_clock IS DISTINCT FROM d.clock_revision
 OR EXISTS(SELECT 1 FROM app.game g JOIN app.game prior ON prior.id=d.after_game_id WHERE g.match_id=m.id AND g.game_index>prior.game_index)
 THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('basis','stale or unavailable between-game decision')); END IF;
 PERFORM app.check_started_match_frontier(m.id);PERFORM app.check_between_deadline(d.id);
 SELECT * INTO chosen FROM app.deck_revision WHERE id=p_revision FOR SHARE;
 SELECT * INTO original FROM app.deck_revision WHERE id=seat.source_revision_id FOR SHARE;
 IF chosen.id IS NULL OR chosen.sealed_at IS NULL OR original.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.deck deck WHERE deck.id=chosen.deck_id AND ((seat.controller_kind='human' AND deck.source_kind='account' AND deck.account_id=seat.account_id) OR (seat.controller_kind='ai' AND (deck.source_kind='system_pool' OR deck.account_id=seat.sponsor_id)))) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','unavailable selected revision')); END IF;
 IF EXISTS((SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id)
 UNION ALL (SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=original.id GROUP BY definition_id,printing_id EXCEPT SELECT definition_id,printing_id,sum(quantity) FROM app.deck_entry WHERE revision_id=chosen.id GROUP BY definition_id,printing_id)) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','selection changes the immutable original card pool')); END IF;
 accepted_at:=clock_timestamp();
 IF d.deadline_policy_ref IS NOT NULL AND d.deadline_at<=accepted_at THEN
 IF p_expiry IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deadline','qualified cutoff has elapsed'));END IF;
 SELECT * INTO expiry FROM app.command_receipt WHERE id=p_expiry;
 IF expiry.state IS DISTINCT FROM 'pending' OR expiry.scope IS DISTINCT FROM 'between_deadline_expiry_v1' OR expiry.principal_key IS DISTINCT FROM 'between-expiry:'||d.id::text OR expiry.target_ref IS DISTINCT FROM d.id::text OR expiry.normalized_source->>'replyReceiptId' IS DISTINCT FROM receipt.id::text OR expiry.normalized_source->'replyInput' IS DISTINCT FROM input OR expiry.normalized_source->'snapshot' IS DISTINCT FROM app.between_deadline_snapshot(d.id) OR expiry.normalized_source->>'observedAt' IS NULL OR app.financial_instant(expiry.normalized_source->>'observedAt')<d.deadline_at OR app.financial_instant(expiry.normalized_source->>'observedAt')>accepted_at THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='expired selection requires its exact qualified expiry producer';END IF;
 ELSIF p_expiry IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='qualified deadline is not due';END IF;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','between-game-reply-v1','input',input,'acceptedAt',to_char(accepted_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'deadlineSnapshot',CASE WHEN d.deadline_policy_ref IS NOT NULL THEN app.between_deadline_snapshot(d.id) END,'expiryReceiptId',p_expiry) WHERE id=receipt.id;
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
CREATE OR REPLACE FUNCTION app.reply_between_games(p_decision uuid,p_expected numeric,p_revision uuid,p_reply text,p_qualification text,p_request uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.claim_between_game_reply(p_decision,p_expected,p_revision,p_reply,p_qualification,p_request);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 RETURN app.apply_between_game_reply(receipt.id,p_decision,p_expected,p_revision,p_reply,p_qualification,NULL);
END $$;

-- Both command identities are claimed before the common match fence. A delivery
-- cannot acquire a reply receipt while holding the match against a human reply.
CREATE FUNCTION app.expire_between_game_deadline(p_decision uuid,p_deadline_revision numeric,p_revision uuid,p_reply text,p_qualification text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; inner_receipt app.command_receipt; d app.between_game_decision; m app.match; expected numeric; input jsonb; reply_input jsonb; at_time timestamptz; result jsonb; discovered uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='deadline expiry requires read committed retry';END IF;
 IF p_deadline_revision IS NULL OR p_deadline_revision::text !~ '^[1-9][0-9]{0,77}$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='expiry requires a qualified deadline generation';END IF;
 input:=jsonb_build_object('decisionId',p_decision,'deadlineRevision',p_deadline_revision::text,'selectedRevisionId',p_revision,'replyRef',p_reply,'proofRef',p_qualification);
 receipt:=app.begin_between_deadline_command('between_deadline_expiry_v1',p_decision,p_request,input);
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload;END IF;
 SELECT match_id INTO discovered FROM app.between_game_decision WHERE id=p_decision;
 SELECT revision INTO expected FROM app.match WHERE id=discovered;
 inner_receipt:=app.claim_between_game_reply(p_decision,expected,p_revision,p_reply,p_qualification,receipt.id);
 IF inner_receipt.state<>'pending' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='new expiry cannot reuse an existing reply disposition';END IF;
 SELECT * INTO m FROM app.match WHERE id=discovered FOR NO KEY UPDATE;
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision FOR NO KEY UPDATE;
 IF m.id IS NULL OR d.id IS NULL OR d.match_id IS DISTINCT FROM m.id OR m.revision IS DISTINCT FROM expected OR d.deadline_revision<>p_deadline_revision OR m.availability<>'available' OR m.state<>'between_games' OR d.state<>'pending' OR d.deadline_at IS NULL OR d.deadline_policy_ref IS NULL THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='expiry delivery is stale; rediscover current decision';END IF;
 PERFORM app.check_started_match_frontier(m.id);PERFORM app.check_between_deadline(d.id);
 at_time:=clock_timestamp();IF at_time<d.deadline_at THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='expiry delivery arrived before its qualified cutoff';END IF;
 reply_input:=jsonb_build_object('decision',d.id,'basis',expected::text,'revision',p_revision,'reply',p_reply,'qualification',p_qualification);
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','between-deadline-expiry-v1','input',input,'snapshot',app.between_deadline_snapshot(d.id),'observedAt',to_char(at_time AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'replyReceiptId',inner_receipt.id,'replyInput',reply_input) WHERE id=receipt.id;
 result:=app.apply_between_game_reply(inner_receipt.id,d.id,expected,p_revision,p_reply,p_qualification,receipt.id);
 RETURN app.finish_account_command(receipt.id,result->>'status'='success',CASE WHEN result->>'status'='success' THEN jsonb_build_object('resourceId',d.id,'replyReceiptId',inner_receipt.id,'deadlineRevision',d.deadline_revision::text,'revision',result->'data'->>'revision','reservedGameId',result->'data'->>'reservedGameId') ELSE result->'data' END);
END $$;

CREATE UNIQUE INDEX between_reply_once ON app.command_receipt(target_ref) WHERE scope='between_game_reply_v1' AND state='accepted';
CREATE UNIQUE INDEX between_expiry_once ON app.command_receipt(target_ref) WHERE scope='between_deadline_expiry_v1' AND state='accepted';
CREATE FUNCTION app.check_between_expiry(p_receipt uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; reply app.command_receipt; d app.between_game_decision; source jsonb; input jsonb;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_receipt;
 IF receipt.scope IS DISTINCT FROM 'between_deadline_expiry_v1' OR receipt.state<>'accepted' THEN RETURN;END IF;
 source:=receipt.normalized_source;input:=source->'input';
 SELECT * INTO d FROM app.between_game_decision WHERE id::text=receipt.target_ref;
 SELECT * INTO reply FROM app.command_receipt WHERE id::text=source->>'replyReceiptId';
 IF d.id IS NULL OR d.state<>'answered' OR receipt.principal_key IS DISTINCT FROM 'between-expiry:'||d.id::text OR source->>'version' IS DISTINCT FROM 'between-deadline-expiry-v1' OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(input) OR input->>'decisionId' IS DISTINCT FROM d.id::text OR input->>'deadlineRevision' IS DISTINCT FROM source->'snapshot'->>'revision' OR input->>'selectedRevisionId' IS DISTINCT FROM d.selected_deck_revision_id::text OR input->>'replyRef' IS DISTINCT FROM d.reply_ref OR input->>'proofRef' IS DISTINCT FROM source->'replyInput'->>'qualification'
 OR source->'snapshot' IS DISTINCT FROM app.between_deadline_snapshot(d.id) OR source->'snapshot'->>'dueAt' IS NULL OR source->>'observedAt' IS NULL OR app.financial_instant(source->>'observedAt')<app.financial_instant(source->'snapshot'->>'accountedAt') OR app.financial_instant(source->>'observedAt')>clock_timestamp() OR app.financial_instant(source->>'observedAt')<app.financial_instant(source->'snapshot'->>'dueAt')
 OR reply.state IS DISTINCT FROM 'accepted' OR reply.scope IS DISTINCT FROM 'between_game_reply_v1' OR reply.principal_key IS DISTINCT FROM 'match-seat:'||d.seat_id::text OR reply.target_ref IS DISTINCT FROM d.id::text OR reply.request_ref IS DISTINCT FROM receipt.id::text OR reply.normalized_source->>'expiryReceiptId' IS DISTINCT FROM receipt.id::text OR reply.normalized_source->'input' IS DISTINCT FROM source->'replyInput' OR reply.payload_hash IS DISTINCT FROM app.auth_json_hash(source->'replyInput') OR reply.normalized_source->'deadlineSnapshot' IS DISTINCT FROM source->'snapshot' OR app.financial_instant(reply.normalized_source->>'acceptedAt')<app.financial_instant(source->>'observedAt')
 OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM d.id::text OR receipt.response_payload->'data'->>'replyReceiptId' IS DISTINCT FROM reply.id::text OR receipt.response_payload->'data'->>'deadlineRevision' IS DISTINCT FROM d.deadline_revision::text OR receipt.response_payload->'data'->>'revision' IS DISTINCT FROM reply.response_payload->'data'->>'revision' OR receipt.response_payload->'data'->>'reservedGameId' IS DISTINCT FROM reply.response_payload->'data'->>'reservedGameId' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted deadline expiry must retain its exact qualified reply and cutoff';END IF;
 PERFORM app.check_between_deadline_snapshot(source->'snapshot');
END $$;
CREATE OR REPLACE FUNCTION app.check_between_game_answer(p_decision uuid) RETURNS void LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE d app.between_game_decision; receipt app.command_receipt; terms jsonb; revision text; qualification text; source jsonb; accepted_at timestamptz;
BEGIN
 SELECT * INTO d FROM app.between_game_decision WHERE id=p_decision;
 IF d.id IS NULL OR d.state<>'answered' THEN RETURN;END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE scope='between_game_reply_v1' AND principal_key='match-seat:'||d.seat_id::text AND target_ref=d.id::text AND state='accepted';
 revision:=receipt.response_payload->'data'->>'revision';qualification:=receipt.response_payload->'data'->>'qualificationRef';source:=receipt.normalized_source;
 IF revision IS NULL OR revision !~ '^[1-9][0-9]{0,77}$' OR qualification IS NULL OR length(btrim(qualification)) NOT BETWEEN 1 AND 2048 OR receipt.response_payload->'data'->>'resourceId' IS DISTINCT FROM d.id::text OR source->>'version' IS DISTINCT FROM 'between-game-reply-v1' OR source->>'acceptedAt' IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='answered selection requires its own exact accepted reply source';END IF;
 accepted_at:=app.financial_instant(source->>'acceptedAt');
 terms:=jsonb_build_object('decision',d.id,'basis',((revision::numeric)-1)::text,'revision',d.selected_deck_revision_id,'reply',d.reply_ref,'qualification',qualification);
 IF receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(terms) OR source->'input' IS DISTINCT FROM terms OR accepted_at>clock_timestamp() OR accepted_at<d.opened_at OR source->'deadlineSnapshot' IS DISTINCT FROM (CASE WHEN d.deadline_policy_ref IS NOT NULL THEN app.between_deadline_snapshot(d.id) ELSE 'null'::jsonb END) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reply source differs from its original terms or deadline';END IF;
 IF d.deadline_policy_ref IS NOT NULL THEN
 IF accepted_at<d.deadline_accounted_at THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='reply predates its acknowledged deadline boundary';END IF;
 PERFORM app.check_between_deadline_snapshot(source->'deadlineSnapshot');
 IF d.deadline_at IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='a frozen decision cannot accept a reply';END IF;
 IF accepted_at>=d.deadline_at THEN
 IF source->>'expiryReceiptId' IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='late reply requires its exact expiry selection';END IF;
 PERFORM app.check_between_expiry((source->>'expiryReceiptId')::uuid);
 IF NOT EXISTS(SELECT 1 FROM app.command_receipt WHERE id::text=source->>'expiryReceiptId' AND state='accepted' AND scope='between_deadline_expiry_v1' AND normalized_source->>'replyReceiptId'=receipt.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='late reply has no accepted reciprocal expiry';END IF;
 ELSIF source->>'expiryReceiptId' IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='early reply cannot claim expiry authority';END IF;
 ELSIF source->>'expiryReceiptId' IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='inactive deadline cannot claim expiry authority';END IF;
END $$;
CREATE FUNCTION app.guard_between_reply_source() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 SELECT * INTO receipt FROM app.command_receipt WHERE id=NEW.id;
 IF receipt.scope='between_deadline_expiry_v1' THEN PERFORM app.check_between_expiry(receipt.id);
 ELSIF receipt.scope='between_game_reply_v1' AND receipt.state='accepted' THEN
 IF NOT EXISTS(SELECT 1 FROM app.between_game_decision WHERE id::text=receipt.target_ref AND state='answered') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted reply requires its actual answered decision';END IF;
 PERFORM app.check_between_game_answer(receipt.target_ref::uuid);
 END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER between_reply_source AFTER INSERT OR UPDATE ON app.command_receipt DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_between_reply_source();
REVOKE ALL ON FUNCTION app.claim_between_game_reply(uuid,numeric,uuid,text,text,uuid),app.apply_between_game_reply(uuid,uuid,numeric,uuid,text,text,uuid),app.expire_between_game_deadline(uuid,numeric,uuid,text,text,uuid),app.check_between_expiry(uuid),app.guard_between_reply_source() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
