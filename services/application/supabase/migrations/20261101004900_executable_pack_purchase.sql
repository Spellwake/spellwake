BEGIN;

-- A valued recipe need not yet have a qualified execution codec. Existing
-- ownership remains readable; new purchases must be openable by this service.
ALTER FUNCTION app.buy_packs(uuid,uuid,integer,uuid,text,text,uuid) RENAME TO buy_packs_priced;
REVOKE EXECUTE ON FUNCTION app.buy_packs_priced(uuid,uuid,integer,uuid,text,text,uuid) FROM PUBLIC,anon,authenticated,service_role;
CREATE FUNCTION app.buy_packs(p_actor uuid,p_recipe uuid,p_quantity integer,p_policy uuid,p_revision text,p_total text,p_request uuid)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe WHERE id=p_recipe AND opening_qualified_at IS NOT NULL) THEN
  receipt:=app.begin_account_command(p_actor,p_request,'pack_purchase',p_recipe::text,
   jsonb_build_object('quantity',p_quantity,'policy',p_policy,'revision',p_revision,'total',p_total));
  IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
  PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
  IF NOT FOUND THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('account','account unavailable')); END IF;
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('product','opening recipe not qualified'));
 END IF;
 RETURN app.buy_packs_priced(p_actor,p_recipe,p_quantity,p_policy,p_revision,p_total,p_request);
END $$;
GRANT EXECUTE ON FUNCTION app.buy_packs(uuid,uuid,integer,uuid,text,text,uuid) TO service_role;

COMMIT;
