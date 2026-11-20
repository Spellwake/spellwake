BEGIN;
ALTER TABLE app.asset_hold ADD COLUMN ante_commitment_id uuid REFERENCES app.ante_commitment(id) ON DELETE RESTRICT;
ALTER TABLE app.asset_hold ADD CONSTRAINT ante_hold_parent_purpose CHECK(ante_commitment_id IS NULL OR purpose='ante');
CREATE FUNCTION app.guard_hold_ante_parent() RETURNS trigger LANGUAGE plpgsql SET search_path='' AS $$
BEGIN
 IF NEW.ante_commitment_id IS DISTINCT FROM OLD.ante_commitment_id THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante hold source parent is immutable'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER hold_ante_parent_history BEFORE UPDATE ON app.asset_hold FOR EACH ROW EXECUTE FUNCTION app.guard_hold_ante_parent();
REVOKE ALL ON FUNCTION app.guard_hold_ante_parent() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
CREATE OR REPLACE FUNCTION app.check_ante_funding_scope(p_matches uuid[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante funding changes require read committed retry'; END IF;
 PERFORM id FROM app.match WHERE id=ANY(p_matches) ORDER BY id FOR NO KEY UPDATE;
 IF EXISTS(
  SELECT 1 FROM app.ante_stake s
  JOIN app.ante_commitment c ON c.id=s.commitment_id
  JOIN app.match_seat seat ON seat.id=s.match_seat_id
  JOIN app.match m ON m.id=c.match_id
  LEFT JOIN app.asset_hold h ON h.id=s.hold_id
  LEFT JOIN app.asset_owner owner ON owner.id=h.owner_id
  LEFT JOIN app.ai_loss_exposure x ON x.id=s.ai_exposure_id
  LEFT JOIN app.ai_loss_bucket b ON b.id=x.bucket_id
  WHERE (c.match_id=ANY(p_matches) OR seat.match_id=ANY(p_matches)) AND
   (seat.match_id<>c.match_id OR m.mode<>'collection' OR s.estimated_value_coins<=0
    OR (s.funding_kind='owned_assets' AND
      (seat.controller_kind<>'human' OR h.id IS NULL OR owner.account_id IS DISTINCT FROM seat.account_id
       OR h.purpose<>'ante' OR (h.ante_commitment_id IS NULL AND h.commitment_ref<>c.id::text) OR (h.ante_commitment_id IS NOT NULL AND h.ante_commitment_id<>c.id)))
    OR (s.funding_kind='ai_loss_allowance' AND
      (seat.controller_kind<>'ai' OR x.id IS NULL OR x.commitment_id<>c.id
       OR x.match_id<>c.match_id OR b.sponsor_id IS DISTINCT FROM seat.sponsor_id)))
 ) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante stake must bind its match seat and funding source'; END IF;
 IF EXISTS(
  SELECT 1 FROM app.ante_stake s JOIN app.ai_loss_exposure x ON x.id=s.ai_exposure_id
  WHERE x.match_id=ANY(p_matches)
  GROUP BY x.id,x.reserved_coins HAVING sum(s.estimated_value_coins)>x.reserved_coins
 ) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI seat stakes exceed their shared reserved exposure'; END IF;
END $$;
-- Trusted normalized seat operation. Runtime actor/controller authorization is
-- a separate whole-RLS boundary; no role receives this capability here.
CREATE FUNCTION app.set_ante_stake(p_match uuid,p_seat uuid,p_expected numeric,p_lines jsonb,p_ai_coins numeric,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE m app.match; seat app.match_seat; c app.ante_commitment; previous app.ante_stake; receipt app.command_receipt;
 actors uuid[]; actor uuid; policy uuid; owner uuid; hold uuid; operation uuid; assets uuid[]; asset uuid; assessment jsonb; exact_total app.exact_value;
 bucket uuid; exposure uuid; reserved numeric; day_key text; terms_value jsonb; fingerprint text; line jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante offer requires read committed retry'; END IF;
 IF p_match IS NULL OR p_seat IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$' THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid ante offer identity'; END IF;
 assets:=app.bundle_asset_ids(p_lines);
 IF p_ai_coins IS NOT NULL AND (p_ai_coins::text !~ '^(0|[1-9][0-9]{0,2})$' OR p_ai_coins>300) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='AI nominal stake outside match allowance'; END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',p_match,'seat',p_seat,'basis',p_expected::text,'assets',p_lines,'aiCoins',p_ai_coins::text));
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref)
 VALUES('ante-seat:'||p_seat::text,'ante_offer_v1',p_match::text,p_request::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='ante-seat:'||p_seat::text AND scope='ante_offer_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_match::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante offer source terms changed'; END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='ante offer pending'; END IF;RETURN receipt.response_payload;
 END IF;
 SELECT array_agg(DISTINCT id ORDER BY id) INTO actors FROM(SELECT account_id id FROM app.match_seat WHERE match_id=p_match AND account_id IS NOT NULL UNION SELECT sponsor_id FROM app.match_seat WHERE match_id=p_match AND sponsor_id IS NOT NULL) participants;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO m FROM app.match WHERE id=p_match FOR NO KEY UPDATE;
 SELECT * INTO seat FROM app.match_seat WHERE id=p_seat AND match_id=p_match FOR SHARE;
 IF m.id IS NULL OR seat.id IS NULL OR m.mode<>'collection' OR m.state<>'ready' OR m.accepted_release_id IS NULL OR m.started_at IS NULL
 OR NOT EXISTS(SELECT 1 FROM app.game WHERE id=m.reserved_game_id AND match_id=m.id AND game_index=0 AND state='reserved')
 OR EXISTS(SELECT 1 FROM app.game WHERE match_id=m.id AND (game_index<>0 OR state<>'reserved'))
 OR EXISTS(SELECT 1 FROM unnest(actors) a WHERE NOT EXISTS(SELECT 1 FROM app.account WHERE id=a AND state='active')) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('match','unavailable initial ante window')); END IF;
 IF (seat.controller_kind='human' AND p_ai_coins IS NOT NULL) OR (seat.controller_kind='ai' AND (p_ai_coins IS NULL OR cardinality(assets)<>0)) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='stake funding does not match seat controller'; END IF;
 INSERT INTO app.ante_commitment(match_id,terms_revision,state) VALUES(m.id,0,'offered') ON CONFLICT(match_id) DO NOTHING;
 SELECT * INTO STRICT c FROM app.ante_commitment WHERE match_id=m.id FOR NO KEY UPDATE;
 IF c.state<>'offered' OR c.terms_revision<>p_expected THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('terms','stale or no longer offered')); END IF;
 SELECT * INTO previous FROM app.ante_stake WHERE commitment_id=c.id AND match_seat_id=seat.id FOR NO KEY UPDATE;
 policy:=app.lock_valuation_catalogue(false);
 actor:=coalesce(seat.account_id,seat.sponsor_id);
 INSERT INTO app.ledger_operation(kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES('ante_offer',receipt.id::text,'ante_offer_v1',actor,clock_timestamp(),'pending') RETURNING id INTO operation;
 IF seat.controller_kind='human' THEN
 SELECT id INTO STRICT owner FROM app.asset_owner WHERE account_id=seat.account_id AND state='active';PERFORM app.lock_asset_account_for_write(owner);
 PERFORM app.lock_assessment_products(assets);
 FOR asset IN SELECT unnest(assets) ORDER BY 1 LOOP PERFORM app.ensure_inventory(owner,asset); END LOOP;
 PERFORM id FROM app.inventory_asset WHERE owner_id=owner AND asset_definition_id=ANY(assets) ORDER BY id FOR NO KEY UPDATE;
 IF NOT app.bundle_available(owner,p_lines,previous.hold_id) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='ante bundle is unavailable or held elsewhere'; END IF;
 IF previous.hold_id IS NOT NULL THEN UPDATE app.asset_hold SET state='released' WHERE id=previous.hold_id; END IF;
 IF cardinality(assets)=0 THEN DELETE FROM app.ante_stake WHERE id=previous.id;
 ELSE
 assessment:=app.assess_bundle(p_lines);exact_total:=app.value_make((assessment->'total'->>'numerator')::numeric,(assessment->'total'->>'denominator')::numeric);
 INSERT INTO app.asset_hold(owner_id,purpose,commitment_ref,state,operation_id,ante_commitment_id) VALUES(owner,'ante','ante-offer:'||receipt.id::text,'held',operation,c.id) RETURNING id INTO hold;
 FOR line IN SELECT value FROM jsonb_array_elements(p_lines) LOOP
 INSERT INTO app.hold_line(hold_id,inventory_asset_id,quantity) SELECT hold,id,(line->>'quantity')::numeric FROM app.inventory_asset WHERE owner_id=owner AND asset_definition_id=(line->>'assetId')::uuid;
 END LOOP;
 INSERT INTO app.ante_stake(commitment_id,match_seat_id,funding_kind,hold_id,estimated_value_coins,valuation_policy_id,value_numerator,value_denominator,bundle_assessment)
 VALUES(c.id,seat.id,'owned_assets',hold,div(exact_total.numerator+exact_total.denominator-1,exact_total.denominator),policy,exact_total.numerator,exact_total.denominator,assessment)
 ON CONFLICT(commitment_id,match_seat_id) DO UPDATE SET hold_id=EXCLUDED.hold_id,estimated_value_coins=EXCLUDED.estimated_value_coins,valuation_policy_id=policy,value_numerator=EXCLUDED.value_numerator,value_denominator=EXCLUDED.value_denominator,bundle_assessment=EXCLUDED.bundle_assessment;
 END IF;
 ELSE
 day_key:=to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD');
 INSERT INTO app.ai_loss_bucket(sponsor_id,utc_day,limit_coins,lost_coins,revision) VALUES(seat.sponsor_id,day_key,1000,0,0) ON CONFLICT(sponsor_id,utc_day) DO NOTHING;
 SELECT id INTO STRICT bucket FROM app.ai_loss_bucket WHERE sponsor_id=seat.sponsor_id AND utc_day=day_key FOR NO KEY UPDATE;
 SELECT id INTO exposure FROM app.ai_loss_exposure WHERE bucket_id=bucket AND match_id=m.id AND state='reserved' FOR NO KEY UPDATE;
 SELECT coalesce(sum(estimated_value_coins),0)+p_ai_coins INTO reserved FROM app.ante_stake s JOIN app.match_seat other ON other.id=s.match_seat_id WHERE s.commitment_id=c.id AND s.funding_kind='ai_loss_allowance' AND other.sponsor_id=seat.sponsor_id AND other.id<>seat.id;
 IF exposure IS NULL THEN
 INSERT INTO app.ai_loss_exposure(bucket_id,match_id,commitment_id,reserved_coins,state) VALUES(bucket,m.id,c.id,reserved,'reserved') RETURNING id INTO exposure;
 ELSE UPDATE app.ai_loss_exposure SET reserved_coins=greatest(reserved_coins,reserved) WHERE id=exposure; END IF;
 IF p_ai_coins=0 THEN DELETE FROM app.ante_stake WHERE id=previous.id;
 ELSE
 SELECT id INTO STRICT asset FROM app.asset_definition WHERE currency_key='coin';assessment:=app.assess_bundle(jsonb_build_array(jsonb_build_object('assetId',asset,'quantity',p_ai_coins::text)));
 INSERT INTO app.ante_stake(commitment_id,match_seat_id,funding_kind,ai_exposure_id,estimated_value_coins,valuation_policy_id,value_numerator,value_denominator,bundle_assessment)
 VALUES(c.id,seat.id,'ai_loss_allowance',exposure,p_ai_coins,policy,p_ai_coins,1,assessment)
 ON CONFLICT(commitment_id,match_seat_id) DO UPDATE SET ai_exposure_id=EXCLUDED.ai_exposure_id,estimated_value_coins=EXCLUDED.estimated_value_coins,valuation_policy_id=policy,value_numerator=EXCLUDED.value_numerator,value_denominator=1,bundle_assessment=EXCLUDED.bundle_assessment;
 END IF;
 UPDATE app.ai_loss_exposure SET reserved_coins=reserved WHERE id=exposure;
 END IF;
 UPDATE app.ante_commitment SET terms_revision=terms_revision+1,accepted_seat_ids='{}',accepted_at=NULL,valuation_snapshot=NULL,valuation_evidence_ref=NULL WHERE id=c.id;
 terms_value:=app.ante_terms_manifest(c.id);
 UPDATE app.ante_commitment SET terms=terms_value,terms_hash=app.auth_json_hash(terms_value) WHERE id=c.id;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;UPDATE app.command_receipt SET operation_id=operation WHERE id=receipt.id;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',c.id,'termsRevision',(c.terms_revision+1)::text,'termsHash',app.auth_json_hash(terms_value)));
END $$;
REVOKE ALL ON FUNCTION app.set_ante_stake(uuid,uuid,numeric,jsonb,numeric,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
