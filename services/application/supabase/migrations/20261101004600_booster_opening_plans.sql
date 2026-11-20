BEGIN;

ALTER TABLE app.booster_recipe ADD COLUMN opening_plan jsonb;
ALTER TABLE app.booster_recipe ADD COLUMN opening_plan_hash text;
ALTER TABLE app.booster_recipe ADD COLUMN opening_qualified_at timestamptz;
ALTER TABLE app.booster_recipe ADD CHECK (
 (opening_plan IS NULL AND opening_plan_hash IS NULL AND opening_qualified_at IS NULL) OR
 (opening_plan IS NOT NULL AND opening_plan_hash IS NOT NULL AND opening_qualified_at IS NOT NULL
 AND qualified_at IS NOT NULL AND opening_plan_hash ~ '^sha256:[0-9a-f]{64}$'));

-- Independent slots retain their weighted draws with replacement. Every named
-- correlated group requires a joint distribution; marginals cannot invent it.
CREATE FUNCTION app.validate_booster_opening_plan(p_recipe uuid,p_plan jsonb) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE group_data jsonb; outcome jsonb; line jsonb; reference text; refs text[]:='{}';
 total_weight numeric; weighted_count numeric; count_cards numeric; slot app.booster_slot;
 candidate record; seen text[]; identity text; weight numeric; outcomes integer:=0; entries integer:=0; mode text;
BEGIN
 IF NOT EXISTS(SELECT 1 FROM app.booster_recipe WHERE id=p_recipe AND qualified_at IS NOT NULL)
 OR jsonb_typeof(p_plan) IS DISTINCT FROM 'object' OR p_plan-ARRAY['version','groups']<>'{}'::jsonb
 OR p_plan->>'version' IS DISTINCT FROM 'pack-opening-v1' OR octet_length(p_plan::text)>1048576
 OR jsonb_typeof(p_plan->'groups') IS DISTINCT FROM 'array' OR jsonb_array_length(p_plan->'groups')>64 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='qualified recipe and bounded opening plan required';
 END IF;
 FOR group_data IN SELECT value FROM jsonb_array_elements(p_plan->'groups') LOOP
  reference:=group_data->>'collationRef';
  mode:=coalesce(group_data->>'kind','joint');
  IF jsonb_typeof(group_data) IS DISTINCT FROM 'object' OR group_data-ARRAY['collationRef','kind','outcomes']<>'{}'::jsonb
  OR jsonb_typeof(group_data->'collationRef') IS DISTINCT FROM 'string' OR length(reference) NOT BETWEEN 1 AND 1024
  OR (group_data ? 'kind' AND jsonb_typeof(group_data->'kind') IS DISTINCT FROM 'string')
  OR reference=ANY(refs) OR NOT EXISTS(SELECT 1 FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref=reference)
  OR mode NOT IN ('joint','multiset-without-replacement') THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid correlated opening group';
  END IF;
  refs:=array_append(refs,reference); total_weight:=0;
  IF mode='multiset-without-replacement' THEN
   IF group_data ? 'outcomes' OR (SELECT count(*) FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref=reference)<>1 THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiset codec requires exactly one authored slot';
   END IF;
   SELECT * INTO STRICT slot FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref=reference;
   SELECT sum(c.weight) INTO total_weight FROM app.booster_slot_card c WHERE c.slot_id=slot.id;
   IF slot.draws>total_weight THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiset has fewer units than draws'; END IF;
   FOR candidate IN SELECT * FROM app.booster_slot_card WHERE slot_id=slot.id LOOP
    IF candidate.expected_numerator*total_weight<>slot.draws*candidate.weight*candidate.expected_denominator THEN
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiset marginals disagree with authored multiplicities';
    END IF;
   END LOOP;
   CONTINUE;
  END IF;
  IF jsonb_typeof(group_data->'outcomes') IS DISTINCT FROM 'array' OR jsonb_array_length(group_data->'outcomes') NOT BETWEEN 1 AND 10000 THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='joint outcomes required';
  END IF;
  FOR outcome IN SELECT value FROM jsonb_array_elements(group_data->'outcomes') LOOP
   outcomes:=outcomes+1;
   IF outcomes>10000 OR jsonb_typeof(outcome) IS DISTINCT FROM 'object' OR outcome-ARRAY['weight','cards']<>'{}'::jsonb
   OR jsonb_typeof(outcome->'weight') IS DISTINCT FROM 'string' OR NOT coalesce(outcome->>'weight' ~ '^[1-9][0-9]{0,77}$',false)
   OR jsonb_typeof(outcome->'cards') IS DISTINCT FROM 'array' OR jsonb_array_length(outcome->'cards') NOT BETWEEN 1 AND 10000 THEN
    RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid weighted joint outcome';
   END IF;
   weight:=(outcome->>'weight')::numeric; total_weight:=total_weight+weight; seen:='{}';
   FOR line IN SELECT value FROM jsonb_array_elements(outcome->'cards') LOOP
    entries:=entries+1;
    IF entries>10000 OR jsonb_typeof(line) IS DISTINCT FROM 'object' OR line-ARRAY['slotId','printingId','quantity']<>'{}'::jsonb
    OR jsonb_typeof(line->'slotId') IS DISTINCT FROM 'string' OR jsonb_typeof(line->'printingId') IS DISTINCT FROM 'string'
    OR jsonb_typeof(line->'quantity') IS DISTINCT FROM 'string' OR NOT coalesce(line->>'quantity' ~ '^[1-9][0-9]{0,3}$',false) THEN
     RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid joint outcome card';
    END IF;
    identity:=(line->>'slotId')::uuid::text||':'||(line->>'printingId')::uuid::text;
    IF identity=ANY(seen) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='duplicate joint outcome card'; END IF;
    seen:=array_append(seen,identity);
    SELECT s.* INTO slot FROM app.booster_slot s JOIN app.booster_slot_card c ON c.slot_id=s.id
    WHERE s.id=(line->>'slotId')::uuid AND c.printing_id=(line->>'printingId')::uuid
    AND s.recipe_id=p_recipe AND s.collation_ref=reference;
    IF NOT FOUND OR (line->>'quantity')::numeric>slot.draws THEN
     RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='joint outcome does not belong to authored slots';
    END IF;
   END LOOP;
   FOR slot IN SELECT * FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref=reference LOOP
    SELECT coalesce(sum((value->>'quantity')::numeric),0) INTO count_cards
    FROM jsonb_array_elements(outcome->'cards') WHERE (value->>'slotId')::uuid=slot.id;
    IF count_cards<>slot.draws THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='joint outcome changes authored draw count'; END IF;
   END LOOP;
  END LOOP;
  FOR candidate IN SELECT c.* FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id
   WHERE s.recipe_id=p_recipe AND s.collation_ref=reference LOOP
   SELECT coalesce(sum((o.value->>'weight')::numeric*coalesce((SELECT (l.value->>'quantity')::numeric
    FROM jsonb_array_elements(o.value->'cards') l WHERE (l.value->>'slotId')::uuid=candidate.slot_id
    AND (l.value->>'printingId')::uuid=candidate.printing_id),0)),0) INTO weighted_count
   FROM jsonb_array_elements(group_data->'outcomes') o;
   IF weighted_count*candidate.expected_denominator<>candidate.expected_numerator*total_weight THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='joint opening distribution disagrees with qualified marginals';
   END IF;
  END LOOP;
 END LOOP;
 IF EXISTS(SELECT 1 FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref IS NOT NULL AND NOT(collation_ref=ANY(refs))) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='correlated slots have no executable joint distribution';
 END IF;
END $$;

CREATE OR REPLACE FUNCTION app.guard_recipe_qualification() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE slot app.booster_slot; candidate app.booster_slot_card; total app.exact_value; slot_count bigint;
BEGIN
 IF TG_OP='INSERT' AND NEW.opening_plan IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening plan requires separate qualification';
 END IF;
 IF TG_OP='UPDATE' AND OLD.qualified_at IS NOT NULL AND OLD.opening_plan IS NULL AND NEW.opening_plan IS NOT NULL
 AND (to_jsonb(NEW)-ARRAY['opening_plan','opening_plan_hash','opening_qualified_at'])=(to_jsonb(OLD)-ARRAY['opening_plan','opening_plan_hash','opening_qualified_at']) THEN
  IF NEW.opening_qualified_at IS NULL OR NEW.opening_plan_hash IS DISTINCT FROM 'sha256:'||encode(sha256(convert_to(NEW.opening_plan::text,'UTF8')),'hex') THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening qualification hash is inconsistent';
  END IF;
  PERFORM app.validate_booster_opening_plan(NEW.id,NEW.opening_plan);
  RETURN NEW;
 END IF;
 IF TG_OP='UPDATE' AND OLD.qualified_at IS NULL AND NEW.opening_plan IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='marginals must qualify before opening plan';
 END IF;
 IF TG_OP='DELETE' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe history cannot be deleted'; END IF;
 IF TG_OP='UPDATE' AND (OLD.qualified_at IS NOT NULL OR
 (NEW.id,NEW.product_id,NEW.revision_ref,NEW.evidence_ref) IS DISTINCT FROM (OLD.id,OLD.product_id,OLD.revision_ref,OLD.evidence_ref)) THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe source and qualified history are immutable';
 END IF;
 IF NEW.qualified_at IS NULL THEN RETURN NEW; END IF;
 IF TG_OP='INSERT' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe must be assembled before qualification'; END IF;
 IF btrim(NEW.evidence_ref)='' THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe evidence is required'; END IF;
 SELECT count(*) INTO slot_count FROM app.booster_slot WHERE recipe_id=NEW.id;
 IF slot_count NOT BETWEEN 1 AND 64 OR
 (SELECT count(*) FROM app.booster_slot_card c JOIN app.booster_slot s ON s.id=c.slot_id WHERE s.recipe_id=NEW.id)>10000 THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe size is unavailable or exceeds qualification bounds';
 END IF;
 FOR slot IN SELECT * FROM app.booster_slot WHERE recipe_id=NEW.id ORDER BY id LOOP
  total:=app.value_make(0,1);
  IF slot.collation_ref IS NOT NULL AND btrim(slot.collation_ref)='' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='collation evidence is required';
  END IF;
  IF slot.collation_ref IS NULL THEN
   IF NOT EXISTS(SELECT 1 FROM app.booster_slot_card WHERE slot_id=slot.id)
   OR EXISTS(SELECT 1 FROM app.booster_slot_card WHERE slot_id=slot.id AND expected_numerator IS NOT NULL) THEN
    RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='independent slot requires weighted candidates without marginal overrides';
   END IF;
   total:=app.value_make(slot.draws,1);
  ELSE
   FOR candidate IN SELECT * FROM app.booster_slot_card WHERE slot_id=slot.id ORDER BY id LOOP
    total:=app.value_add(total,app.slot_expected(candidate.id));
   END LOOP;
  END IF;
  IF total.numerator<>slot.draws*total.denominator THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='slot expected copies must equal its authored draw count';
  END IF;
 END LOOP;
 RETURN NEW;
END $$;

CREATE FUNCTION app.qualify_booster_opening(p_recipe uuid,p_plan jsonb,p_hash text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE recipe app.booster_recipe;
BEGIN
 PERFORM app.lock_valuation_catalogue(false);
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=p_recipe FOR NO KEY UPDATE;
 IF recipe.opening_plan IS NOT NULL THEN
  IF recipe.opening_plan IS DISTINCT FROM p_plan OR recipe.opening_plan_hash IS DISTINCT FROM p_hash THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='opening qualification is immutable';
  END IF;
  RETURN;
 END IF;
 UPDATE app.booster_recipe SET opening_plan=p_plan,opening_plan_hash=p_hash,opening_qualified_at=clock_timestamp() WHERE id=p_recipe;
END $$;
GRANT EXECUTE ON FUNCTION app.qualify_booster_opening(uuid,jsonb,text) TO service_role;

COMMIT;
