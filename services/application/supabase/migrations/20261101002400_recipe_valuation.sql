BEGIN;

-- Fractions stay exact through bundle comparisons. SQL numeric division would
-- round, so normalization uses integer div/mod only. Bounds cap adversarial work.
CREATE TYPE app.exact_value AS (numerator numeric,denominator numeric);
REVOKE ALL ON TYPE app.exact_value FROM PUBLIC;
CREATE FUNCTION app.value_make(p_n numeric,p_d numeric) RETURNS app.exact_value
LANGUAGE plpgsql IMMUTABLE STRICT SET search_path='' AS $$
DECLARE a numeric:=p_n; b numeric:=p_d; temporary numeric;
BEGIN
 IF NOT (p_n>=0 AND p_n<'Infinity'::numeric AND p_d>0 AND p_d<'Infinity'::numeric AND scale(p_n)=0 AND scale(p_d)=0)
 OR length(p_n::text)>8192 OR length(p_d::text)>8192 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid exact valuation component';
 END IF;
 WHILE b<>0 LOOP temporary:=mod(a,b); a:=b; b:=temporary; END LOOP;
 RETURN ROW(div(p_n,a),div(p_d,a))::app.exact_value;
END $$;
CREATE FUNCTION app.value_add(p_a app.exact_value,p_b app.exact_value) RETURNS app.exact_value
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT app.value_make((p_a).numerator*(p_b).denominator+(p_b).numerator*(p_a).denominator,(p_a).denominator*(p_b).denominator)
$$;
CREATE FUNCTION app.value_less(p_a app.exact_value,p_b app.exact_value) RETURNS boolean
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT (p_a).numerator*(p_b).denominator<(p_b).numerator*(p_a).denominator
$$;
CREATE FUNCTION app.value_json(p_value app.exact_value) RETURNS jsonb
LANGUAGE sql IMMUTABLE STRICT SET search_path='' AS $$
 SELECT jsonb_build_object('numerator',(p_value).numerator::text,'denominator',(p_value).denominator::text)
$$;

ALTER TABLE app.booster_recipe ADD COLUMN content_revision numeric NOT NULL DEFAULT 0
 CHECK (content_revision>=0 AND content_revision<power(10::numeric,78) AND scale(content_revision)=0);
ALTER TABLE app.booster_recipe ADD COLUMN qualified_at timestamptz;
ALTER TABLE app.booster_recipe ADD COLUMN qualification_ref text;
ALTER TABLE app.booster_recipe ADD CONSTRAINT recipe_qualification_group CHECK
 ((qualified_at IS NULL AND qualification_ref IS NULL) OR (qualified_at IS NOT NULL AND qualification_ref IS NOT NULL AND qualification_ref ~ '^sha256:[0-9a-f]{64}$'));
-- A changed immutable recipe is a different collectible pack product. Historical
-- drafts may coexist; one product cannot mix two qualified pack identities.
CREATE UNIQUE INDEX qualified_product_recipe ON app.booster_recipe(product_id) WHERE qualified_at IS NOT NULL;
ALTER TABLE app.booster_slot ADD CHECK (draws BETWEEN 1 AND 1000);
ALTER TABLE app.booster_slot_card ADD CHECK (weight>0);
ALTER TABLE app.booster_slot_card ADD COLUMN expected_numerator numeric;
ALTER TABLE app.booster_slot_card ADD COLUMN expected_denominator numeric;
ALTER TABLE app.booster_slot_card ADD CHECK (
 (expected_numerator IS NULL AND expected_denominator IS NULL) OR
 (expected_numerator IS NOT NULL AND expected_denominator IS NOT NULL AND expected_numerator>0
 AND expected_denominator>0 AND expected_numerator<power(10::numeric,78) AND expected_denominator<power(10::numeric,78)
 AND scale(expected_numerator)=0 AND scale(expected_denominator)=0));

CREATE FUNCTION app.slot_expected(p_card uuid) RETURNS app.exact_value
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE candidate app.booster_slot_card; slot app.booster_slot; total numeric;
BEGIN
 SELECT * INTO STRICT candidate FROM app.booster_slot_card WHERE id=p_card;
 SELECT * INTO STRICT slot FROM app.booster_slot WHERE id=candidate.slot_id;
 IF slot.collation_ref IS NOT NULL THEN
  IF candidate.expected_numerator IS NULL OR candidate.expected_denominator IS NULL THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='correlated collation requires qualified exact marginals';
  END IF;
  RETURN app.value_make(candidate.expected_numerator,candidate.expected_denominator);
 END IF;
 IF candidate.expected_numerator IS NOT NULL THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='independent slots derive their marginals from weights';
 END IF;
 SELECT sum(weight) INTO total FROM app.booster_slot_card WHERE slot_id=slot.id;
 RETURN app.value_make(slot.draws*candidate.weight,total);
END $$;

CREATE FUNCTION app.guard_recipe_content() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE recipe uuid; sealed timestamptz;
BEGIN
 IF TG_TABLE_NAME='booster_slot' THEN
  recipe:=CASE WHEN TG_OP='DELETE' THEN OLD.recipe_id ELSE NEW.recipe_id END;
  IF TG_OP='UPDATE' AND (NEW.id,NEW.recipe_id) IS DISTINCT FROM (OLD.id,OLD.recipe_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe slot identity is immutable';
  END IF;
 ELSE
  SELECT recipe_id INTO STRICT recipe FROM app.booster_slot WHERE id=CASE WHEN TG_OP='DELETE' THEN OLD.slot_id ELSE NEW.slot_id END;
  IF TG_OP='UPDATE' AND (NEW.id,NEW.slot_id,NEW.printing_id) IS DISTINCT FROM (OLD.id,OLD.slot_id,OLD.printing_id) THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe candidate identity is immutable';
  END IF;
 END IF;
 SELECT qualified_at INTO sealed FROM app.booster_recipe WHERE id=recipe FOR NO KEY UPDATE;
 IF sealed IS NOT NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified recipe content is immutable'; END IF;
 UPDATE app.booster_recipe SET content_revision=content_revision+1 WHERE id=recipe;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER recipe_slot_content BEFORE INSERT OR UPDATE OR DELETE ON app.booster_slot
 FOR EACH ROW EXECUTE FUNCTION app.guard_recipe_content();
CREATE TRIGGER recipe_candidate_content BEFORE INSERT OR UPDATE OR DELETE ON app.booster_slot_card
 FOR EACH ROW EXECUTE FUNCTION app.guard_recipe_content();

CREATE FUNCTION app.guard_recipe_qualification() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE slot app.booster_slot; candidate app.booster_slot_card; total app.exact_value; slot_count bigint;
BEGIN
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
CREATE TRIGGER recipe_qualification BEFORE INSERT OR UPDATE OR DELETE ON app.booster_recipe
 FOR EACH ROW EXECUTE FUNCTION app.guard_recipe_qualification();

-- Trusted content qualification records a reviewed artifact hash. This function
-- cannot establish the truth of a source document or execute a collation codec.
CREATE FUNCTION app.qualify_booster_recipe(p_recipe uuid,p_manifest text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE existing text;
BEGIN
 SELECT qualification_ref INTO existing FROM app.booster_recipe WHERE id=p_recipe FOR NO KEY UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='recipe unavailable'; END IF;
 IF existing IS NOT NULL THEN
  IF existing IS DISTINCT FROM p_manifest THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='recipe qualification changed'; END IF;
  RETURN;
 END IF;
 UPDATE app.booster_recipe SET qualified_at=clock_timestamp(),qualification_ref=p_manifest WHERE id=p_recipe;
END $$;
GRANT EXECUTE ON FUNCTION app.qualify_booster_recipe(uuid,text) TO service_role;

COMMIT;
