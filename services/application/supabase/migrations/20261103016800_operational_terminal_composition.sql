BEGIN;

CREATE OR REPLACE FUNCTION app.stamp_match_incident() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parent uuid; next_revision numeric; source uuid; match_revision numeric; phase text; current_game uuid; reserved_game uuid;
BEGIN
 IF NEW.availability='unrecoverable' THEN RETURN NULL;END IF;
 IF NEW.incident_receipt_id IS NULL THEN RETURN NULL;END IF;
 IF TG_OP='UPDATE' AND (NEW.availability,NEW.incident_receipt_id,NEW.restoration_receipt_id) IS NOT DISTINCT FROM (OLD.availability,OLD.incident_receipt_id,OLD.restoration_receipt_id) THEN RETURN NULL;END IF;
 SELECT match_id INTO STRICT parent FROM app.game WHERE id=NEW.game_id;
 UPDATE app.match SET availability=NEW.availability,availability_revision=availability_revision+1,incident_receipt_id=NEW.incident_receipt_id,restoration_receipt_id=NEW.restoration_receipt_id WHERE id=parent RETURNING availability_revision,revision,state,current_game_id,reserved_game_id INTO next_revision,match_revision,phase,current_game,reserved_game;
 source:=CASE WHEN NEW.availability='recovering' THEN NEW.incident_receipt_id ELSE NEW.restoration_receipt_id END;
 UPDATE app.command_receipt SET normalized_source=normalized_source||jsonb_build_object('matchId',parent,'matchAvailabilityRevision',next_revision::text,'matchRevision',match_revision::text,'matchPhase',phase,'currentGameId',current_game,'reservedGameId',reserved_game) WHERE id=source AND state='pending';
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match incident transition requires pending canonical source';END IF;
 RETURN NULL;
END $$;

CREATE OR REPLACE FUNCTION app.check_match_availability(p_match uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; incident app.command_receipt; restored app.command_receipt; host app.operational_state; pending_ids jsonb;
BEGIN
 SELECT * INTO m FROM app.match WHERE id=p_match;
 IF m.id IS NULL OR m.incident_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO incident FROM app.command_receipt WHERE id=m.incident_receipt_id;
 IF incident.state IS DISTINCT FROM 'accepted' OR incident.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR incident.scope NOT IN ('game_incident_v1','match_incident_v1') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match availability lacks accepted incident';END IF;
 IF m.availability='unrecoverable' THEN PERFORM app.check_operational_match_terminal(p_match);RETURN;END IF;
 IF m.availability='recovering' THEN
 IF m.restoration_receipt_id IS NOT NULL OR incident.normalized_source->>'matchAvailabilityRevision' IS DISTINCT FROM m.availability_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused availability differs from incident revision';END IF;
 ELSE
 SELECT * INTO restored FROM app.command_receipt WHERE id=m.restoration_receipt_id;
 IF restored.state IS DISTINCT FROM 'accepted' OR restored.scope NOT IN ('game_restoration_v1','match_restoration_v1') OR restored.normalized_source->>'incidentId' IS DISTINCT FROM incident.id::text OR restored.normalized_source->>'matchId' IS DISTINCT FROM m.id::text OR restored.normalized_source->>'matchAvailabilityRevision' IS DISTINCT FROM m.availability_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match resume lacks exact accepted incident disposition';END IF;
 END IF;
 IF incident.scope='game_incident_v1' THEN
 SELECT * INTO host FROM app.operational_state WHERE incident_receipt_id=incident.id;
 IF host.id IS NULL OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=host.game_id AND match_id=m.id) OR host.availability<>m.availability OR host.restoration_receipt_id IS DISTINCT FROM m.restoration_receipt_id THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match availability differs from its game incident';END IF;
 IF m.availability='recovering' AND (incident.normalized_source->>'matchRevision' IS DISTINCT FROM m.revision::text OR incident.normalized_source->>'matchPhase' IS DISTINCT FROM m.state OR incident.normalized_source->>'currentGameId' IS DISTINCT FROM m.current_game_id::text OR incident.normalized_source->>'reservedGameId' IS DISTINCT FROM m.reserved_game_id::text) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused active match frontier changed';END IF;
 ELSE
 IF incident.principal_key IS DISTINCT FROM 'match-recovery:'||m.id::text OR incident.target_ref IS DISTINCT FROM m.id::text OR incident.normalized_source->>'version' IS DISTINCT FROM 'match-incident-v1' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match-only incident source identity differs';END IF;
 IF m.availability='recovering' THEN
 SELECT coalesce(jsonb_agg(id ORDER BY id),'[]'::jsonb) INTO pending_ids FROM app.between_game_decision WHERE match_id=m.id AND state='pending';
 IF incident.response_payload->'data'->>'revision' IS DISTINCT FROM m.revision::text OR incident.normalized_source->>'phase' IS DISTINCT FROM m.state OR incident.normalized_source->>'reservedGameId' IS DISTINCT FROM m.reserved_game_id::text OR m.current_game_id IS NOT NULL OR pending_ids IS DISTINCT FROM incident.normalized_source->'pendingDecisionIds'
 OR app.match_clock_snapshot(m.id) IS DISTINCT FROM incident.normalized_source->'clockValues'
 OR EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat seat ON seat.id=c.match_seat_id WHERE seat.match_id=m.id AND NOT c.paused)
 OR EXISTS(SELECT 1 FROM app.between_game_decision WHERE match_id=m.id AND state='pending' AND deadline_at IS NOT NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='paused match-only phase, decisions or banks changed';END IF;
 ELSE IF restored.scope<>'match_restoration_v1' OR restored.principal_key IS DISTINCT FROM 'match-recovery:'||m.id::text OR restored.target_ref IS DISTINCT FROM m.id::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='match-only resume source differs';END IF;END IF;
 END IF;
END $$;

CREATE OR REPLACE FUNCTION app.check_game_recovery_source(p_game uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE host app.operational_state; source app.command_receipt; restored app.command_receipt; checkpoint app.game_checkpoint; current_game app.game;
BEGIN
 SELECT * INTO host FROM app.operational_state WHERE game_id=p_game;
 IF host.id IS NULL OR host.incident_receipt_id IS NULL THEN RETURN;END IF;
 SELECT * INTO source FROM app.command_receipt WHERE id=host.incident_receipt_id;
 SELECT * INTO checkpoint FROM app.game_checkpoint WHERE id=host.recovery_checkpoint_id;
 SELECT * INTO current_game FROM app.game WHERE id=p_game;
 IF source.state IS DISTINCT FROM 'accepted' OR source.scope IS DISTINCT FROM 'game_incident_v1' OR source.principal_key IS DISTINCT FROM 'recovery:'||p_game::text OR source.target_ref IS DISTINCT FROM p_game::text
 OR source.normalized_source->>'version' IS DISTINCT FROM 'game-incident-v1' OR source.normalized_source->>'gameId' IS DISTINCT FROM p_game::text OR source.normalized_source->>'proofRef' IS DISTINCT FROM host.incident_ref
 OR source.normalized_source->>'incidentAt' IS DISTINCT FROM to_char(host.incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"') OR source.normalized_source->>'checkpointId' IS DISTINCT FROM host.recovery_checkpoint_id::text
 OR checkpoint.game_id IS DISTINCT FROM p_game OR source.normalized_source->>'acceptedRevision' IS DISTINCT FROM checkpoint.accepted_revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='operational incident lacks exact accepted checkpoint source';END IF;
 IF host.availability='unrecoverable' THEN
 IF host.terminal_receipt_id IS NULL OR NOT EXISTS(SELECT 1 FROM app.match WHERE id=current_game.match_id AND terminal_receipt_id=host.terminal_receipt_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unrecoverable host lacks its exact terminal match';END IF;
 PERFORM app.check_operational_match_terminal(current_game.match_id);RETURN;END IF;
 IF host.availability='recovering' THEN
 IF current_game.state<>'active' OR host.restoration_receipt_id IS NOT NULL OR host.fence_epoch::text IS DISTINCT FROM source.response_payload->'data'->>'epoch' OR host.revision::text IS DISTINCT FROM source.response_payload->'data'->>'operationalRevision'
 OR current_game.accepted_revision IS DISTINCT FROM checkpoint.accepted_revision OR host.owner_instance_ref IS DISTINCT FROM source.normalized_source->>'owner'
 OR EXISTS(SELECT 1 FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id WHERE s.match_id=current_game.match_id AND (NOT c.paused OR c.last_accounted_at<>host.incident_at OR c.clock_revision::text IS DISTINCT FROM source.normalized_source->>'clockRevision'))
 OR app.match_clock_snapshot(current_game.match_id) IS DISTINCT FROM source.normalized_source->'clockValues' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recovering host or paused bank differs from incident disposition';END IF;
 ELSIF host.availability='available' THEN
 SELECT * INTO restored FROM app.command_receipt WHERE id=host.restoration_receipt_id;
 IF restored.state IS DISTINCT FROM 'accepted' OR restored.scope IS DISTINCT FROM 'game_restoration_v1' OR restored.target_ref IS DISTINCT FROM p_game::text OR restored.principal_key IS DISTINCT FROM 'recovery:'||p_game::text
 OR restored.normalized_source->>'incidentId' IS DISTINCT FROM source.id::text OR restored.normalized_source->>'checkpointId' IS DISTINCT FROM checkpoint.id::text
 OR restored.normalized_source->>'acceptedRevision' IS DISTINCT FROM checkpoint.accepted_revision::text OR restored.normalized_source->>'owner' IS DISTINCT FROM host.owner_instance_ref
 OR restored.response_payload->'data'->>'epoch' IS DISTINCT FROM host.fence_epoch::text OR restored.response_payload->'data'->>'operationalRevision' IS DISTINCT FROM host.revision::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='restored host lacks exact checkpoint/fence disposition';END IF;
 ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='unknown source-bound recovery state';END IF;
END $$;

CREATE OR REPLACE FUNCTION app.check_ante_source(p_commitment uuid) RETURNS void
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
 IF EXISTS(SELECT 1 FROM app.ante_stake s JOIN app.asset_hold h ON h.id=s.hold_id WHERE s.commitment_id=c.id AND h.state<>'released') OR EXISTS(SELECT 1 FROM app.ai_loss_exposure WHERE commitment_id=c.id AND (state<>'released' OR loss_coins IS DISTINCT FROM 0))
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt r JOIN app.ledger_operation o ON o.id=r.operation_id WHERE r.target_ref=c.id::text AND r.state='accepted' AND o.state='committed'
 AND ((r.scope='ante_cancel_v1' AND o.id=c.settlement_operation_id AND o.source_scope='ante_cancel_v1') OR (r.scope='ante_terminal_cancel_v1' AND o.id=c.settlement_operation_id AND o.source_scope='ante_cancel_v1' AND r.principal_key='ante-terminal:'||c.id::text
 AND r.normalized_source=jsonb_build_object('version','ante-terminal-cancel-v1','commitmentId',c.id,'terminalReceiptId',m.terminal_receipt_id,'basis',c.terms_revision::text)
 AND r.payload_hash=app.auth_json_hash(r.normalized_source) AND r.request_ref=m.terminal_receipt_id::text
 AND EXISTS(SELECT 1 FROM app.command_receipt terminal WHERE terminal.id=m.terminal_receipt_id AND terminal.scope='match_unrecoverable_v1' AND terminal.state='accepted' AND terminal.target_ref=m.id::text)) OR (r.scope='ante_accept_v1' AND o.id=c.reservation_operation_id AND o.source_scope='ante_commit_v1'))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='canceled ante requires released sources and its accepted cancellation'; END IF;
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
 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=c.id AND (x.state<>'reserved' OR x.loss_coins IS NOT NULL OR b.utc_day<>c.start_day_utc OR x.reserved_coins<>(SELECT coalesce(sum(s.estimated_value_coins),0) FROM app.ante_stake s WHERE s.ai_exposure_id=x.id)))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed ante cannot release or resize accepted funding'; END IF;
 IF c.state IN ('settled','refunded') AND (c.settlement_result_id IS NULL OR c.settlement IS NULL OR c.settlement_operation_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal ante requires normalized result allocation and accounting'; END IF;
 IF c.state IN ('settled','refunded') THEN PERFORM app.check_ante_settlement(c.id); END IF;
END $$;

COMMIT;
