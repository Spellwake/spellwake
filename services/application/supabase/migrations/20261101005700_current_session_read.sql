BEGIN;

-- One canonical request-claims resolver for RLS and current-session reads.
-- This helper never creates a binding or grants authority from client metadata.
CREATE FUNCTION app.read_current_auth_context() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE claims jsonb; subject uuid; session_id uuid; authority jsonb;
BEGIN
 claims:=nullif(current_setting('request.jwt.claims',true),'')::jsonb;
 IF jsonb_typeof(claims) IS DISTINCT FROM 'object' OR claims->>'role' IS DISTINCT FROM 'authenticated'
 OR (claims->'aud' IS DISTINCT FROM to_jsonb('authenticated'::text)
  AND claims->'aud' IS DISTINCT FROM jsonb_build_array('authenticated'))
 OR jsonb_typeof(claims->'exp') IS DISTINCT FROM 'number' THEN RETURN NULL; END IF;
 IF (claims->>'exp')::numeric<=extract(epoch FROM statement_timestamp()) THEN RETURN NULL; END IF;
 subject:=(claims->>'sub')::uuid; session_id:=(claims->>'session_id')::uuid;
 authority:=app.read_auth_session_authority(subject,session_id);
 IF authority IS NULL OR claims->'spellwake_auth' IS DISTINCT FROM authority THEN RETURN NULL; END IF;
 RETURN jsonb_build_object('subjectId',subject::text,'sessionId',session_id::text,'method',authority->>'method');
EXCEPTION WHEN invalid_text_representation OR numeric_value_out_of_range OR check_violation THEN
 RETURN NULL;
END $$;
REVOKE EXECUTE ON FUNCTION app.read_current_auth_context() FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

CREATE OR REPLACE FUNCTION app.current_account_id() RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; result uuid;
BEGIN
 context:=app.read_current_auth_context();
 IF context IS NULL THEN RETURN NULL; END IF;
 SELECT id INTO result FROM app.account WHERE auth_subject=(context->>'subjectId')::uuid
  AND state IN ('active','deletion_pending') AND sign_in_method=context->>'method';
 RETURN result;
END $$;

-- No caller-supplied identifiers. PostgREST first verifies the original bearer.
CREATE FUNCTION app.read_current_session() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE context jsonb; player app.account; state text;
BEGIN
 context:=app.read_current_auth_context();
 IF context IS NULL THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
 END IF;
 -- Inspect existence before filtering: an incompatible account is not new setup.
 SELECT * INTO player FROM app.account WHERE auth_subject=(context->>'subjectId')::uuid;
 IF FOUND THEN
  IF player.state NOT IN ('active','deletion_pending') OR player.sign_in_method IS DISTINCT FROM context->>'method' THEN
   RETURN jsonb_build_object('status','fail','data',jsonb_build_object('session','unauthorized'));
  END IF;
  state:=player.state;
 ELSE
  state:='needs_registration';
 END IF;
 RETURN jsonb_build_object('status','success','data',context||jsonb_build_object('state',state,'accountId',player.id));
END $$;
REVOKE EXECUTE ON FUNCTION app.read_current_session() FROM PUBLIC,anon,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_current_session() TO authenticated;

COMMIT;
