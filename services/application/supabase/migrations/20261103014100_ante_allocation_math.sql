BEGIN;
-- Raise the lowest allocated-value cohort in whole-unit batches, then resolve
-- indivisible ties with recorded hashes. Work is bounded by recipient cohorts,
-- rather than iterating once per coin. Accepted quantities stay exact.
CREATE FUNCTION app.split_ante_asset(p_winners uuid[],p_balances jsonb,p_quantity numeric,p_unit_n numeric,p_unit_d numeric,p_seed text,p_stake uuid,p_asset uuid) RETURNS jsonb
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE count_value integer; values_array app.exact_value[]:='{}'; shares numeric[]; index_value integer; remaining numeric; chosen integer;
 tie text; chosen_tie text; unit_value app.exact_value; allocations jsonb:='[]'; balances jsonb:='[]'; value_item jsonb;
 lowest integer; highest_cohort integer; outside integer; cohort boolean[]; cohort_count integer; threshold app.exact_value; rounds numeric; gap_n numeric; gap_d numeric;
BEGIN
 count_value:=cardinality(p_winners);
 IF p_winners IS NULL OR count_value NOT BETWEEN 1 AND 6 OR array_position(p_winners,NULL) IS NOT NULL OR count_value<>(SELECT count(DISTINCT id) FROM unnest(p_winners) id)
 OR p_quantity IS NULL OR p_quantity::text !~ '^[1-9][0-9]{0,77}$' OR p_unit_n IS NULL OR p_unit_n<=0 OR p_unit_d IS NULL OR p_unit_d<=0 OR p_seed IS NULL OR p_seed !~ '^[0-9a-f]{64}$'
 OR jsonb_typeof(p_balances) IS DISTINCT FROM 'array' OR jsonb_array_length(p_balances)<>count_value THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid bounded ante allocation input'; END IF;
 unit_value:=app.value_make(p_unit_n,p_unit_d);shares:=array_fill(0::numeric,ARRAY[count_value]);
 FOR value_item IN SELECT * FROM jsonb_array_elements(p_balances) LOOP values_array:=array_append(values_array,app.value_make((value_item->>'numerator')::numeric,(value_item->>'denominator')::numeric)); END LOOP;
 remaining:=p_quantity;
 WHILE remaining>0 LOOP
 lowest:=1;
 FOR index_value IN 2..count_value LOOP IF app.value_less(values_array[index_value],values_array[lowest]) THEN lowest:=index_value; END IF; END LOOP;
 threshold:=app.value_add(values_array[lowest],unit_value);cohort:=array_fill(false,ARRAY[count_value]);cohort_count:=0;highest_cohort:=lowest;outside:=NULL;
 FOR index_value IN 1..count_value LOOP
 IF app.value_less(values_array[index_value],threshold) THEN
 cohort[index_value]:=true;cohort_count:=cohort_count+1;
 IF app.value_less(values_array[highest_cohort],values_array[index_value]) THEN highest_cohort:=index_value; END IF;
 ELSIF outside IS NULL OR app.value_less(values_array[index_value],values_array[outside]) THEN outside:=index_value; END IF;
 END LOOP;
 rounds:=div(remaining,cohort_count);
 IF outside IS NOT NULL THEN
 gap_n:=(values_array[outside]).numerator*(values_array[highest_cohort]).denominator-(values_array[highest_cohort]).numerator*(values_array[outside]).denominator;
 gap_d:=(values_array[outside]).denominator*(values_array[highest_cohort]).denominator;
 rounds:=least(rounds,div(gap_n*unit_value.denominator,gap_d*unit_value.numerator));
 END IF;
 IF rounds>0 THEN
 FOR index_value IN 1..count_value LOOP IF cohort[index_value] THEN shares[index_value]:=shares[index_value]+rounds;values_array[index_value]:=app.value_add(values_array[index_value],app.value_make(unit_value.numerator*rounds,unit_value.denominator)); END IF; END LOOP;
 remaining:=remaining-rounds*cohort_count;CONTINUE;
 END IF;
 chosen:=NULL;chosen_tie:=NULL;
 FOR index_value IN 1..count_value LOOP
 tie:=encode(sha256(convert_to(p_seed||':'||p_stake::text||':'||p_asset::text||':'||remaining::text||':'||p_winners[index_value]::text,'UTF8')),'hex');
 IF chosen IS NULL OR app.value_less(values_array[index_value],values_array[chosen]) OR (NOT app.value_less(values_array[chosen],values_array[index_value]) AND tie<chosen_tie) THEN chosen:=index_value;chosen_tie:=tie; END IF;
 END LOOP;
 shares[chosen]:=shares[chosen]+1;values_array[chosen]:=app.value_add(values_array[chosen],unit_value);remaining:=remaining-1;
 END LOOP;
 FOR index_value IN 1..count_value LOOP
 balances:=balances||jsonb_build_array(app.value_json(values_array[index_value]));
 IF shares[index_value]>0 THEN allocations:=allocations||jsonb_build_array(jsonb_build_object('recipientSeatId',p_winners[index_value],'quantity',shares[index_value]::text)); END IF;
 END LOOP;
 RETURN jsonb_build_object('allocations',allocations,'balances',balances);
END $$;
REVOKE ALL ON FUNCTION app.split_ante_asset(uuid[],jsonb,numeric,numeric,numeric,text,uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
