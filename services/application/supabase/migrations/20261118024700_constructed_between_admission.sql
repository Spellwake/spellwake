BEGIN;

-- The writer has locked the match and re-observed the selected revision.
-- Bind this reply to the card policy and FFA roster accepted for that match.
CREATE FUNCTION app.guard_constructed_between_admission() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE
  terms jsonb := NEW.normalized_source->'sourceTerms';
  prepared jsonb := NEW.normalized_source->'prepared';
  accepted jsonb;
  roster jsonb;
  m app.match;
  format_key text;
BEGIN
  SELECT * INTO STRICT m FROM app.match WHERE id=(terms->'match'->>'id')::uuid;
  SELECT f.key INTO STRICT format_key FROM app.format f WHERE f.id=m.format_id FOR SHARE;
  IF prepared->>'qualificationRef' IS DISTINCT FROM
       'native-constructed-between-v1/' || format_key || '/' || (NEW.normalized_source->>'sourceHash')
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game selection lacks bound format'; END IF;
  SELECT r.normalized_source INTO accepted
    FROM app.command_receipt r
   WHERE r.scope='qualified_countdown_intent_v1' AND r.state='accepted'
     AND r.target_ref=m.room_id::text
     AND r.normalized_source->'prepared'->>'qualificationRef'=m.admission_qualification_ref
     AND r.normalized_source->'prepared'->>'releaseId'=m.accepted_release_id::text
     AND r.normalized_source->'constructedPolicy'->>'legalityRevisionId'=m.legality_revision_id::text
     AND r.normalized_source->'constructedPolicy'->>'formatId'=m.format_id::text
     AND r.normalized_source->'constructedPolicy'->>'formatKey'=format_key
     AND r.normalized_source->'sourceTerms'->'room' @> jsonb_build_object(
       'profileId',m.profile_id,'mode',m.mode,'competitive',m.competitive,'structure',m.structure)
   ORDER BY r.id LIMIT 1;
  roster:=accepted->'sourceTerms'->'slots';
  IF roster IS NULL OR jsonb_array_length(roster)<>(SELECT count(*) FROM app.match_seat WHERE match_id=m.id)
     OR EXISTS (
       SELECT 1 FROM app.match_seat s WHERE s.match_id=m.id AND
         (s.team_ref IS NOT NULL OR NOT EXISTS (
           SELECT 1 FROM jsonb_array_elements(roster) slot
            WHERE slot @> jsonb_build_object(
              'seatIndex',s.seat_index::text,'controllerKind',s.controller_kind,
              'accountId',s.account_id,'sponsorId',s.sponsor_id,
              'teamRef',NULL,'selectedRevisionId',s.source_revision_id))))
  THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='between-game selection lacks accepted room policy'; END IF;
  RETURN NEW;
END $$;

REVOKE ALL ON FUNCTION app.guard_constructed_between_admission()
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
CREATE TRIGGER constructed_between_admission
BEFORE INSERT OR UPDATE OF normalized_source ON app.command_receipt
FOR EACH ROW WHEN (NEW.scope='qualified_between_intent_v1'
  AND split_part(NEW.normalized_source->'prepared'->>'qualificationRef','/',1)='native-constructed-between-v1')
EXECUTE FUNCTION app.guard_constructed_between_admission();
COMMIT;
