BEGIN;
-- Acyclic base source projection. The full minimization reader is deliberately
-- not called here: its later work-set validation uses these exact terms.
CREATE FUNCTION app.account_resource_terms(p_account uuid,p_grant uuid,p_retirement uuid) RETURNS jsonb LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT jsonb_build_object('version','account-resource-cleanup-v1','kind','controller_secret','accountId',a.id,'retirementReceiptId',r.id,'retirementOperationId',a.retirement_operation_id,'grantId',g.id,'bindingId',b.id,'epoch',g.binding_epoch::text,'secretRef',g.grant_secret_ref)
 FROM app.account a JOIN app.command_receipt r ON r.id=p_retirement JOIN app.controller_grant g ON g.id=p_grant JOIN app.ai_runner_binding b ON b.id=g.binding_id JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id
 WHERE a.id=p_account AND a.state='deleted' AND a.retirement_operation_id IS NOT NULL AND r.scope='account_retirement_v1' AND r.account_id=a.id AND r.principal_key='human:'||a.id::text AND r.target_ref=a.id::text AND r.request_ref=a.id::text AND r.payload_hash=app.auth_json_hash(jsonb_build_object('version',1,'scope','account_retirement_v1','accountId',a.id)) AND r.operation_id=a.retirement_operation_id AND r.state IN('pending','accepted') AND p_account IN(slot.sponsor_id,room.host_id) AND g.state='revoked'
$$;
CREATE FUNCTION app.record_account_resource_work(p_account uuid,p_retirement uuid) RETURNS uuid[] LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE g app.controller_grant; source jsonb; job app.command_receipt; jobs uuid[]:='{}'::uuid[]; operation uuid;
BEGIN
 SELECT retirement_operation_id INTO operation FROM app.account WHERE id=p_account FOR UPDATE;
 PERFORM app.check_registered_auth_retirement(p_account,operation);
 FOR g IN SELECT grant_row.* FROM app.controller_grant grant_row JOIN app.ai_runner_binding b ON b.id=grant_row.binding_id JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE p_account IN(slot.sponsor_id,room.host_id) ORDER BY grant_row.id LOOP
 source:=app.account_resource_terms(p_account,g.id,p_retirement);
 IF source IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='resource cleanup requires exact retired revoked authority';END IF;
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('account-resources:'||p_account::text,'account_resource_cleanup_v1',g.id::text,g.id::text,app.auth_json_hash(source),'pending','',source) ON CONFLICT DO NOTHING RETURNING * INTO job;
 IF job.id IS NULL THEN
 SELECT * INTO STRICT job FROM app.command_receipt WHERE principal_key='account-resources:'||p_account::text AND scope='account_resource_cleanup_v1' AND request_ref=g.id::text FOR UPDATE;
 IF job.state<>'accepted' OR job.target_ref<>g.id::text OR job.normalized_source IS DISTINCT FROM source OR job.payload_hash<>app.auth_json_hash(source) THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='resource cleanup source conflicts';END IF;
 ELSE
 PERFORM app.finish_account_command(job.id,true,jsonb_build_object('resourceId',g.id,'kind','controller_secret','disposition','cleanup pending'));
 END IF;
 jobs:=array_append(jobs,job.id);
 END LOOP;
 SELECT coalesce(array_agg(id ORDER BY id),'{}'::uuid[]) INTO jobs FROM unnest(jobs) id;
 RETURN jobs;
END $$;
CREATE FUNCTION app.account_resource_absent(p_work uuid) RETURNS boolean LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS(SELECT 1 FROM app.command_receipt job JOIN app.command_receipt ack ON ack.principal_key=job.principal_key AND ack.scope='account_resource_ack_v1' AND ack.target_ref=job.id::text
 WHERE job.id=p_work AND job.scope='account_resource_cleanup_v1' AND job.state='accepted' AND ack.state='accepted' AND ack.normalized_source->>'version'='account-resource-ack-v1' AND ack.normalized_source->>'workId'=job.id::text AND ack.normalized_source->>'workHash'=job.payload_hash AND ack.normalized_source->>'accountId'=job.normalized_source->>'accountId' AND ack.normalized_source->>'epoch'=job.normalized_source->>'epoch' AND ack.normalized_source->>'outcome'='absent' AND ack.operation_id IS NULL AND ack.account_id IS NULL AND ack.controller_grant_id IS NULL AND ack.payload_hash=app.auth_json_hash(ack.normalized_source) AND length(btrim(ack.normalized_source->>'proofRef')) BETWEEN 1 AND 2048 AND ack.normalized_source=jsonb_build_object('version','account-resource-ack-v1','accountId',job.normalized_source->>'accountId','workId',job.id,'workHash',job.payload_hash,'epoch',job.normalized_source->>'epoch','outcome','absent','proofRef',ack.normalized_source->>'proofRef') AND ack.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',job.id,'disposition','absent')))
$$;
CREATE FUNCTION app.read_account_resource_work(p_account uuid,p_after uuid,p_limit integer) RETURNS jsonb LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE items jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid resource cleanup page';END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=p_account AND a.state='deleted' AND CASE WHEN a.state='deleted' THEN (app.read_account_retirement_proof(a.id,a.retirement_operation_id)).id IS NOT NULL ELSE false END) THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='canonical retired resource account required';END IF;
 SELECT coalesce(jsonb_agg(jsonb_build_object('workId',id,'grantId',normalized_source->>'grantId','bindingId',normalized_source->>'bindingId','epoch',normalized_source->>'epoch','secretRef',normalized_source->>'secretRef','retirementOperationId',normalized_source->>'retirementOperationId') ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT job.* FROM app.command_receipt job WHERE job.principal_key='account-resources:'||p_account::text AND job.scope='account_resource_cleanup_v1' AND job.state='accepted' AND (p_after IS NULL OR job.id>p_after) AND NOT app.account_resource_absent(job.id) ORDER BY job.id LIMIT p_limit) page;
 RETURN jsonb_build_object('items',items,'nextCursor',CASE WHEN jsonb_array_length(items)=p_limit THEN items->-1->>'workId' ELSE NULL END);
END $$;
CREATE FUNCTION app.observe_account_resource_cleanup(p_account uuid,p_work uuid,p_epoch numeric,p_outcome text,p_proof text,p_request uuid) RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE job app.command_receipt; receipt app.command_receipt; terms jsonb; fingerprint text; retirement uuid;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='resource observation requires read committed retry';END IF;
 IF p_work IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_epoch IS NULL OR p_epoch::text !~ '^(0|[1-9][0-9]{0,77})$' OR p_outcome IS NULL OR p_outcome NOT IN('unknown','absent') OR p_proof IS NULL OR length(btrim(p_proof)) NOT BETWEEN 1 AND 2048 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid resource observation envelope';END IF;
 PERFORM id FROM app.account WHERE id=p_account AND state='deleted' FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='retired resource owner unavailable';END IF;
 SELECT * INTO job FROM app.command_receipt WHERE id=p_work AND principal_key='account-resources:'||p_account::text AND scope='account_resource_cleanup_v1' FOR SHARE;
 SELECT id INTO retirement FROM app.command_receipt WHERE account_id=p_account AND principal_key='human:'||p_account::text AND scope='account_retirement_v1' AND request_ref=p_account::text AND state='accepted';
 IF job.id IS NULL OR job.state<>'accepted' OR job.normalized_source->>'epoch' IS DISTINCT FROM p_epoch::text OR job.normalized_source IS DISTINCT FROM app.account_resource_terms(p_account,(job.normalized_source->>'grantId')::uuid,retirement) OR (app.read_account_retirement_proof(p_account,(job.normalized_source->>'retirementOperationId')::uuid)).id IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='resource observation source differs';END IF;
 terms:=jsonb_build_object('version','account-resource-ack-v1','accountId',p_account,'workId',job.id,'workHash',job.payload_hash,'epoch',p_epoch::text,'outcome',p_outcome,'proofRef',p_proof);fingerprint:=app.auth_json_hash(terms);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES(job.principal_key,'account_resource_ack_v1',job.id::text,p_request::text,fingerprint,'pending','',terms) ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
 SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key=job.principal_key AND scope='account_resource_ack_v1' AND request_ref=p_request::text FOR UPDATE;
 IF receipt.target_ref<>job.id::text OR receipt.payload_hash<>fingerprint THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='resource observation request terms conflict';END IF;
 IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='resource observation disposition pending';END IF;RETURN receipt.response_payload;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',job.id,'disposition',p_outcome));
END $$;
REVOKE ALL ON FUNCTION app.account_resource_terms(uuid,uuid,uuid),app.record_account_resource_work(uuid,uuid),app.account_resource_absent(uuid),app.read_account_resource_work(uuid,uuid,integer),app.observe_account_resource_cleanup(uuid,uuid,numeric,text,text,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
