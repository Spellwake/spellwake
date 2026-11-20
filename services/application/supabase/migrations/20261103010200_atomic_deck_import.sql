BEGIN;

-- Reuse the canonical writers in one transaction. Rejected content cannot leave an empty deck.
CREATE FUNCTION app.import_deck(p_actor uuid,p_name text,p_mode text,p_format uuid,p_entries jsonb,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; created jsonb; saved jsonb; failure jsonb;
 deck_id uuid; revision_id uuid; rejected boolean:=false;
BEGIN
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid import request'; END IF;
 receipt:=app.begin_account_command(p_actor,p_request,'deck_import',p_actor::text,
  jsonb_build_object('name',p_name,'mode',p_mode,'format',p_format,'entries',p_entries));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','unavailable')); END IF;
 BEGIN
  -- An internal receipt UUID is isolated from caller-selected operation identities.
  created:=app.create_deck(p_actor,p_name,p_mode,p_format,receipt.id);
  IF created->>'status' IS DISTINCT FROM 'success' THEN
   failure:=created->'data'; RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='rejected deck import'; END IF;
  deck_id:=(created->'data'->>'resourceId')::uuid;
  saved:=app.save_deck_contents(p_actor,deck_id,NULL,p_entries,receipt.id);
  IF saved->>'status' IS DISTINCT FROM 'success' THEN
   failure:=saved->'data'; RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='rejected deck import'; END IF;
  revision_id:=(saved->'data'->>'resourceId')::uuid;
 EXCEPTION WHEN SQLSTATE 'SW001' THEN
  -- Database writes in the block roll back; the outer source receipt records the rejection.
  rejected:=true;
 END;
 IF rejected THEN RETURN app.finish_account_command(receipt.id,false,failure); END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',deck_id,'revisionId',revision_id));
END $$;
REVOKE ALL ON FUNCTION app.import_deck(uuid,text,text,uuid,jsonb,uuid)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

COMMIT;
