BEGIN;
CREATE FUNCTION app.consume_quest_game_activity(p_game uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE source_game app.game; source_match app.match; cursor_row app.activity_consumer_cursor; event_row app.game_event;
 fact app.game_activity_fact; assignment app.quest_assignment; definition app.quest_definition_revision; actors uuid[];
 progress_value numeric; owner uuid; coin uuid; inventory uuid; operation uuid; reward_receipt app.command_receipt; reward_hash text;
 processed integer:=0; rewarded integer:=0; next_sequence numeric;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest consumption requires read committed retry'; END IF;
 IF p_game IS NULL OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 1000 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='bounded game activity batch required'; END IF;
 SELECT * INTO source_game FROM app.game WHERE id=p_game;
 IF source_game.id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23503',MESSAGE='game activity source missing'; END IF;
 SELECT array_agg(account_id ORDER BY account_id) INTO actors FROM app.match_seat WHERE match_id=source_game.match_id AND controller_kind='human';
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 INSERT INTO app.activity_consumer_cursor(consumer_key,game_id,last_sequence,revision,initialized) VALUES('quests-v1',p_game,0,0,false) ON CONFLICT(consumer_key,game_id) DO NOTHING;
 SELECT * INTO STRICT cursor_row FROM app.activity_consumer_cursor WHERE consumer_key='quests-v1' AND game_id=p_game FOR NO KEY UPDATE;
 SELECT * INTO STRICT source_game FROM app.game WHERE id=p_game FOR SHARE;
 SELECT * INTO STRICT source_match FROM app.match WHERE id=source_game.match_id FOR SHARE;
 IF EXISTS(SELECT 1 FROM app.match_seat WHERE match_id=source_match.id AND controller_kind='human' AND NOT(account_id=ANY(coalesce(actors,'{}'::uuid[])))) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='quest participant fence changed'; END IF;
 FOR event_row IN SELECT * FROM app.game_event WHERE game_id=p_game AND (NOT cursor_row.initialized OR sequence>cursor_row.last_sequence) ORDER BY sequence LIMIT p_limit LOOP
 IF source_match.mode='collection' THEN
 FOR fact IN SELECT * FROM app.game_activity_fact WHERE source_event_id=event_row.id ORDER BY account_id,kind,fact_revision,id LOOP
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=fact.account_id AND state IN ('active','deletion_pending')) THEN CONTINUE; END IF;
 FOR assignment IN SELECT q.* FROM app.quest_assignment q JOIN app.quest_definition_revision d ON d.id=q.definition_id WHERE q.account_id=fact.account_id AND q.completed_at IS NULL AND q.replaced_by_id IS NULL AND q.assigned_at<=event_row.accepted_at AND d.fact_kind=fact.kind AND d.fact_schema_revision=fact.fact_revision ORDER BY q.id FOR NO KEY UPDATE OF q LOOP
 SELECT * INTO STRICT definition FROM app.quest_definition_revision WHERE id=assignment.definition_id;
 progress_value:=least(definition.target,assignment.progress+fact.value);
 IF progress_value=assignment.progress THEN CONTINUE; END IF;
 IF progress_value=definition.target THEN
 reward_hash:=app.auth_json_hash(jsonb_build_object('assignment',assignment.id,'definition',definition.id,'fact',fact.id));
 INSERT INTO app.command_receipt(account_id,principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES(fact.account_id,'quests:'||fact.account_id::text,'quest_reward_v1',assignment.id::text,assignment.id::text,reward_hash,'pending','') RETURNING * INTO reward_receipt;
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=fact.account_id AND state='active';
 PERFORM app.lock_asset_account_for_write(owner);
 SELECT id INTO STRICT coin FROM app.asset_definition WHERE currency_key='coin';inventory:=app.ensure_inventory(owner,coin);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES('quest_reward',assignment.id::text,'quest_reward_v1',fact.account_id,clock_timestamp(),'pending') RETURNING id INTO operation;
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) VALUES(operation,inventory,'credit',definition.reward_coins,'quest_reward_v1',assignment.id::text);
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 UPDATE app.quest_assignment SET progress=progress_value,completed_at=event_row.accepted_at,reward_source_fact_id=fact.id,reward_operation_id=operation WHERE id=assignment.id;
 UPDATE app.command_receipt SET operation_id=operation WHERE id=reward_receipt.id;
 PERFORM app.finish_account_command(reward_receipt.id,true,jsonb_build_object('resourceId',assignment.id,'factId',fact.id,'coins',definition.reward_coins::text));
 rewarded:=rewarded+1;
 ELSE UPDATE app.quest_assignment SET progress=progress_value WHERE id=assignment.id;
 END IF;
 END LOOP;
 END LOOP;
 END IF;
 processed:=processed+1;next_sequence:=event_row.sequence;
 END LOOP;
 IF processed>0 THEN UPDATE app.activity_consumer_cursor SET initialized=true,last_sequence=next_sequence,revision=revision+1 WHERE id=cursor_row.id; END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('resourceId',cursor_row.id,'events',processed::text,'rewards',rewarded::text));
END $$;
REVOKE ALL ON FUNCTION app.consume_quest_game_activity(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
