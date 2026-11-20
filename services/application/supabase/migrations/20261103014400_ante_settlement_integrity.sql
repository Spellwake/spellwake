BEGIN;
CREATE FUNCTION app.check_ante_settlement(p_commitment uuid) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE c app.ante_commitment; r app.match_result; op app.ledger_operation; calculated jsonb; item jsonb; stake app.ante_stake; recipient app.match_seat;
 source_owner uuid; target_owner uuid; source_key text; kind text; expected_quantity numeric; asset uuid; row_count bigint; expected_count bigint:=0; exposure app.ai_loss_exposure; expected_loss numeric;
BEGIN
 SELECT * INTO STRICT c FROM app.ante_commitment WHERE id=p_commitment;
 SELECT * INTO r FROM app.match_result WHERE id=c.settlement_result_id;SELECT * INTO op FROM app.ledger_operation WHERE id=c.settlement_operation_id;
 IF r.id IS NULL OR r.match_id<>c.match_id OR r.winner_seat_ids IS NULL OR c.state IS DISTINCT FROM (CASE WHEN r.outcome='win' THEN 'settled' ELSE 'refunded' END)
 OR op.id IS NULL OR op.source_scope<>'ante_settlement_v1' OR op.source_ref<>c.id::text OR op.kind IS DISTINCT FROM (CASE WHEN r.outcome='win' THEN 'ante_settlement' ELSE 'ante_refund' END) OR op.state<>'committed'
 OR NOT EXISTS(SELECT 1 FROM app.command_receipt receipt WHERE receipt.principal_key='ante-settlement:'||c.id::text AND receipt.scope='ante_settlement_v1' AND receipt.target_ref=r.id::text AND receipt.operation_id=op.id AND receipt.state='accepted') THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal ante requires its source-bound result and accepted accounting disposition'; END IF;
 calculated:=app.build_ante_allocation(c.id,r.id,c.settlement->>'seed');
 IF c.settlement IS DISTINCT FROM calculated OR c.allocation_evidence_ref IS DISTINCT FROM 'sha256:'||app.auth_json_hash(calculated) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante allocation must reproduce the accepted practical split and random leftovers'; END IF;
 FOR stake IN SELECT * FROM app.ante_stake WHERE commitment_id=c.id AND funding_kind='owned_assets' LOOP
 IF NOT EXISTS(SELECT 1 FROM app.asset_hold WHERE id=stake.hold_id AND state=CASE WHEN r.outcome<>'win' OR stake.match_seat_id=ANY(r.winner_seat_ids) THEN 'released' ELSE 'consumed' END) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante original sources must return or be consumed according to the terminal winners'; END IF;
 END LOOP;
 FOR item IN SELECT * FROM jsonb_array_elements(calculated->'allocations') LOOP
 kind:=item->>'classification';source_key:='ante-allocation:'||app.auth_json_hash(item);expected_quantity:=(item->>'quantity')::numeric;asset:=(item->>'sourceAssetId')::uuid;
 SELECT * INTO STRICT stake FROM app.ante_stake WHERE id=(item->>'sourceStakeId')::uuid;
 SELECT * INTO STRICT recipient FROM app.match_seat WHERE id=(item->>'recipientSeatId')::uuid;
 IF kind IN ('transfer','retire') THEN
 SELECT owner_id INTO STRICT source_owner FROM app.asset_hold WHERE id=stake.hold_id;
 SELECT count(*) INTO row_count FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id WHERE p.operation_id=op.id AND p.source_ref=source_key AND p.source_kind=CASE WHEN kind='retire' THEN 'ante_retire' ELSE 'ante_transfer' END AND p.direction='debit' AND p.quantity=expected_quantity AND i.owner_id=source_owner AND i.asset_definition_id=asset;
 IF row_count<>1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante transfer or retirement lacks its exact original-asset debit'; END IF;expected_count:=expected_count+1;
 END IF;
 IF kind IN ('transfer','ai_issue') THEN
 SELECT id INTO STRICT target_owner FROM app.asset_owner WHERE account_id=recipient.account_id;
 SELECT count(*) INTO row_count FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id WHERE p.operation_id=op.id AND p.source_ref=source_key AND p.source_kind=CASE WHEN kind='ai_issue' THEN 'ai_ante_loss' ELSE 'ante_transfer' END AND p.direction='credit' AND p.quantity=expected_quantity AND i.owner_id=target_owner AND i.asset_definition_id=asset;
 IF row_count<>1 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante payout lacks its exact entitled-human credit'; END IF;expected_count:=expected_count+1;
 END IF;
 END LOOP;
 IF (SELECT count(*) FROM app.ledger_posting WHERE operation_id=op.id)<>expected_count THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante settlement contains extra or unissued-share postings'; END IF;
 FOR exposure IN SELECT * FROM app.ai_loss_exposure WHERE commitment_id=c.id LOOP
 SELECT coalesce(sum(s.estimated_value_coins),0) INTO expected_loss FROM app.ante_stake s WHERE s.ai_exposure_id=exposure.id AND r.outcome='win' AND NOT(s.match_seat_id=ANY(r.winner_seat_ids));
 IF exposure.loss_coins IS DISTINCT FROM expected_loss OR exposure.state IS DISTINCT FROM (CASE WHEN expected_loss>0 THEN 'settled' ELSE 'released' END)
 OR NOT EXISTS(SELECT 1 FROM app.ai_loss_bucket b WHERE b.id=exposure.bucket_id AND b.utc_day=c.start_day_utc AND b.lost_coins>=(SELECT coalesce(sum(x.loss_coins),0) FROM app.ai_loss_exposure x WHERE x.bucket_id=b.id AND x.state='settled')) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI settlement must charge every lost original-day coin and never replenish allowance'; END IF;
 END LOOP;
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
 OR EXISTS(SELECT 1 FROM app.ai_loss_exposure x JOIN app.ai_loss_bucket b ON b.id=x.bucket_id WHERE x.commitment_id=c.id AND (x.state<>'reserved' OR x.loss_coins IS NOT NULL OR b.utc_day<>c.start_day_utc OR x.reserved_coins<>(SELECT coalesce(sum(s.estimated_value_coins),0) FROM app.ante_stake s WHERE s.ai_exposure_id=x.id)))) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='committed ante cannot release or resize accepted funding'; END IF;
 IF c.state IN ('settled','refunded') AND (c.settlement_result_id IS NULL OR c.settlement IS NULL OR c.settlement_operation_id IS NULL) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='terminal ante requires normalized result allocation and accounting'; END IF;
 IF c.state IN ('settled','refunded') THEN PERFORM app.check_ante_settlement(c.id); END IF;
END $$;
REVOKE ALL ON FUNCTION app.check_ante_settlement(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
