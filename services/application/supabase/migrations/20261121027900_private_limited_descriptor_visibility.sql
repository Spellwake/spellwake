BEGIN;

-- Check the exact predecessor before extending the two signed discovery paths.
DO $descriptor_predecessor$
BEGIN
 IF NOT EXISTS (
   SELECT 1 FROM pg_catalog.pg_policy policy JOIN pg_catalog.pg_class source ON source.oid=policy.polrelid
   WHERE policy.polrelid='app.format'::regclass AND policy.polname='format_catalogue_read'
     AND policy.polcmd='r' AND policy.polpermissive AND source.relrowsecurity
     AND policy.polroles=ARRAY['authenticated'::regrole::oid] AND policy.polwithcheck IS NULL
     AND pg_catalog.pg_get_expr(policy.polqual,policy.polrelid)='(app.current_account_id() IS NOT NULL)'
 ) OR EXISTS (
   SELECT 1 FROM (VALUES
    ('app.read_table_profiles(uuid,integer)', '941969e22094e768af89c6efe3cdf080129f91af9ad42a40a957498bb8385a13'),
    ('app.read_table_profile_configurations(uuid,integer,integer)', '28e03ad210beadd1a5175b9ece23a0fb11a28fbdd8fa14d3d0d772338235b1c3')
   ) AS expected(signature,body_hash)
   LEFT JOIN pg_catalog.pg_proc installed ON installed.oid=pg_catalog.to_regprocedure(expected.signature)
   WHERE pg_catalog.encode(pg_catalog.sha256(pg_catalog.convert_to(installed.prosrc,'UTF8')),'hex')
         IS DISTINCT FROM expected.body_hash OR installed.prosecdef IS DISTINCT FROM true
 ) THEN
   RAISE EXCEPTION USING ERRCODE='55000',MESSAGE='private Limited descriptor predecessor differs from reviewed hosted discovery';
 END IF;
END
$descriptor_predecessor$;

CREATE TABLE app.limited_private_profile (
 profile_id uuid PRIMARY KEY REFERENCES app.table_profile_revision(id),
 release_id uuid NOT NULL REFERENCES app.qualified_release(id)
);
ALTER TABLE app.limited_private_profile ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.limited_private_profile FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- Limited descriptors are visible only while the caller has a grant for a source of that format.
CREATE FUNCTION app.private_limited_format_visible(p_format uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 SELECT EXISTS (
   SELECT 1 FROM app.limited_draft_source source
   JOIN app.limited_pilot_member member ON member.release_id=source.release_id
   WHERE source.format_id=p_format AND member.account_id=app.current_account_id()
 )
$$;
REVOKE ALL ON FUNCTION app.private_limited_format_visible(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.private_limited_format_visible(uuid) TO authenticated;
ALTER POLICY format_catalogue_read ON app.format USING (
 app.current_account_id() IS NOT NULL
 AND (key NOT IN ('draft','cube') OR app.private_limited_format_visible(id))
);

-- Existing profiles retain their visibility unless an operator binds them to a private release.
CREATE FUNCTION app.private_limited_profile_visible(p_profile uuid,p_actor uuid) RETURNS boolean
LANGUAGE sql STABLE SET search_path='' AS $$
 SELECT NOT EXISTS (
   SELECT 1 FROM app.limited_private_profile profile
   WHERE profile.profile_id=p_profile AND NOT EXISTS (
     SELECT 1 FROM app.limited_pilot_member member
     WHERE member.release_id=profile.release_id AND member.account_id=p_actor
   )
 )
$$;
REVOKE ALL ON FUNCTION app.private_limited_profile_visible(uuid,uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.read_table_profiles(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT id,jsonb_build_object('id',id,'key',key,'minimumSeats',minimum_seats::text,'seatCapacity',seat_capacity::text,'hasMatchmaking',matchmaking_config IS NOT NULL) body FROM app.table_profile_revision WHERE admission_qualification_ref IS NOT NULL AND app.private_limited_profile_visible(id,actor) AND(p_after IS NULL OR id>p_after) ORDER BY id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'id')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'items',items,'nextCursor',cursor_id));
END $$;

CREATE OR REPLACE FUNCTION app.read_table_profile_configurations(p_profile uuid,p_after integer,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;configuration jsonb;items jsonb;cursor_index text;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_profile IS NULL OR p_profile='00000000-0000-0000-0000-000000000000'::uuid OR p_after NOT BETWEEN 0 AND 511 OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT matchmaking_config INTO configuration FROM app.table_profile_revision WHERE id=p_profile AND admission_qualification_ref IS NOT NULL AND matchmaking_config IS NOT NULL AND app.private_limited_profile_visible(p_profile,actor);
 IF configuration IS NULL THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('profile','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY ordinal),'[]'::jsonb) INTO items FROM(
 SELECT ordinality-1 ordinal,jsonb_build_object('index',(ordinality-1)::text,'key',value->'key','formatId',value->'formatId','seatCount',value->'seatCount','structure',value->'structure') body FROM jsonb_array_elements(configuration->'configurations') WITH ORDINALITY source(value,ordinality) WHERE p_after IS NULL OR ordinality-1>p_after ORDER BY ordinality LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_index:=items->-1->>'index';END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'profileId',p_profile,'items',items,'nextIndex',cursor_index));
END $$;

COMMIT;
