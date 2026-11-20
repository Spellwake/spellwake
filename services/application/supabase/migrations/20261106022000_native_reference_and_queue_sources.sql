BEGIN;

-- These helpers validate namespace/source provenance only. Retained native bytes,
-- kernel legality and player authority remain separate reviewed boundaries.
CREATE FUNCTION app.native_u64_is_canonical(p_value text) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path='' AS $$
 SELECT coalesce(p_value ~ '^(0|[1-9][0-9]{0,19})$'
  AND (length(p_value)<20 OR p_value<='18446744073709551615'),false)
$$;

CREATE FUNCTION app.native_artifact_ref_is_valid(p_ref text,p_game uuid,p_context uuid,p_role text) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE SET search_path='' AS $$
DECLARE parts text[]; maximum_size integer;
BEGIN
 IF p_ref IS NULL OR octet_length(p_ref)>256 OR p_game IS NULL OR p_context IS NULL
  OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_game,p_context)
  OR p_role IS NULL OR p_role NOT IN('initial','input','state','facts','event','view','replay') THEN RETURN false;END IF;
 parts:=string_to_array(p_ref,'/');
 IF cardinality(parts)<>6 OR parts[1]<>'native-artifact-v1'
  OR parts[2]<>p_game::text OR parts[3]<>p_context::text OR parts[4]<>p_role
  OR parts[5] !~ '^[1-9][0-9]{0,7}$' OR parts[6] !~ '^[0-9a-f]{64}\.json$' THEN RETURN false;END IF;
 maximum_size:=CASE p_role WHEN 'initial' THEN 1048576 WHEN 'input' THEN 1048576 WHEN 'state' THEN 32768 ELSE 33554432 END;
 RETURN parts[5]::integer<=maximum_size;
END $$;

-- Enumerate immutable intent identities, not expanded choice bytes. Copied profile
-- configuration can exceed both the original intake envelope and a single page.
CREATE FUNCTION app.read_native_queue_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE intent_id uuid;receipt app.command_receipt;entry app.queue_entry;
 items jsonb:='[]';body jsonb;response jsonb;cursor_id uuid;more boolean:=false;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native queue page';END IF;
 FOR intent_id IN SELECT DISTINCT q.intent_receipt_id FROM app.queue_entry q
  JOIN app.account a ON a.id=q.account_id AND a.state='active'
  WHERE q.state='queued' AND q.intent_receipt_id IS NOT NULL
   AND(p_after IS NULL OR q.intent_receipt_id>p_after)
  ORDER BY q.intent_receipt_id LIMIT p_limit+1 LOOP
  IF jsonb_array_length(items)=p_limit THEN more:=true;EXIT;END IF;
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE id=intent_id;
  FOR entry IN SELECT * FROM app.queue_entry WHERE intent_receipt_id=intent_id ORDER BY alternative_index LOOP
   PERFORM app.check_queue_intent(entry.id);
  END LOOP;
  IF receipt.state<>'accepted' OR receipt.scope<>'queue_intent_v1'
   OR receipt.normalized_source->>'version' IS DISTINCT FROM 'queue-intent-v1'
   OR receipt.account_id IS NULL OR receipt.principal_key IS DISTINCT FROM 'human:'||receipt.account_id::text
   OR receipt.target_ref IS DISTINCT FROM receipt.account_id::text THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid accepted native queue source';END IF;
  SELECT jsonb_build_object('id',receipt.id,'accountId',receipt.account_id,
   'mode',receipt.normalized_source->>'mode','competitive',receipt.normalized_source->'competitive',
   'alternatives',jsonb_agg(jsonb_build_object('entryId',q.id,'alternativeIndex',q.alternative_index::text,
    'deckRevisionId',q.deck_revision_id,'formatId',q.format_id,'preferencesRef',q.preferences_ref) ORDER BY q.alternative_index),
   'sourceHash',encode(sha256(convert_to(receipt.normalized_source::text,'UTF8')),'hex')) INTO body
   FROM app.queue_entry q WHERE q.intent_receipt_id=intent_id AND q.state='queued';
  response:=jsonb_build_object('items',items||jsonb_build_array(body),'nextAfter',intent_id);
  IF octet_length(response::text)>1048576 THEN
   IF jsonb_array_length(items)=0 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native queue summary exceeds page budget';END IF;
   more:=true;EXIT;
  END IF;
  items:=items||jsonb_build_array(body);cursor_id:=intent_id;
 END LOOP;
 RETURN jsonb_build_object('items',items,'nextAfter',CASE WHEN more THEN cursor_id END);
END $$;

-- Whole accepted source bytes are immutable across pages, including canceled
-- alternatives. Byte paging preserves legal padded strings and UTF8 boundaries.
CREATE FUNCTION app.read_native_queue_intent_source(p_intent uuid,p_after_offset numeric,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;entry app.queue_entry;source_bytes bytea;
 total_size integer;start_offset integer;position_offset integer;piece bytea;
 items jsonb:='[]';item jsonb;response jsonb;source_hash text;count_items integer:=0;
BEGIN
 IF p_intent IS NULL OR p_intent='00000000-0000-0000-0000-000000000000'::uuid
  OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 8
  OR(p_after_offset IS NOT NULL AND p_after_offset::text !~ '^(0|[1-9][0-9]*)$') THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native queue source page';END IF;
 SELECT * INTO receipt FROM app.command_receipt WHERE id=p_intent AND scope='queue_intent_v1' AND state='accepted';
 IF receipt.id IS NULL OR receipt.normalized_source IS NULL THEN
  RAISE EXCEPTION USING ERRCODE='P0002',MESSAGE='native queue source unavailable';END IF;
 IF receipt.normalized_source->>'version' IS DISTINCT FROM 'queue-intent-v1'
  OR receipt.account_id IS NULL OR receipt.principal_key IS DISTINCT FROM 'human:'||receipt.account_id::text
  OR receipt.target_ref IS DISTINCT FROM receipt.account_id::text
  OR jsonb_typeof(receipt.normalized_source->'alternatives') IS DISTINCT FROM 'array'
  OR jsonb_array_length(receipt.normalized_source->'alternatives') NOT BETWEEN 1 AND 32
  OR(SELECT count(*) FROM app.queue_entry WHERE intent_receipt_id=receipt.id)<>jsonb_array_length(receipt.normalized_source->'alternatives') THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='invalid accepted native queue source';END IF;
 FOR entry IN SELECT * FROM app.queue_entry WHERE intent_receipt_id=receipt.id ORDER BY alternative_index LOOP
  PERFORM app.check_queue_intent(entry.id);
 END LOOP;
 source_bytes:=convert_to(receipt.normalized_source::text,'UTF8');total_size:=octet_length(source_bytes);
 IF coalesce(p_after_offset,0)>total_size THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native queue byte offset outside source';END IF;
 start_offset:=coalesce(p_after_offset,0)::integer;position_offset:=start_offset;
 source_hash:=encode(sha256(source_bytes),'hex');
 WHILE position_offset<total_size AND count_items<p_limit LOOP
  piece:=substring(source_bytes FROM position_offset+1 FOR least(65536,total_size-position_offset));
  item:=jsonb_build_object('offset',position_offset::text,'byteLength',octet_length(piece)::text,
   'base64',replace(encode(piece,'base64'),chr(10),''));
  response:=jsonb_build_object('intentId',receipt.id,'accountId',receipt.account_id,
   'mode',receipt.normalized_source->>'mode','competitive',receipt.normalized_source->'competitive',
   'sourceHash',source_hash,'totalBytes',total_size::text,'afterOffset',start_offset::text,
   'items',items||jsonb_build_array(item),'nextOffset',(position_offset+octet_length(piece))::text);
  IF octet_length(response::text)>1048576 THEN
   IF count_items=0 THEN RAISE EXCEPTION USING ERRCODE='54000',MESSAGE='native queue byte chunk exceeds page budget';END IF;
   EXIT;
  END IF;
  items:=items||jsonb_build_array(item);position_offset:=position_offset+octet_length(piece);count_items:=count_items+1;
 END LOOP;
 RETURN jsonb_build_object('intentId',receipt.id,'accountId',receipt.account_id,
  'mode',receipt.normalized_source->>'mode','competitive',receipt.normalized_source->'competitive',
  'sourceHash',source_hash,'totalBytes',total_size::text,'afterOffset',start_offset::text,'items',items,
  'nextOffset',CASE WHEN position_offset<total_size THEN position_offset::text END);
END $$;

-- Build the complete database tier before the later access/grant stage. No
-- browser, controller or elevated runtime receives these capabilities yet.
REVOKE ALL ON FUNCTION app.native_u64_is_canonical(text),app.native_artifact_ref_is_valid(text,uuid,uuid,text),
 app.read_native_queue_work(uuid,integer),app.read_native_queue_intent_source(uuid,numeric,integer)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
COMMIT;
