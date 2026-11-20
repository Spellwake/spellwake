BEGIN;

-- Shared transaction-time authority check. This does not lock every managed
-- Auth revocation row through commit; it refreshes authority after account waits.
CREATE FUNCTION app.lock_session_account(p_subject uuid,p_session uuid) RETURNS app.account
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account; authority jsonb;
BEGIN
 PERFORM id FROM auth.users WHERE id=p_subject FOR KEY SHARE;
 SELECT * INTO player FROM app.account WHERE auth_subject=p_subject FOR NO KEY UPDATE;
 authority:=app.read_auth_session_authority(p_subject,p_session);
 IF player.id IS NULL OR authority IS NULL OR player.sign_in_method IS DISTINCT FROM authority->>'method'
 OR player.state NOT IN ('active','deletion_pending') THEN
  RAISE EXCEPTION USING ERRCODE='28000',MESSAGE='verified live account required';
 END IF;
 RETURN player;
END $$;
REVOKE EXECUTE ON FUNCTION app.lock_session_account(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Existing Stripe callers retain their interface and lock order.
CREATE OR REPLACE FUNCTION app.stripe_actor(p_subject uuid,p_session uuid) RETURNS app.account
LANGUAGE sql SECURITY DEFINER SET search_path='' AS $$
 SELECT app.lock_session_account(p_subject,p_session)
$$;

CREATE FUNCTION app.create_deck_command(p_subject uuid,p_session uuid,p_request uuid,p_name text,p_mode text,p_format uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 -- Canonical writer replays accepted receipts before rejecting new pending-account work.
 RETURN app.create_deck(player.id,p_name,p_mode,p_format,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;

CREATE FUNCTION app.save_deck_command(p_subject uuid,p_session uuid,p_request uuid,p_deck uuid,p_expected_revision uuid,p_entries jsonb)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE player app.account;
BEGIN
 player:=app.lock_session_account(p_subject,p_session);
 IF p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','invalid')); END IF;
 RETURN app.save_deck_contents(player.id,p_deck,p_expected_revision,p_entries,p_request);
EXCEPTION WHEN invalid_authorization_specification THEN
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
WHEN check_violation THEN
 IF SQLERRM='request identity was used for different command terms' THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('request','conflict')); END IF;
 RAISE;
END $$;
REVOKE EXECUTE ON FUNCTION app.create_deck_command(uuid,uuid,uuid,text,text,uuid),app.save_deck_command(uuid,uuid,uuid,uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.create_deck_command(uuid,uuid,uuid,text,text,uuid),app.save_deck_command(uuid,uuid,uuid,uuid,uuid,jsonb) TO service_role;

CREATE FUNCTION app.read_my_deck(p_deck uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT jsonb_build_object('deck',jsonb_build_object('id',d.id,'name',d.name,'mode',d.mode,'currentRevisionId',d.current_revision_id,
  'format',jsonb_build_object('id',f.id,'key',f.key,'name',f.name),
  'appearance',jsonb_build_object('backId',d.back_id,'sleeveId',d.sleeve_id,'diceId',d.dice_id,'playmatId',d.playmat_id)),
  'revision',CASE WHEN r.id IS NULL THEN NULL ELSE jsonb_build_object('id',r.id,'number',r.revision_number::text,
   'entries',(SELECT coalesce(jsonb_agg(jsonb_build_object('definitionId',e.definition_id,'printingId',e.printing_id,'section',e.section,'quantity',e.quantity::text)
    ORDER BY e.section,e.definition_id,e.printing_id NULLS FIRST),'[]') FROM app.deck_entry e WHERE e.revision_id=r.id)) END)
 INTO result FROM app.deck d JOIN app.format f ON f.id=d.format_id
 LEFT JOIN app.deck_revision r ON r.id=d.current_revision_id AND r.deck_id=d.id AND r.sealed_at IS NOT NULL
 WHERE d.id=p_deck AND d.account_id=actor AND d.source_kind='account';
 IF result IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('deck','unavailable')); END IF;
 RETURN jsonb_build_object('status','success','data',result);
END $$;

CREATE FUNCTION app.read_my_formats(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY INVOKER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; result jsonb;
BEGIN
 IF p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 OR p_after='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid')); END IF;
 IF actor IS NULL THEN
  authority:=app.read_current_session();
  IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));
 END IF;
 SELECT coalesce(jsonb_agg(page.entry ORDER BY page.id),'[]') INTO result FROM (
  SELECT f.id,jsonb_build_object('id',f.id,'key',f.key,'name',f.name) entry FROM app.format f
  WHERE p_after IS NULL OR f.id>p_after ORDER BY f.id LIMIT p_limit
 ) page;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('items',result,
  'nextCursor',CASE WHEN jsonb_array_length(result)=p_limit THEN result->-1->>'id' ELSE NULL END));
END $$;
REVOKE EXECUTE ON FUNCTION app.read_my_deck(uuid),app.read_my_formats(uuid,integer) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_deck(uuid),app.read_my_formats(uuid,integer) TO authenticated;

COMMIT;
