BEGIN;
CREATE FUNCTION app.check_ante_source(p_commitment uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; m app.match; snapshot jsonb; expected_ids uuid[]; stake app.ante_stake; terms_value jsonb;
BEGIN
 SELECT * INTO c FROM app.ante_commitment WHERE id=p_commitment;
 IF c.id IS NULL OR c.terms IS NULL THEN RETURN; END IF;
 SELECT * INTO STRICT m FROM app.match WHERE id=c.match_id FOR NO KEY UPDATE;
 terms_value:=app.ante_terms_manifest(c.id);
 IF c.terms IS DISTINCT FROM terms_value OR c.terms_hash IS DISTINCT FROM app.auth_json_hash(terms_value) OR m.mode<>'collection'
 OR array_position(c.accepted_seat_ids,NULL) IS NOT NULL OR array_ndims(c.accepted_seat_ids)>1
 OR cardinality(c.accepted_seat_ids)<>(SELECT count(DISTINCT id) FROM unnest(c.accepted_seat_ids) id)
 OR EXISTS(SELECT 1 FROM unnest(c.accepted_seat_ids) accepted WHERE NOT EXISTS(SELECT 1 FROM app.match_seat WHERE id=accepted AND match_id=m.id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante terms and acceptances must match their immutable seat/funding sources'; END IF;
 FOR stake IN SELECT * FROM app.ante_stake WHERE commitment_id=c.id LOOP
 IF stake.value_numerator IS NULL OR stake.value_denominator IS NULL OR stake.bundle_assessment IS NULL
 OR stake.bundle_assessment->'total'->>'numerator' IS DISTINCT FROM stake.value_numerator::text OR stake.bundle_assessment->'total'->>'denominator' IS DISTINCT FROM stake.value_denominator::text
 OR stake.estimated_value_coins<>div(stake.value_numerator+stake.value_denominator-1,stake.value_denominator)
 OR (stake.funding_kind='owned_assets' AND NOT EXISTS(SELECT 1 FROM app.asset_hold WHERE id=stake.hold_id AND ante_commitment_id=c.id))
 OR (stake.funding_kind='ai_loss_allowance' AND (stake.value_denominator<>1 OR stake.value_numerator<>stake.estimated_value_coins)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante exact valuation must match its typed funding assessment'; END IF;
 END LOOP;
 IF EXISTS(SELECT 1 FROM app.asset_hold h WHERE h.ante_commitment_id=c.id AND h.state='held' AND NOT EXISTS(SELECT 1 FROM app.ante_stake s WHERE s.commitment_id=c.id AND s.hold_id=h.id))
 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure x WHERE x.commitment_id=c.id AND x.state='reserved' AND x.reserved_coins<>(SELECT coalesce(sum(s.estimated_value_coins),0) FROM app.ante_stake s WHERE s.ai_exposure_id=x.id)) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante funding cannot contain unassigned held assets or exposure'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(c.accepted_seat_ids) accepted WHERE NOT EXISTS(SELECT 1 FROM app.command_receipt r WHERE r.principal_key='ante-seat:'||accepted::text AND r.scope='ante_accept_v1' AND r.target_ref=c.id::text AND r.state='accepted' AND r.payload_hash=app.auth_json_hash(jsonb_build_object('commitment',c.id,'seat',accepted,'basis',c.terms_revision::text,'termsHash',c.terms_hash)))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='every ante acceptance requires its own current terms receipt'; END IF;
 IF c.state='offered' THEN
 IF c.accepted_at IS NOT NULL OR EXISTS(SELECT 1 FROM app.ante_stake s JOIN app.asset_hold h ON h.id=s.hold_id WHERE s.commitment_id=c.id AND h.state<>'held') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='offered ante must preserve its available held bundle'; END IF;
 RETURN;
 END IF;
 IF c.state='canceled' THEN
 IF EXISTS(SELECT 1 FROM app.ante_stake s JOIN app.asset_hold h ON h.id=s.hold_id WHERE s.commitment_id=c.id AND h.state<>'released') OR EXISTS(SELECT 1 FROM app.ai_loss_exposure WHERE commitment_id=c.id AND state<>'released')
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt r JOIN app.ledger_operation o ON o.id=r.operation_id WHERE r.target_ref=c.id::text AND r.state='accepted' AND o.state='committed'
 AND ((r.scope='ante_cancel_v1' AND o.id=c.settlement_operation_id AND o.source_scope='ante_cancel_v1') OR (r.scope='ante_accept_v1' AND o.id=c.reservation_operation_id AND o.source_scope='ante_commit_v1'))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canceled ante requires released sources and its accepted cancellation'; END IF;
 RETURN;
 END IF;
 SELECT array_agg(id ORDER BY id) INTO expected_ids FROM app.match_seat WHERE match_id=m.id;
 IF c.accepted_at IS NULL OR c.start_day_utc IS DISTINCT FROM to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD')
 OR (SELECT array_agg(id ORDER BY id) FROM unnest(c.accepted_seat_ids) id) IS DISTINCT FROM expected_ids
 OR (SELECT count(*) FROM app.ante_stake WHERE commitment_id=c.id)<2
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt r JOIN app.ledger_operation o ON o.id=r.operation_id WHERE r.scope='ante_accept_v1' AND r.target_ref=c.id::text AND r.state='accepted' AND o.id=c.reservation_operation_id AND o.state='committed' AND o.kind='ante_commit' AND o.source_scope='ante_commit_v1' AND o.source_ref=c.id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed ante requires complete consent and its accepted start-day reservation'; END IF;
 SELECT jsonb_build_object('version','ante-valuations-v1','stakes',coalesce(jsonb_agg(jsonb_build_object('seatId',s.match_seat_id,'assessment',s.bundle_assessment) ORDER BY s.match_seat_id),'[]'::jsonb)) INTO snapshot FROM app.ante_stake s WHERE commitment_id=c.id;
 IF snapshot IS DISTINCT FROM c.valuation_snapshot OR c.valuation_evidence_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(snapshot) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted ante valuation snapshot differs from exact stake evidence'; END IF;
 IF c.state='committed' AND (EXISTS(SELECT 1 FROM app.ante_stake s JOIN app.asset_hold h ON h.id=s.hold_id WHERE s.commitment_id=c.id AND h.state<>'held')
 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=c.id AND (x.state<>'reserved' OR b.utc_day<>c.start_day_utc OR x.reserved_coins<>(SELECT coalesce(sum(s.estimated_value_coins),0) FROM app.ante_stake s WHERE s.ai_exposure_id=x.id)))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed ante cannot release or resize accepted funding'; END IF;
 IF c.state IN ('settled','refunded') AND (c.settlement_result_id IS NULL OR c.settlement IS NULL OR c.settlement_operation_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal ante requires normalized result allocation and accounting'; END IF;
END $$;
CREATE FUNCTION app.check_ante_source_trigger() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE target uuid;
BEGIN
 IF TG_TABLE_NAME='ante_commitment' THEN target:=NEW.id;
 ELSIF TG_TABLE_NAME='ante_stake' THEN IF TG_OP='DELETE' THEN target:=OLD.commitment_id; ELSE target:=NEW.commitment_id; END IF;
 ELSIF TG_TABLE_NAME='asset_hold' THEN target:=NEW.ante_commitment_id;
 ELSE target:=NEW.commitment_id; END IF;
 IF target IS NOT NULL THEN PERFORM app.check_ante_source(target); END IF;RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ante_terms_source AFTER INSERT OR UPDATE ON app.ante_commitment DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_ante_source_trigger();
CREATE CONSTRAINT TRIGGER ante_stake_source AFTER INSERT OR UPDATE OR DELETE ON app.ante_stake DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_ante_source_trigger();
CREATE CONSTRAINT TRIGGER hold_ante_source AFTER INSERT OR UPDATE ON app.asset_hold DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_ante_source_trigger();
CREATE CONSTRAINT TRIGGER ante_exposure_source AFTER INSERT OR UPDATE ON app.ai_loss_exposure DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_ante_source_trigger();
REVOKE ALL ON FUNCTION app.check_ante_source(uuid),app.check_ante_source_trigger() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.accept_reserved_game(p_match uuid,p_expected numeric,p_game uuid,p_owner text,p_opening jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; g app.game; receipt app.command_receipt; fingerprint text; context_id uuid; at_time timestamptz; next_clock numeric; ante_source app.ante_commitment;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening acceptance requires read committed retry'; END IF;
 IF p_match IS NULL OR p_game IS NULL OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR jsonb_typeof(p_opening) IS DISTINCT FROM 'object' OR octet_length(p_opening::text)>16384
 OR EXISTS(SELECT 1 FROM unnest(ARRAY['snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef']) k WHERE jsonb_typeof(p_opening->k) IS DISTINCT FROM 'string' OR length(btrim(p_opening->>k)) NOT BETWEEN 1 AND 2048)
 OR EXISTS(SELECT 1 FROM jsonb_object_keys(p_opening) k WHERE k NOT IN ('snapshotRef','contextRevision','stateRef','factsRef','eventRef','qualificationRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid normalized opening artifacts'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',p_match,'game',p_game,'owner',p_owner,'opening',p_opening));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('match-opening:'||p_match::text,'game_opening_v1',p_game::text,p_game::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='match-opening:'||p_match::text AND scope='game_opening_v1' AND request_ref=p_game::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening source was reused with different artifacts'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='opening pending'; END IF;
 RETURN receipt.response_payload;
 END IF;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT * INTO g FROM app.game WHERE id=p_game FOR NO KEY UPDATE;
 IF m.id IS NULL OR g.id IS NULL OR m.revision<>p_expected OR m.reserved_game_id IS DISTINCT FROM p_game OR m.current_game_id IS NOT NULL OR g.match_id<>m.id OR g.state<>'reserved' OR m.state<>'ready' OR m.accepted_release_id IS NULL THEN
 RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='stale or unavailable game reservation; retry from current basis'; END IF;
 -- Optional ante must have its accepted commitment before an opening exists.
 IF EXISTS(SELECT 1 FROM app.ante_commitment WHERE match_id=m.id AND state NOT IN ('committed','canceled')) THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante source is not committed; opening remains reserved'; END IF;
 FOR ante_source IN SELECT * FROM app.ante_commitment WHERE match_id=m.id LOOP
 IF ante_source.terms IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='game opening requires source-qualified ante terms'; END IF;
 PERFORM app.check_ante_source(ante_source.id);
 END LOOP;
 PERFORM c.id FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
 SELECT coalesce(max(c.clock_revision),0)+1 INTO next_clock FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id;
 INSERT INTO app.game_context(game_id,release_id,profile_id,snapshot_ref,context_revision)
 VALUES(g.id,m.accepted_release_id,m.profile_id,p_opening->>'snapshotRef',p_opening->>'contextRevision') RETURNING id INTO context_id;
 INSERT INTO app.game_checkpoint(game_id,context_id,accepted_revision,private_state_ref,facts_ref,clock_revision)
 VALUES(g.id,context_id,0,p_opening->>'stateRef',p_opening->>'factsRef',next_clock);
 INSERT INTO app.game_event(game_id,context_id,sequence,accepted_revision,private_payload_ref) VALUES(g.id,context_id,0,0,p_opening->>'eventRef');
 UPDATE app.game SET state='active' WHERE id=g.id;
 UPDATE app.match SET state='active',reserved_game_id=NULL,current_game_id=g.id,revision=revision+1 WHERE id=m.id;
 INSERT INTO app.operational_state(game_id,owner_instance_ref,fence_epoch,availability,revision) VALUES(g.id,p_owner,1,'available',0);
 at_time:=clock_timestamp();
 UPDATE app.match_clock c SET paused=true,last_accounted_at=at_time,clock_revision=next_clock FROM app.match_seat s WHERE s.id=c.match_seat_id AND s.match_id=m.id;
 UPDATE app.command_receipt SET normalized_source=jsonb_build_object('version','game-opening-v1')||p_opening WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',g.id,'contextId',context_id,'revision',(m.revision+1)::text));
END $$;
COMMIT;
