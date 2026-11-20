BEGIN;
-- Each supervised process registers its own UUID against the shared configured owner. Freeze the result
-- across games in one transaction, so retrying an exit cannot pause a new lease.
CREATE TABLE app.native_host_watch (
 instance_id uuid PRIMARY KEY, owner_ref text NOT NULL, finished boolean NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX native_host_watch_running_owner ON app.native_host_watch(owner_ref) WHERE NOT finished;
ALTER TABLE app.native_host_watch ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON app.native_host_watch FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;

-- All participants in an API host use the configured owner. Only one supervised
-- process may serve that owner until its predecessor's exit has been delivered.
CREATE FUNCTION app.register_native_host_watch(p_instance uuid,p_owner text) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
BEGIN
 IF p_instance IS NULL OR p_instance='00000000-0000-0000-0000-000000000000' OR p_owner IS NULL OR length(btrim(p_owner)) NOT BETWEEN 1 AND 256 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid supervised host identity'; END IF;
 INSERT INTO app.native_host_watch(instance_id,owner_ref) VALUES(p_instance,p_owner) ON CONFLICT DO NOTHING;
 RETURN jsonb_build_object('registered',FOUND);
END $$;
REVOKE ALL ON FUNCTION app.register_native_host_watch(uuid,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.register_native_host_watch(uuid,text) TO service_role;

CREATE FUNCTION app.pause_native_host_exit(p_instance uuid,p_incident_at timestamptz,p_process integer,p_status text)
RETURNS jsonb LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE watch app.native_host_watch; instant timestamptz; receipt app.command_receipt; host app.operational_state;
 reply jsonb; terms jsonb; paused integer:=0; stale integer:=0;
BEGIN
 IF p_instance IS NULL OR p_instance='00000000-0000-0000-0000-000000000000' OR p_process IS NULL OR p_process<1
 OR p_incident_at IS NULL OR NOT isfinite(p_incident_at) OR p_incident_at>clock_timestamp()
 OR p_status IS NULL OR p_status !~ '^(exit:(0|[1-9][0-9]{0,9})|signal:[1-9][0-9]{0,9})$' THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid host exit evidence'; END IF;
 SELECT * INTO watch FROM app.native_host_watch WHERE instance_id=p_instance FOR UPDATE;
 IF NOT FOUND THEN
  -- A child that failed during startup never acquired the owner's work.
  RETURN jsonb_build_object('status','success','data',jsonb_build_object('instanceId',p_instance,'paused',0,'stale',0));
 END IF;
 terms:=jsonb_build_object('instance',p_instance,'incidentAt',to_char(p_incident_at AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),'process',p_process,'status',p_status);
 INSERT INTO app.command_receipt(principal_key,scope,target_ref,request_ref,payload_hash,state,result_ref,normalized_source)
 VALUES('native-host-exit:'||p_instance::text,'native_host_exit_v1',p_instance::text,p_instance::text,app.auth_json_hash(terms),'pending','',terms)
 ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='native-host-exit:'||p_instance::text AND scope='native_host_exit_v1' AND request_ref=p_instance::text FOR UPDATE;
  IF receipt.normalized_source IS DISTINCT FROM terms OR receipt.state<>'accepted' THEN
   RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host exit identity conflict'; END IF;
  paused:=(receipt.response_payload->'data'->>'paused')::integer;
  stale:=(receipt.response_payload->'data'->>'stale')::integer;
 ELSE
  FOR host IN SELECT op.* FROM app.operational_state op JOIN app.game g ON g.id=op.game_id
   WHERE op.owner_instance_ref=watch.owner_ref AND op.availability='available' AND g.state='active' ORDER BY g.match_id,g.id
  LOOP
   -- A command already in flight can be durably accounted after process exit.
   -- Freeze at the latest accepted clock cut; the exit receipt keeps the actual instant.
   PERFORM m.id FROM app.match m JOIN app.game g ON g.match_id=m.id WHERE g.id=host.game_id FOR NO KEY UPDATE OF m;
   PERFORM id FROM app.game WHERE id=host.game_id FOR NO KEY UPDATE;
   PERFORM id FROM app.operational_state WHERE id=host.id FOR NO KEY UPDATE;
   PERFORM c.id FROM app.match_clock c JOIN app.match_seat s ON s.id=c.match_seat_id JOIN app.game g ON g.match_id=s.match_id
     WHERE g.id=host.game_id ORDER BY c.match_seat_id FOR NO KEY UPDATE OF c;
   SELECT greatest(p_incident_at,max(c.last_accounted_at)) INTO instant FROM app.match_clock c
     JOIN app.match_seat s ON s.id=c.match_seat_id JOIN app.game g ON g.match_id=s.match_id WHERE g.id=host.game_id;
   reply:=app.pause_game_incident(host.game_id,watch.owner_ref,host.fence_epoch,host.revision,instant,'server_process',
     'native-host-exit-v1:'||receipt.id::text,p_instance);
   IF reply->>'status'='success' THEN paused:=paused+1;
   ELSIF reply->>'status'='fail' AND reply->'data'='{"basis":"stale or unavailable"}'::jsonb THEN stale:=stale+1;
   ELSE RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='host exit could not freeze an owned game'; END IF;
  END LOOP;
  UPDATE app.native_host_watch SET finished=true WHERE instance_id=p_instance;
  -- Stored command acknowledgments contain string fields. The worker RPC exposes typed counts.
  PERFORM app.finish_account_command(receipt.id,true,jsonb_build_object('instanceId',p_instance,'paused',paused::text,'stale',stale::text));
 END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('instanceId',p_instance,'paused',paused,'stale',stale));
END $$;
REVOKE ALL ON FUNCTION app.pause_native_host_exit(uuid,timestamptz,integer,text) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.pause_native_host_exit(uuid,timestamptz,integer,text) TO service_role;
COMMIT;
