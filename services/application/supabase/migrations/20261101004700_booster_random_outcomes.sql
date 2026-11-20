BEGIN;

-- PG17 gen_random_uuid uses pg_strong_random before fixing six UUID bits.
-- Two independent values provide244 random bits; SHA256 domain-separates the
-- retained seed. No extension, client seed or shared random-state owner.
CREATE FUNCTION app.new_pack_seed_v1() RETURNS bytea
LANGUAGE sql VOLATILE SET search_path='' AS $$
 SELECT sha256(convert_to('spellwake-pack-seed-v1:','UTF8')||uuid_send(gen_random_uuid())||uuid_send(gen_random_uuid()))
$$;

-- Exact integer rejection avoids modulo bias, including78-digit weights and
-- totals exceeding256 bits. A pathological rejection exhaustion fails atomically.
CREATE FUNCTION app.pack_sample_v1(p_seed bytea,p_counter bigint,p_max numeric)
RETURNS TABLE(sample numeric,next_counter bigint)
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE bytes bytea; value numeric; space numeric; threshold numeric; i integer; attempt integer;
BEGIN
 IF p_seed IS NULL OR octet_length(p_seed)<>32 OR p_counter IS NULL OR p_counter NOT BETWEEN 0 AND 1000000000
 OR p_max IS NULL OR NOT(p_max>0 AND p_max<power(10::numeric,83) AND scale(p_max)=0) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid bounded pack random input';
 END IF;
 next_counter:=p_counter;
 FOR attempt IN 1..16 LOOP
  bytes:=sha256(p_seed||convert_to('spellwake-pack-v1:'||next_counter::text||':0','UTF8'))
      ||sha256(p_seed||convert_to('spellwake-pack-v1:'||next_counter::text||':1','UTF8'));
  next_counter:=next_counter+1; value:=0; space:=1;
  FOR i IN 0..63 LOOP value:=value*256+get_byte(bytes,i); space:=space*256; END LOOP;
  threshold:=space-mod(space,p_max);
  IF value<threshold THEN sample:=mod(value,p_max); RETURN NEXT; RETURN; END IF;
 END LOOP;
 RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='pack random rejection bound exhausted';
END $$;

CREATE FUNCTION app.draw_booster_recipe_v1(p_recipe uuid,p_seed bytea) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE recipe app.booster_recipe; slot app.booster_slot; candidate app.booster_slot_card;
 group_data jsonb; outcome jsonb; line jsonb; total numeric; target numeric; cumulative numeric;
 counter bigint:=0; draw integer; quantity integer; picked uuid; picks uuid[]:='{}'; result jsonb; used jsonb;
BEGIN
 SELECT * INTO STRICT recipe FROM app.booster_recipe WHERE id=p_recipe;
 IF recipe.qualified_at IS NULL OR recipe.opening_plan IS NULL OR p_seed IS NULL OR octet_length(p_seed)<>32 THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='qualified executable pack and private seed required';
 END IF;
 FOR slot IN SELECT * FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref IS NULL ORDER BY id LOOP
  SELECT sum(weight) INTO total FROM app.booster_slot_card WHERE slot_id=slot.id;
  FOR draw IN 1..slot.draws::integer LOOP
   SELECT sample,next_counter INTO target,counter FROM app.pack_sample_v1(p_seed,counter,total);
   cumulative:=0; picked:=NULL;
   FOR candidate IN SELECT * FROM app.booster_slot_card WHERE slot_id=slot.id ORDER BY printing_id LOOP
    cumulative:=cumulative+candidate.weight;
    IF target<cumulative THEN picked:=candidate.printing_id; EXIT; END IF;
   END LOOP;
   IF picked IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack weighted draw has no outcome'; END IF;
   picks:=array_append(picks,picked);
  END LOOP;
 END LOOP;
 FOR group_data IN SELECT value FROM jsonb_array_elements(recipe.opening_plan->'groups') WITH ORDINALITY ORDER BY ordinality LOOP
  IF group_data->>'kind'='multiset-without-replacement' THEN
   SELECT * INTO STRICT slot FROM app.booster_slot WHERE recipe_id=p_recipe AND collation_ref=group_data->>'collationRef';
   SELECT sum(weight) INTO total FROM app.booster_slot_card WHERE slot_id=slot.id;
   used:='{}';
   FOR draw IN 1..slot.draws::integer LOOP
    SELECT sample,next_counter INTO target,counter FROM app.pack_sample_v1(p_seed,counter,total-(draw-1));
    cumulative:=0; picked:=NULL;
    FOR candidate IN SELECT * FROM app.booster_slot_card WHERE slot_id=slot.id ORDER BY printing_id LOOP
     cumulative:=cumulative+candidate.weight-coalesce((used->>candidate.printing_id::text)::numeric,0);
     IF target<cumulative THEN picked:=candidate.printing_id; EXIT; END IF;
    END LOOP;
    IF picked IS NULL THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='multiset draw has no remaining unit'; END IF;
    used:=jsonb_set(used,ARRAY[picked::text],to_jsonb((coalesce((used->>picked::text)::integer,0)+1)::text));
    picks:=array_append(picks,picked);
   END LOOP;
   CONTINUE;
  END IF;
  SELECT sum((value->>'weight')::numeric) INTO total FROM jsonb_array_elements(group_data->'outcomes');
  SELECT sample,next_counter INTO target,counter FROM app.pack_sample_v1(p_seed,counter,total);
  cumulative:=0;
  FOR outcome IN SELECT value FROM jsonb_array_elements(group_data->'outcomes') WITH ORDINALITY ORDER BY ordinality LOOP
   cumulative:=cumulative+(outcome->>'weight')::numeric;
   IF target<cumulative THEN
    FOR line IN SELECT value FROM jsonb_array_elements(outcome->'cards') LOOP
     quantity:=(line->>'quantity')::integer;
     FOR draw IN 1..quantity LOOP picks:=array_append(picks,(line->>'printingId')::uuid); END LOOP;
    END LOOP;
    EXIT;
   END IF;
  END LOOP;
 END LOOP;
 SELECT coalesce(jsonb_agg(jsonb_build_object('printingId',p.printing,'quantity',p.quantity::text) ORDER BY p.printing),'[]')
 INTO result FROM (SELECT value AS printing,count(*) AS quantity FROM unnest(picks) value GROUP BY value) p;
 IF jsonb_array_length(result)=0 THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='pack has no authored contents'; END IF;
 RETURN result;
END $$;

COMMIT;
