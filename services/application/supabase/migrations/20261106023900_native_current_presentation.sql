BEGIN;

-- Presentation is a coherent current observation, separate from immutable native
-- card state. Existing volatile Auth checks do not become a lifetime permission.
CREATE FUNCTION app.read_native_human_presentation(
 p_subject uuid,p_session uuid,p_game uuid,p_context uuid,p_free jsonb
) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE access jsonb;matched app.match;seat app.match_seat;revision app.deck_revision;
 library app.deck;owner_id uuid;lifecycle text;selection jsonb;items jsonb:='[]';
 slot_kind text;slot_key text;free_id uuid;
 unauthorized constant jsonb:='{"status":"fail","data":{"session":"unauthorized"}}'::jsonb;
 unavailable constant jsonb:='{"status":"fail","data":{"game":"unavailable"}}'::jsonb;
 no_presentation constant jsonb:='{"status":"fail","data":{"presentation":"unavailable"}}'::jsonb;
BEGIN
 IF p_subject IS NULL OR p_session IS NULL OR p_game IS NULL OR p_context IS NULL
 OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_subject,p_session,p_game,p_context) THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native presentation identity';END IF;
 -- Trusted startup configuration only. The public request cannot choose a free
 -- replacement, impersonate an owner or supply an entitlement assertion.
 IF NOT app.valid_appearance_selection(p_free)
 OR p_free->'playmatId' IS DISTINCT FROM 'null'::jsonb THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native free presentation';END IF;
 FOREACH slot_kind IN ARRAY ARRAY['back','sleeve','dice'] LOOP
  slot_key:=slot_kind||'Id';free_id:=(p_free->>slot_key)::uuid;
  IF p_free->>slot_key IS DISTINCT FROM free_id::text OR NOT EXISTS(
   SELECT 1 FROM app.cosmetic c WHERE c.id=free_id AND c.kind=slot_kind AND c.is_free AND c.state='active') THEN
   RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='native free presentation is not configured';END IF;
 END LOOP;
 access:=app.read_native_human_access(p_subject,p_session,p_game,p_context);
 IF access=unauthorized OR access=unavailable THEN RETURN access;END IF;
 IF access->>'status' IS DISTINCT FROM 'success' THEN
  RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='native presentation access is inconsistent';END IF;
 SELECT * INTO matched FROM app.match WHERE id=(access->'data'->'source'->>'matchId')::uuid;
 IF matched.id IS NULL THEN RETURN no_presentation;END IF;
 FOR seat IN SELECT * FROM app.match_seat WHERE match_id=matched.id ORDER BY seat_index LOOP
  SELECT * INTO revision FROM app.deck_revision WHERE id=seat.source_revision_id;
  SELECT * INTO library FROM app.deck WHERE id=revision.deck_id;
  IF seat.deck_accepted_at IS NULL OR revision.id IS NULL OR revision.sealed_at IS NULL
   OR library.id IS NULL OR library.mode IS DISTINCT FROM matched.mode
   OR library.format_id IS DISTINCT FROM matched.format_id THEN RETURN no_presentation;END IF;
  IF seat.controller_kind='human' THEN
   owner_id:=seat.account_id;
   IF owner_id IS NULL OR seat.sponsor_id IS NOT NULL OR library.source_kind<>'account'
    OR library.account_id IS DISTINCT FROM owner_id THEN RETURN no_presentation;END IF;
  ELSIF seat.controller_kind='ai' THEN
   owner_id:=seat.sponsor_id;
   IF owner_id IS NULL OR seat.account_id IS NOT NULL THEN RETURN no_presentation;END IF;
   IF library.source_kind='account' THEN
    IF library.account_id IS DISTINCT FROM owner_id THEN RETURN no_presentation;END IF;
   ELSIF library.source_kind='system_pool' THEN
    IF library.account_id IS NOT NULL OR library.system_pool_ref IS NULL
     OR library.source_manifest_ref IS NULL THEN RETURN no_presentation;END IF;
   ELSE RETURN no_presentation;END IF;
  ELSE RETURN no_presentation;END IF;
  SELECT state INTO lifecycle FROM app.account WHERE id=owner_id;
  IF lifecycle IS NULL OR lifecycle NOT IN('active','deletion_pending','deleted') THEN RETURN no_presentation;END IF;
  IF lifecycle='deleted' THEN
   -- A historical owner has no current paid access. This uses the same validated
   -- configured free slots as the application, not the viewer's grants.
   selection:=p_free;
  ELSE
   selection:=app.resolve_account_appearance(owner_id,library.back_id,library.sleeve_id,library.dice_id,library.playmat_id);
  END IF;
  items:=items||jsonb_build_array(jsonb_build_object('seatId',seat.id,'appearance',selection));
 END LOOP;
 IF jsonb_array_length(items) NOT BETWEEN 2 AND 6 THEN RETURN no_presentation;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object(
  'version','native-presentation-v1','access',access->'data',
  'presentation',jsonb_build_object('seats',items)));
END $$;
REVOKE ALL ON FUNCTION app.read_native_human_presentation(uuid,uuid,uuid,uuid,jsonb)
 FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_native_human_presentation(uuid,uuid,uuid,uuid,jsonb) TO service_role;
COMMIT;
