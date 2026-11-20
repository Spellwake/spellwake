BEGIN;
-- The receipt owns the complete intent. Each row is one owned sealed-deck alternative.
CREATE FUNCTION app.enqueue_matchmaking_intent(p_actor uuid,p_mode text,p_competitive boolean,p_alternatives jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; alternative jsonb; choice jsonb; revision_id uuid; library app.deck;
 profile app.table_profile_revision; configuration jsonb; normalized jsonb:='[]'; choices jsonb; ordinal bigint:=0; entry_id uuid; first_id uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='queue intake requires read committed retry'; END IF;
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid queue request'; END IF;
 IF octet_length(jsonb_build_object('mode',p_mode,'competitive',p_competitive,'alternatives',p_alternatives)::text)>262144 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='queue intent exceeds command envelope'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'queue_intent_v1',p_actor::text,jsonb_build_object('mode',p_mode,'competitive',p_competitive,'alternatives',p_alternatives));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 IF p_mode IS NULL OR p_mode NOT IN ('open','collection') OR p_competitive IS NULL OR (p_competitive AND p_mode<>'collection') OR jsonb_typeof(p_alternatives) IS DISTINCT FROM 'array' OR octet_length(p_alternatives::text)>262144 THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('intent','invalid envelope')); END IF;
 IF jsonb_array_length(p_alternatives) NOT BETWEEN 1 AND 32 OR (SELECT count(DISTINCT x->>'deckRevisionId') FROM jsonb_array_elements(p_alternatives) x)<>jsonb_array_length(p_alternatives) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('alternatives','distinct bounded decks required')); END IF;
 FOR alternative IN SELECT * FROM jsonb_array_elements(p_alternatives) LOOP
 IF jsonb_typeof(alternative) IS DISTINCT FROM 'object' OR jsonb_typeof(alternative->'deckRevisionId') IS DISTINCT FROM 'string' OR alternative->>'deckRevisionId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
 OR jsonb_typeof(alternative->'choices') IS DISTINCT FROM 'array' OR EXISTS(SELECT 1 FROM jsonb_object_keys(alternative) k WHERE k NOT IN ('deckRevisionId','choices')) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('alternative','invalid shape')); END IF;
 IF jsonb_array_length(alternative->'choices') NOT BETWEEN 1 AND 128 OR (SELECT count(DISTINCT (x->>'profileId',x->>'configurationKey')) FROM jsonb_array_elements(alternative->'choices') x)<>jsonb_array_length(alternative->'choices') THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('choices','distinct bounded configurations required')); END IF;
 revision_id:=(alternative->>'deckRevisionId')::uuid;
 SELECT d.* INTO library FROM app.deck_revision r JOIN app.deck d ON d.id=r.deck_id WHERE r.id=revision_id AND r.sealed_at IS NOT NULL AND d.source_kind='account' AND d.account_id=p_actor AND d.mode=p_mode FOR SHARE OF r,d;
 IF library.id IS NULL THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('deck','owned sealed revision required')); END IF;
 choices:='[]';
 FOR choice IN SELECT * FROM jsonb_array_elements(alternative->'choices') LOOP
 IF jsonb_typeof(choice) IS DISTINCT FROM 'object' OR EXISTS(SELECT 1 FROM unnest(ARRAY['profileId','configurationKey','maximumAiSeats','maximumSponsoredAiSeats']) k WHERE jsonb_typeof(choice->k) IS DISTINCT FROM 'string')
 OR choice->>'profileId' !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' OR length(btrim(choice->>'configurationKey')) NOT BETWEEN 1 AND 128
 OR choice->>'maximumAiSeats' !~ '^[0-5]$' OR choice->>'maximumSponsoredAiSeats' !~ '^[0-5]$'
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(choice) k WHERE k NOT IN ('profileId','configurationKey','maximumAiSeats','maximumSponsoredAiSeats')) THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('choice','invalid shape')); END IF;
 SELECT * INTO profile FROM app.table_profile_revision WHERE id=(choice->>'profileId')::uuid FOR SHARE;
 SELECT x INTO configuration FROM jsonb_array_elements(profile.matchmaking_config->'configurations') x WHERE x->>'key'=choice->>'configurationKey';
 IF configuration IS NULL OR configuration->>'formatId' IS DISTINCT FROM library.format_id::text OR (choice->>'maximumSponsoredAiSeats')::integer>(choice->>'maximumAiSeats')::integer
 OR (choice->>'maximumAiSeats')::integer>=(configuration->>'seatCount')::integer OR (p_competitive AND (choice->>'maximumAiSeats')<>'0') THEN
 RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('choice','unsupported format, profile or AI allowance')); END IF;
 choices:=choices||jsonb_build_array(choice||jsonb_build_object('configuration',configuration,'influence',profile.matchmaking_config->'influence','planechase',profile.matchmaking_config->'planechase','qualificationRef',profile.matchmaking_config->'qualificationRef'));
 END LOOP;
 normalized:=normalized||jsonb_build_array(jsonb_build_object('deckRevisionId',revision_id,'formatId',library.format_id,'choices',choices));
 END LOOP;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','queue-intent-v1','mode',p_mode,'competitive',p_competitive,'alternatives',normalized) WHERE id=receipt.id;
 FOR alternative IN SELECT * FROM jsonb_array_elements(normalized) LOOP
 INSERT INTO app.queue_entry(account_id,deck_revision_id,mode,competitive,preferences_ref,state,intent_receipt_id,alternative_index,format_id,preferences)
 VALUES(p_actor,(alternative->>'deckRevisionId')::uuid,p_mode,p_competitive,'sha256:'||receipt.payload_hash,'queued',receipt.id,ordinal,(alternative->>'formatId')::uuid,jsonb_build_object('version','queue-alternative-v1','choices',alternative->'choices')) RETURNING id INTO entry_id;
 first_id:=coalesce(first_id,entry_id);ordinal:=ordinal+1;
 END LOOP;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',first_id,'intentId',receipt.id));
END $$;
-- Typed rows must keep their actual receipt projection, not just an opaque source label.
CREATE FUNCTION app.check_queue_intent(p_entry uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE entry app.queue_entry; receipt app.command_receipt; alternative jsonb;
BEGIN
 SELECT * INTO entry FROM app.queue_entry WHERE id=p_entry;
 IF entry.id IS NULL OR entry.intent_receipt_id IS NULL THEN RETURN; END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=entry.intent_receipt_id;
 alternative:=receipt.normalized_source->'alternatives'->entry.alternative_index::integer;
 IF receipt.state<>'accepted' OR receipt.scope<>'queue_intent_v1' OR receipt.principal_key<>'human:'||entry.account_id::text OR receipt.normalized_source->>'version'<>'queue-intent-v1'
 OR receipt.normalized_source->>'mode' IS DISTINCT FROM entry.mode OR receipt.normalized_source->'competitive' IS DISTINCT FROM to_jsonb(entry.competitive)
 OR alternative IS NULL OR alternative->>'deckRevisionId' IS DISTINCT FROM entry.deck_revision_id::text OR alternative->>'formatId' IS DISTINCT FROM entry.format_id::text
 OR entry.preferences IS DISTINCT FROM jsonb_build_object('version','queue-alternative-v1','choices',alternative->'choices') OR entry.preferences_ref IS DISTINCT FROM 'sha256:'||receipt.payload_hash
 OR (SELECT count(*) FROM app.queue_entry WHERE intent_receipt_id=receipt.id)<>jsonb_array_length(receipt.normalized_source->'alternatives') THEN
 RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='queue alternative differs from accepted whole intent'; END IF;
END $$;
CREATE FUNCTION app.guard_queue_intent() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN PERFORM app.check_queue_intent(NEW.id);RETURN NULL;END $$;
CREATE CONSTRAINT TRIGGER queue_intent_source AFTER INSERT OR UPDATE ON app.queue_entry DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.guard_queue_intent();
REVOKE ALL ON FUNCTION app.enqueue_matchmaking_intent(uuid,text,boolean,jsonb,uuid),app.check_queue_intent(uuid),app.guard_queue_intent() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
