-- Session-bound transport over the existing hold, ledger and receipt writers.
BEGIN;

CREATE FUNCTION app.economy_command_terms(p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE kind text; command jsonb; keys text[]; expected text[]; request_id uuid; target uuid;
 scope_name text; terms jsonb; assets uuid[]; policy uuid;
BEGIN
 IF jsonb_typeof(p_input) IS DISTINCT FROM 'object' OR octet_length(p_input::text)>65536
 OR (SELECT array_agg(k ORDER BY k) FROM jsonb_object_keys(p_input) k) IS DISTINCT FROM ARRAY['command','kind']::text[]
 OR jsonb_typeof(p_input->'kind') IS DISTINCT FROM 'string'
 OR jsonb_typeof(p_input->'command') IS DISTINCT FROM 'object' THEN RETURN NULL; END IF;
 kind:=p_input->>'kind'; command:=p_input->'command';
 SELECT array_agg(k ORDER BY k) INTO keys FROM jsonb_object_keys(command) k;
 IF jsonb_typeof(command->'requestId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
 request_id:=(command->>'requestId')::uuid;
 IF kind='trade_create' THEN
  expected:=ARRAY['give','recipientId','request','requestId']; scope_name:='trade_create';
  IF jsonb_typeof(command->'recipientId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
  target:=(command->>'recipientId')::uuid;
  assets:=app.bundle_asset_ids(command->'give')||app.bundle_asset_ids(command->'request');
  IF cardinality(assets)=0 OR '00000000-0000-0000-0000-000000000000'::uuid=ANY(assets) THEN RETURN NULL; END IF;
  terms:=jsonb_build_object('give',command->'give','request',command->'request');
 ELSIF kind='trade_response' THEN
  expected:=ARRAY['action','requestId','tradeId']; scope_name:='trade_response';
  IF jsonb_typeof(command->'tradeId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'action') IS DISTINCT FROM 'string'
  OR command->>'action' NOT IN ('accept','reject','cancel') THEN RETURN NULL; END IF;
  target:=(command->>'tradeId')::uuid; terms:=jsonb_build_object('action',command->>'action');
 ELSIF kind='listing_create' THEN
  expected:=ARRAY['lot','requestId','totalPrice']; scope_name:='market_create';
  assets:=app.bundle_asset_ids(jsonb_build_array(command->'lot')); target:=assets[1];
  IF jsonb_typeof(command->'totalPrice') IS DISTINCT FROM 'string'
  OR command->>'totalPrice' !~ '^[1-9][0-9]{0,77}$' THEN RETURN NULL; END IF;
  terms:=jsonb_build_object('quantity',command->'lot'->>'quantity','price',command->>'totalPrice');
 ELSIF kind IN ('listing_cancel','listing_buy') THEN
  IF jsonb_typeof(command->'listingId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
  target:=(command->>'listingId')::uuid;
  IF kind='listing_cancel' THEN
   expected:=ARRAY['listingId','requestId']; scope_name:='market_cancel'; terms:='{}'::jsonb;
  ELSE
   expected:=ARRAY['expectedRevision','expectedTotal','listingId','requestId']; scope_name:='market_buy';
   IF jsonb_typeof(command->'expectedRevision') IS DISTINCT FROM 'string'
   OR jsonb_typeof(command->'expectedTotal') IS DISTINCT FROM 'string'
   OR command->>'expectedRevision' !~ '^[1-9][0-9]{0,77}$'
   OR command->>'expectedTotal' !~ '^[1-9][0-9]{0,77}$' THEN RETURN NULL; END IF;
   terms:=jsonb_build_object('revision',command->>'expectedRevision','price',command->>'expectedTotal');
  END IF;
 ELSIF kind='pack_buy' THEN
  expected:=ARRAY['expectedPolicyId','expectedRevision','expectedTotal','quantity','recipeId','requestId']; scope_name:='pack_purchase';
  IF jsonb_typeof(command->'recipeId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'expectedPolicyId') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'quantity') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'expectedRevision') IS DISTINCT FROM 'string'
  OR jsonb_typeof(command->'expectedTotal') IS DISTINCT FROM 'string'
  OR command->>'quantity' !~ '^[1-9][0-9]{0,3}$'
  OR command->>'expectedRevision' !~ '^(0|[1-9][0-9]{0,77})$'
  OR command->>'expectedTotal' !~ '^[1-9][0-9]{0,77}$' THEN RETURN NULL; END IF;
  IF (command->>'quantity')::integer>1000 THEN RETURN NULL; END IF;
  target:=(command->>'recipeId')::uuid; policy:=(command->>'expectedPolicyId')::uuid;
  IF policy='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN NULL; END IF;
  terms:=jsonb_build_object('quantity',(command->>'quantity')::integer,'policy',policy,
   'revision',command->>'expectedRevision','total',command->>'expectedTotal');
 ELSIF kind='pack_open' THEN
  expected:=ARRAY['inventoryAssetId','requestId']; scope_name:='pack_open';
  IF jsonb_typeof(command->'inventoryAssetId') IS DISTINCT FROM 'string' THEN RETURN NULL; END IF;
  target:=(command->>'inventoryAssetId')::uuid; terms:=jsonb_build_object('inventoryAssetId',target);
 ELSE RETURN NULL;
 END IF;
 IF keys IS DISTINCT FROM expected OR request_id='00000000-0000-0000-0000-000000000000'::uuid
 OR target IS NULL OR target='00000000-0000-0000-0000-000000000000'::uuid THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('requestId',request_id,'scope',scope_name,'targetId',target,'terms',terms);
EXCEPTION WHEN invalid_text_representation OR invalid_parameter_value OR numeric_value_out_of_range THEN RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION app.economy_command_terms(jsonb) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE FUNCTION app.execute_economy_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; actor_id uuid; peer_id uuid; target uuid; request_id uuid;
 player app.account; command jsonb; kind text; result jsonb;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='economy session command requires read committed'; END IF;
 parsed:=app.economy_command_terms(p_input);
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 target:=(parsed->>'targetId')::uuid; request_id:=(parsed->>'requestId')::uuid;
 kind:=p_input->>'kind'; command:=p_input->'command';
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT id INTO actor_id FROM app.account WHERE auth_subject=p_subject;
 IF actor_id IS NULL THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified economy account required'; END IF;
 -- Discover immutable participants without locking an offer/listing ahead of accounts.
 IF kind='trade_create' THEN peer_id:=target;
 ELSIF kind='trade_response' THEN
  SELECT CASE WHEN t.sender_id=actor_id THEN t.recipient_id ELSE t.sender_id END INTO peer_id
  FROM app.trade_offer t WHERE t.id=target AND actor_id IN (t.sender_id,t.recipient_id);
 ELSIF kind='listing_buy' THEN
  SELECT seller_id INTO peer_id FROM app.market_listing WHERE id=target;
 END IF;
 PERFORM id FROM app.account WHERE id IN (actor_id,peer_id) ORDER BY id FOR NO KEY UPDATE;
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified economy account changed'; END IF;
 CASE kind
 WHEN 'trade_create' THEN result:=app.create_trade(actor_id,target,command->'give',command->'request',request_id);
 WHEN 'trade_response' THEN result:=app.respond_trade(actor_id,target,command->>'action',request_id);
 WHEN 'listing_create' THEN result:=app.create_listing(actor_id,target,command->'lot'->>'quantity',command->>'totalPrice',request_id);
 WHEN 'listing_cancel' THEN result:=app.cancel_listing(actor_id,target,request_id);
 WHEN 'listing_buy' THEN result:=app.buy_listing(actor_id,target,command->>'expectedRevision',command->>'expectedTotal',request_id);
 WHEN 'pack_buy' THEN result:=app.buy_packs(actor_id,target,(command->>'quantity')::integer,
  (command->>'expectedPolicyId')::uuid,command->>'expectedRevision',command->>'expectedTotal',request_id);
 WHEN 'pack_open' THEN result:=app.open_pack(actor_id,target,request_id);
 END CASE;
 -- Refresh after downstream catalogue, receipt, product and inventory waits.
 -- Raising here rolls back this block's writer effects and receipt atomically.
 -- This does not promise to serialize every managed Auth change through COMMIT.
 player:=app.lock_session_account(p_subject,p_session);
 IF player.id IS DISTINCT FROM actor_id THEN RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified economy account changed'; END IF;
 RETURN result;
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.read_economy_command(p_subject uuid,p_session uuid,p_input jsonb) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE parsed jsonb; player app.account; receipt app.command_receipt;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='economy session command requires read committed'; END IF;
 parsed:=app.economy_command_terms(p_input);
 IF parsed IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 player:=app.lock_session_account(p_subject,p_session);
 SELECT * INTO receipt FROM app.command_receipt WHERE principal_key='human:'||player.id::text
 AND scope=parsed->>'scope' AND request_ref=parsed->>'requestId';
 IF FOUND AND (receipt.target_ref IS DISTINCT FROM parsed->>'targetId'
 OR receipt.payload_hash IS DISTINCT FROM encode(sha256(convert_to((parsed->'terms')::text,'UTF8')),'hex')) THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RETURN jsonb_build_object('status','success','data',app.read_account_command(player.id,parsed->>'scope',(parsed->>'requestId')::uuid));
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
END $$;
REVOKE ALL ON FUNCTION app.execute_economy_command(uuid,uuid,jsonb),app.read_economy_command(uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.execute_economy_command(uuid,uuid,jsonb),app.read_economy_command(uuid,uuid,jsonb) TO service_role;
REVOKE EXECUTE ON FUNCTION app.create_trade(uuid,uuid,jsonb,jsonb,uuid),app.respond_trade(uuid,uuid,text,uuid),
 app.create_listing(uuid,uuid,text,text,uuid),app.cancel_listing(uuid,uuid,uuid),app.buy_listing(uuid,uuid,text,text,uuid),
 app.buy_packs(uuid,uuid,integer,uuid,text,text,uuid),app.open_pack(uuid,uuid,uuid) FROM service_role;
COMMIT;
