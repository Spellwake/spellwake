BEGIN;

-- Private canonical observation/transaction boundary. A trusted Rust caller must
-- first authenticate actual credential bytes in ControllerSecretStore; these
-- locators and service-role RPC privileges are not credential possession.
CREATE FUNCTION app.ai_ante_original_is_valid(p_source jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
BEGIN
 IF jsonb_typeof(p_source) IS DISTINCT FROM 'object' OR octet_length(p_source::text)>32768 THEN RETURN false;END IF;
 IF EXISTS(SELECT 1 FROM jsonb_object_keys(p_source) k WHERE k NOT IN('requestId','matchId','seatId','expectedTermsRevision','coins'))
  OR jsonb_typeof(p_source->'requestId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_source->'matchId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(p_source->'seatId') IS DISTINCT FROM 'string'
  OR NOT app.native_uuid_is_canonical(p_source->>'requestId')
  OR NOT app.native_uuid_is_canonical(p_source->>'matchId')
  OR NOT app.native_uuid_is_canonical(p_source->>'seatId')
  OR jsonb_typeof(p_source->'expectedTermsRevision') IS DISTINCT FROM 'string'
  OR ((p_source->>'expectedTermsRevision') ~ '^(0|[1-9][0-9]{0,77})$') IS DISTINCT FROM true
  OR jsonb_typeof(p_source->'coins') IS DISTINCT FROM 'string'
  OR ((p_source->>'coins') ~ '^(0|[1-9][0-9]{0,2})$') IS DISTINCT FROM true THEN RETURN false;END IF;
 RETURN (p_source->>'coins')::numeric<=300;
END $$;

-- VOLATILE is deliberate for these read-only observers: distinct pre/post SQL
-- statements need fresh READ COMMITTED snapshots, including after a source wait.
CREATE FUNCTION app.read_controller_ai_ante_context(p_grant uuid,p_epoch numeric,p_secret_ref text,p_match uuid) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor jsonb;context jsonb;body jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"actor":"unavailable"}}'::jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller ante requires read committed retry';END IF;
 actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,p_match);
 IF actor IS NULL THEN RETURN unavailable;END IF;
 BEGIN
  SELECT jsonb_build_object('matchId',m.id,'seatId',seat.id,'sponsorId',seat.sponsor_id,
   'matchStartedAt',to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
   'commitmentId',commitment.id,'exposureId',exposure.id) INTO STRICT context
  FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id AND seat.id=(actor->>'seatId')::uuid
  LEFT JOIN app.ante_commitment commitment ON commitment.match_id=m.id
  LEFT JOIN app.ai_loss_bucket bucket ON bucket.sponsor_id=seat.sponsor_id AND bucket.utc_day=to_char(m.started_at AT TIME ZONE 'UTC','YYYY-MM-DD')
  LEFT JOIN app.ai_loss_exposure exposure ON exposure.bucket_id=bucket.id AND exposure.match_id=m.id AND exposure.commitment_id=commitment.id
  WHERE m.id=p_match AND m.started_at IS NOT NULL AND seat.controller_kind='ai' AND seat.account_id IS NULL AND seat.sponsor_id IS NOT NULL;
 EXCEPTION WHEN no_data_found THEN context:=NULL;
 END;
 IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,p_match) IS DISTINCT FROM actor THEN RETURN unavailable;END IF;
 IF context IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('match','unavailable'));END IF;
 body:=jsonb_build_object('status','success','data',jsonb_build_object('actor',actor,'context',context));
 IF octet_length(body::text)>32768 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='controller ante context exceeds private response budget';END IF;
 RETURN body;
END $$;

CREATE FUNCTION app.read_controller_ai_ante_original(p_grant uuid,p_epoch numeric,p_secret_ref text,p_source jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor jsonb;receipt app.command_receipt;fingerprint text;disposition jsonb;body jsonb;ack jsonb;matched uuid;seat_id uuid;
 unavailable constant jsonb:='{"status":"fail","data":{"actor":"unavailable"}}'::jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller ante requires read committed retry';END IF;
 IF NOT app.ai_ante_original_is_valid(p_source) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid'));END IF;
 matched:=(p_source->>'matchId')::uuid;seat_id:=(p_source->>'seatId')::uuid;
 actor:=app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,matched);
 IF actor IS NULL OR actor->>'seatId' IS DISTINCT FROM seat_id::text THEN RETURN unavailable;END IF;
 fingerprint:=app.auth_json_hash(jsonb_build_object('match',matched,'seat',seat_id,
  'basis',p_source->>'expectedTermsRevision','assets','[]'::jsonb,'aiCoins',p_source->>'coins'));
 SELECT * INTO receipt FROM app.command_receipt
 WHERE principal_key='ante-seat:'||seat_id::text AND scope='ante_offer_v1' AND request_ref=p_source->>'requestId';
 IF receipt.id IS NULL THEN disposition:=jsonb_build_object('state','unknown','response',NULL);
 ELSE
  IF receipt.target_ref IS DISTINCT FROM matched::text OR receipt.payload_hash IS DISTINCT FROM fingerprint THEN
   IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,matched) IS DISTINCT FROM actor THEN RETURN unavailable;END IF;
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict'));END IF;
  IF receipt.account_id IS NOT NULL OR receipt.controller_grant_id IS NOT NULL OR receipt.result_ref<>'' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante original has foreign canonical identity';END IF;
  IF receipt.state='pending' THEN
   IF receipt.response_payload IS NOT NULL OR receipt.operation_id IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pending AI ante original contains a result';END IF;
   disposition:=jsonb_build_object('state','pending','response',NULL);
  ELSIF receipt.state='accepted' THEN
   ack:=receipt.response_payload->'data';
   IF jsonb_typeof(ack) IS DISTINCT FROM 'object'
    OR jsonb_typeof(ack->'resourceId') IS DISTINCT FROM 'string' OR NOT app.native_uuid_is_canonical(ack->>'resourceId')
    OR jsonb_typeof(ack->'termsRevision') IS DISTINCT FROM 'string'
    OR ack->>'termsRevision' IS DISTINCT FROM (((p_source->>'expectedTermsRevision')::numeric+1)::text)
    OR jsonb_typeof(ack->'termsHash') IS DISTINCT FROM 'string' OR ((ack->>'termsHash') ~ '^[0-9a-f]{64}$') IS DISTINCT FROM true
    OR receipt.response_payload IS DISTINCT FROM jsonb_build_object('status','success','data',jsonb_build_object('resourceId',ack->>'resourceId','termsRevision',ack->>'termsRevision','termsHash',ack->>'termsHash')) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante original acknowledgment is inconsistent';END IF;
   IF NOT EXISTS(SELECT 1 FROM app.ante_commitment c WHERE c.id=(ack->>'resourceId')::uuid AND c.match_id=matched)
    OR NOT EXISTS(SELECT 1 FROM app.ledger_operation operation JOIN app.match_seat seat ON seat.id=seat_id AND seat.match_id=matched
     WHERE operation.id=receipt.operation_id AND operation.kind='ante_offer' AND operation.source_ref=receipt.id::text
      AND operation.source_scope='ante_offer_v1' AND operation.actor_account_id=seat.sponsor_id AND operation.state='committed') THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante original lacks its canonical commitment and operation';END IF;
   -- Historical hash/revision are retained receipt values, not today's terms.
   disposition:=jsonb_build_object('state','accepted','response',receipt.response_payload);
  ELSIF receipt.state='rejected' THEN
   IF receipt.operation_id IS NOT NULL OR receipt.response_payload NOT IN(
    '{"status":"fail","data":{"match":"unavailable initial ante window"}}'::jsonb,
    '{"status":"fail","data":{"terms":"stale or no longer offered"}}'::jsonb) OR receipt.response_payload IS NULL THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante original has an undeclared rejection';END IF;
   disposition:=jsonb_build_object('state','rejected','response',receipt.response_payload);
  ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante original has an invalid state';END IF;
 END IF;
 IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,matched) IS DISTINCT FROM actor THEN RETURN unavailable;END IF;
 body:=jsonb_build_object('status','success','data',jsonb_build_object('actor',actor,'original',p_source,'disposition',disposition));
 IF octet_length(body::text)>32768 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='controller ante original exceeds private response budget';END IF;
 RETURN body;
END $$;

CREATE FUNCTION app.submit_controller_ai_ante(p_grant uuid,p_epoch numeric,p_secret_ref text,p_source jsonb) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' AS $$
DECLARE original jsonb;actor jsonb;reply jsonb;observed jsonb;matched uuid;
 unavailable constant jsonb:='{"status":"fail","data":{"actor":"unavailable"}}'::jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='controller ante requires read committed retry';END IF;
 original:=app.read_controller_ai_ante_original(p_grant,p_epoch,p_secret_ref,p_source);
 IF original->>'status' IS DISTINCT FROM 'success' OR original->'data'->'disposition'->>'state' IS DISTINCT FROM 'unknown' THEN RETURN original;END IF;
 actor:=original->'data'->'actor';matched:=(p_source->>'matchId')::uuid;
 -- Preserve SQL136's receipt-first, sorted participant-account, then match lock
 -- order. Do not acquire the controller sponsor ahead of that canonical owner.
 BEGIN
  reply:=app.set_ante_stake(matched,(p_source->>'seatId')::uuid,(p_source->>'expectedTermsRevision')::numeric,
   '[]'::jsonb,(p_source->>'coins')::numeric,(p_source->>'requestId')::uuid);
  -- Separate VOLATILE statement: fresh snapshot. New writes hold canonical
  -- account/match fences; source changes before acquisition must roll back all
  -- inner receipt/economic writes. Legitimate business rejection stays durable.
  IF app.read_native_controller_actor(p_grant,p_epoch,p_secret_ref,matched) IS DISTINCT FROM actor THEN
   RAISE EXCEPTION USING ERRCODE='SW241',MESSAGE='controller authority changed during ante commit';END IF;
  observed:=app.read_controller_ai_ante_original(p_grant,p_epoch,p_secret_ref,p_source);
  IF observed=unavailable THEN RAISE EXCEPTION USING ERRCODE='SW241',MESSAGE='controller authority changed during ante commit';END IF;
  IF observed->>'status' IS DISTINCT FROM 'success' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante canonical original is unavailable after writer';END IF;
  IF observed->'data'->'actor' IS DISTINCT FROM actor THEN
   RAISE EXCEPTION USING ERRCODE='SW241',MESSAGE='controller authority changed during ante commit';END IF;
  IF (observed->'data'->'disposition'->>'state' IN('accepted','rejected')) IS DISTINCT FROM true
   OR observed->'data'->'disposition'->'response' IS DISTINCT FROM reply THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='AI ante canonical original differs from writer result';END IF;
 EXCEPTION WHEN SQLSTATE 'SW241' THEN
  -- The subtransaction rollback includes SQL136's new original and all economic
  -- effects. A local post-IO denial in Rust has a different, non-rollback meaning.
  RETURN unavailable;
 WHEN check_violation THEN
  -- A competing different original can commit after the pre-read and before
  -- SQL136's unique-receipt wait. Normalize only that owner's exact conflict;
  -- all other source/constraint failures remain exceptions, never stored blame.
  IF SQLERRM='ante offer source terms changed' THEN
   observed:=app.read_controller_ai_ante_original(p_grant,p_epoch,p_secret_ref,p_source);
   IF observed=unavailable OR observed='{"status":"fail","data":{"request":"conflict"}}'::jsonb THEN RETURN observed;END IF;
  END IF;
  RAISE;
 END;
 RETURN observed;
END $$;

REVOKE ALL ON FUNCTION app.ai_ante_original_is_valid(jsonb),
 app.read_controller_ai_ante_context(uuid,numeric,text,uuid),
 app.read_controller_ai_ante_original(uuid,numeric,text,jsonb),
 app.submit_controller_ai_ante(uuid,numeric,text,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_controller_ai_ante_context(uuid,numeric,text,uuid),
 app.read_controller_ai_ante_original(uuid,numeric,text,jsonb),
 app.submit_controller_ai_ante(uuid,numeric,text,jsonb) TO service_role;
COMMIT;
