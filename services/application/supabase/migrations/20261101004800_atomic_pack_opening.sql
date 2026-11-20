BEGIN;

REVOKE INSERT,UPDATE,DELETE ON app.pack_open_receipt FROM service_role;

CREATE FUNCTION app.guard_pack_open_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_OP<>'INSERT' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='accepted pack outcome is immutable'; END IF;
 IF NOT EXISTS(SELECT 1 FROM app.ledger_operation o JOIN app.command_receipt c ON c.id::text=o.source_ref
 JOIN app.inventory_asset i ON i.id=NEW.inventory_asset_id JOIN app.asset_owner a ON a.id=i.owner_id
 JOIN app.asset_definition d ON d.id=i.asset_definition_id
 WHERE o.id=NEW.operation_id AND o.kind='pack_open' AND o.source_scope='pack_open' AND o.state='pending'
 AND o.actor_account_id=a.account_id AND c.account_id=a.account_id AND c.scope='pack_open' AND c.state='pending'
 AND c.operation_id=o.id AND c.target_ref=i.id::text AND d.kind='pack' AND d.recipe_id=NEW.recipe_id) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack receipt requires its pending actor and inventory source';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER pack_open_history BEFORE INSERT OR UPDATE OR DELETE ON app.pack_open_receipt
 FOR EACH ROW EXECUTE FUNCTION app.guard_pack_open_receipt();

CREATE FUNCTION app.validate_pack_open(p_operation uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE opened app.pack_open_receipt; recipe app.booster_recipe; result jsonb; expected jsonb;
 owner uuid; actor uuid; wanted jsonb; actual jsonb;
BEGIN
 SELECT * INTO STRICT opened FROM app.pack_open_receipt WHERE operation_id=p_operation;
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=opened.recipe_id;
 SELECT i.owner_id,a.account_id INTO STRICT owner,actor FROM app.inventory_asset i JOIN app.asset_owner a ON a.id=i.owner_id WHERE i.id=opened.inventory_asset_id;
 result:=opened.result_ref::jsonb;
 IF jsonb_typeof(result) IS DISTINCT FROM 'object' OR result-ARRAY['version','seed','planHash','recipeQualification','contents']<>'{}'::jsonb
 OR result->>'version' IS DISTINCT FROM 'pack-result-sha256-counter512-v1'
 OR jsonb_typeof(result->'seed') IS DISTINCT FROM 'string' OR NOT coalesce(result->>'seed' ~ '^[0-9a-f]{64}$',false)
 OR result->>'planHash' IS DISTINCT FROM recipe.opening_plan_hash
 OR result->>'recipeQualification' IS DISTINCT FROM recipe.qualification_ref THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack result codec or qualification is inconsistent';
 END IF;
 expected:=app.draw_booster_recipe_v1(opened.recipe_id,decode(result->>'seed','hex'));
 IF result->'contents' IS DISTINCT FROM expected OR NOT EXISTS(
 SELECT 1 FROM app.ledger_operation o JOIN app.command_receipt c ON c.id::text=o.source_ref
 WHERE o.id=p_operation AND o.kind='pack_open' AND o.source_scope='pack_open' AND o.state='committed'
 AND o.actor_account_id=actor AND c.account_id=actor AND c.scope='pack_open' AND c.state='accepted'
 AND c.operation_id=o.id AND c.target_ref=opened.inventory_asset_id::text
 AND c.response_payload=jsonb_build_object('status','success','data',jsonb_build_object('resourceId',opened.id))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack result requires its exact accepted operation';
 END IF;
 SELECT jsonb_object_agg(value->>'printingId',value->>'quantity') INTO wanted FROM jsonb_array_elements(expected);
 SELECT coalesce(jsonb_object_agg(p.printing_id::text,p.quantity::text),'{}') INTO actual FROM (
  SELECT d.printing_id,sum(l.quantity) AS quantity FROM app.ledger_posting l JOIN app.inventory_asset i ON i.id=l.inventory_asset_id
  JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE l.operation_id=p_operation AND l.direction='credit'
  AND d.kind='card' GROUP BY d.printing_id) p;
 IF wanted IS DISTINCT FROM actual
 OR (SELECT count(*) FROM app.ledger_posting WHERE operation_id=p_operation)<>jsonb_array_length(expected)+1
 OR (SELECT count(*) FROM app.ledger_posting WHERE operation_id=p_operation AND direction='debit' AND inventory_asset_id=opened.inventory_asset_id AND quantity=1)<>1
 OR EXISTS(SELECT 1 FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id JOIN app.asset_definition d ON d.id=i.asset_definition_id
 WHERE p.operation_id=p_operation AND (i.owner_id<>owner OR p.source_kind<>'pack_open' OR p.source_ref<>opened.id::text
 OR NOT((p.direction='debit' AND i.id=opened.inventory_asset_id AND p.quantity=1) OR (p.direction='credit' AND d.kind='card')))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack postings differ from one consumed pack and committed contents';
 END IF;
END $$;

CREATE FUNCTION app.check_pack_open_commit() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF TG_TABLE_NAME='pack_open_receipt' THEN PERFORM app.validate_pack_open(NEW.operation_id);
 ELSE PERFORM app.validate_pack_open(NEW.id); END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER pack_open_commit AFTER INSERT ON app.pack_open_receipt
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION app.check_pack_open_commit();
CREATE CONSTRAINT TRIGGER pack_open_operation_commit AFTER INSERT OR UPDATE ON app.ledger_operation
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW WHEN(NEW.kind='pack_open')
 EXECUTE FUNCTION app.check_pack_open_commit();

CREATE FUNCTION app.open_pack(p_actor uuid,p_inventory uuid,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE command app.command_receipt; recipe app.booster_recipe; pack app.inventory_asset; owner uuid;
 seed bytea; contents jsonb; line jsonb; card uuid; ids uuid[]; inventory uuid; available numeric;
 operation uuid:=gen_random_uuid(); opened uuid:=gen_random_uuid(); result jsonb;
BEGIN
 IF p_inventory IS NULL OR p_inventory='00000000-0000-0000-0000-000000000000'::uuid THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='pack inventory identity required';
 END IF;
 command:=app.begin_account_command(p_actor,p_request,'pack_open',p_inventory::text,jsonb_build_object('inventoryAssetId',p_inventory));
 IF command.state<>'pending' THEN RETURN command.response_payload; END IF;
 PERFORM id FROM app.account WHERE id=p_actor AND state='active' FOR NO KEY UPDATE;
 IF NOT FOUND THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('account','account unavailable')); END IF;
 SELECT i.* INTO pack FROM app.inventory_asset i JOIN app.asset_owner a ON a.id=i.owner_id
 JOIN app.asset_definition d ON d.id=i.asset_definition_id WHERE i.id=p_inventory AND a.account_id=p_actor AND a.state='active' AND d.kind='pack';
 IF NOT FOUND THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('pack','owned pack unavailable')); END IF;
 owner:=pack.owner_id;
 PERFORM app.lock_valuation_catalogue(false);
 SELECT r.* INTO STRICT recipe FROM app.booster_recipe r JOIN app.asset_definition d ON d.recipe_id=r.id
 WHERE d.id=pack.asset_definition_id FOR SHARE OF r;
 IF recipe.opening_plan IS NULL THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('pack','opening recipe not qualified')); END IF;
 -- Reject obviously unavailable stacks without creating visible zero-owned
 -- output rows. The locked check below remains the authorization fence.
 SELECT a.available INTO available FROM app.asset_availability a WHERE a.inventory_asset_id=p_inventory;
 IF available<1 THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('pack','no unheld pack available')); END IF;
 seed:=app.new_pack_seed_v1(); contents:=app.draw_booster_recipe_v1(recipe.id,seed); ids:=ARRAY[p_inventory];
 FOR line IN SELECT value FROM jsonb_array_elements(contents) LOOP
  INSERT INTO app.asset_definition(kind,printing_id) VALUES('card',(line->>'printingId')::uuid) ON CONFLICT(printing_id) DO NOTHING;
  SELECT id INTO STRICT card FROM app.asset_definition WHERE kind='card' AND printing_id=(line->>'printingId')::uuid;
  inventory:=app.ensure_inventory(owner,card); ids:=array_append(ids,inventory);
 END LOOP;
 PERFORM id FROM app.inventory_asset WHERE id=ANY(ids) ORDER BY id FOR NO KEY UPDATE;
 SELECT a.available INTO available FROM app.asset_availability a WHERE a.inventory_asset_id=p_inventory;
 IF available<1 THEN RETURN app.finish_account_command(command.id,false,jsonb_build_object('pack','no unheld pack available')); END IF;
 INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state)
 VALUES(operation,'pack_open',command.id::text,'pack_open',p_actor,clock_timestamp(),'pending');
 UPDATE app.command_receipt SET operation_id=operation WHERE id=command.id;
 result:=jsonb_build_object('version','pack-result-sha256-counter512-v1','seed',encode(seed,'hex'),
 'planHash',recipe.opening_plan_hash,'recipeQualification',recipe.qualification_ref,'contents',contents);
 INSERT INTO app.pack_open_receipt(id,inventory_asset_id,recipe_id,operation_id,result_ref) VALUES(opened,p_inventory,recipe.id,operation,result::text);
 INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
 VALUES(operation,p_inventory,'debit',1,'pack_open',opened::text);
 FOR line IN SELECT value FROM jsonb_array_elements(contents) LOOP
  SELECT i.id INTO STRICT inventory FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id
  WHERE i.owner_id=owner AND d.printing_id=(line->>'printingId')::uuid AND d.kind='card';
  INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
  VALUES(operation,inventory,'credit',(line->>'quantity')::numeric,'pack_open',opened::text);
 END LOOP;
 UPDATE app.ledger_operation SET state='committed' WHERE id=operation;
 RETURN app.finish_account_command(command.id,true,jsonb_build_object('resourceId',opened));
END $$;
GRANT EXECUTE ON FUNCTION app.open_pack(uuid,uuid,uuid) TO service_role;

CREATE FUNCTION app.read_pack_open(p_actor uuid,p_open uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE opened app.pack_open_receipt;
BEGIN
 SELECT r.* INTO opened FROM app.pack_open_receipt r JOIN app.inventory_asset i ON i.id=r.inventory_asset_id
 JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.account a ON a.id=o.account_id
 WHERE r.id=p_open AND a.id=p_actor AND a.state IN ('active','deletion_pending');
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='42501',MESSAGE='pack outcome unavailable'; END IF;
 RETURN jsonb_build_object('id',opened.id,'inventoryAssetId',opened.inventory_asset_id,'recipeId',opened.recipe_id,
 'operationId',opened.operation_id,'contents',opened.result_ref::jsonb->'contents');
END $$;
GRANT EXECUTE ON FUNCTION app.read_pack_open(uuid,uuid) TO service_role;

COMMIT;
