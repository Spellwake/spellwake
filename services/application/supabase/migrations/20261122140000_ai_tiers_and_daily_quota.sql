BEGIN;

-- Normal AI is unlimited. A hosted match with Advanced AI consumes one match
-- from the host's daily allowance (Free 3, Initiate 3, Adept 10, Mage 20,
-- Archmage 60, Benefactor unlimited). The private cap counts Advanced seats.
ALTER TABLE app.room_slot
  ADD COLUMN ai_tier text NOT NULL DEFAULT 'normal'
  CHECK (ai_tier IN ('normal', 'advanced'));

ALTER TABLE app.match_seat
  ADD COLUMN ai_tier text NOT NULL DEFAULT 'normal'
  CHECK (ai_tier IN ('normal', 'advanced'));

ALTER TABLE app.ai_runner_binding
  ADD COLUMN ai_tier text NOT NULL DEFAULT 'normal'
  CHECK (ai_tier IN ('normal', 'advanced'));

-- Daily Advanced AI usage bucket per hosting account (UTC day boundary).
CREATE TABLE app.ai_usage_daily (
  account_id uuid NOT NULL REFERENCES app.account(id) ON DELETE RESTRICT,
  usage_day date NOT NULL,
  used_matches integer NOT NULL DEFAULT 0 CHECK (used_matches >= 0),
  used_advanced_seats integer NOT NULL DEFAULT 0
    CHECK (used_advanced_seats BETWEEN used_matches AND 250),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (account_id, usage_day)
);
ALTER TABLE app.ai_usage_daily ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.ai_usage_daily FORCE ROW LEVEL SECURITY;
REVOKE ALL ON app.ai_usage_daily FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Per-match Advanced AI usage reservation ledger (idempotent per match,
-- refundable if Spellwake fails before opening).
CREATE TABLE app.ai_usage_reservation (
  match_id uuid PRIMARY KEY REFERENCES app.match(id) ON DELETE RESTRICT,
  account_id uuid NOT NULL REFERENCES app.account(id) ON DELETE RESTRICT,
  usage_day date NOT NULL,
  advanced_seats integer NOT NULL CHECK (advanced_seats BETWEEN 1 AND 6),
  state text NOT NULL CHECK (state IN ('reserved', 'consumed', 'restored')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX ai_usage_reservation_account_day_idx
  ON app.ai_usage_reservation (account_id, usage_day);
ALTER TABLE app.ai_usage_reservation ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.ai_usage_reservation FORCE ROW LEVEL SECURITY;
REVOKE ALL ON app.ai_usage_reservation FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Resolve an account's active Advanced AI daily quota policy.
CREATE FUNCTION app.ai_daily_quota_for_account(p_account uuid)
RETURNS TABLE(daily_limit integer, display_unlimited boolean, hard_cap integer, tier_name text)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  active_tier_id uuid;
  tier_row app.supporter_tier;
BEGIN
  active_tier_id := app.active_supporter_tier(p_account);
  IF active_tier_id IS NOT NULL THEN
    SELECT * INTO tier_row FROM app.supporter_tier WHERE id = active_tier_id;
  END IF;
  IF tier_row.id IS NULL THEN
    RETURN QUERY SELECT 3, false, 250, 'Free'::text;
  ELSIF tier_row.rank >= 5 THEN
    RETURN QUERY SELECT NULL::integer, true, 250, tier_row.name;
  ELSIF tier_row.rank = 4 THEN
    RETURN QUERY SELECT 60, false, 250, tier_row.name;
  ELSIF tier_row.rank = 3 THEN
    RETURN QUERY SELECT 20, false, 250, tier_row.name;
  ELSIF tier_row.rank = 2 THEN
    RETURN QUERY SELECT 10, false, 250, tier_row.name;
  ELSE
    RETURN QUERY SELECT 3, false, 250, tier_row.name;
  END IF;
END $$;
REVOKE ALL ON FUNCTION app.ai_daily_quota_for_account(uuid)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Authenticated read of the caller's daily Advanced AI allowance.
-- For the $50 Benefactor tier, dailyLimit stays null so the private 250 cap
-- is never disclosed as a public numeric allowance.
CREATE FUNCTION app.read_my_ai_quota() RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  policy record;
  utc_day date := (clock_timestamp() AT TIME ZONE 'UTC')::date;
  next_reset timestamptz := ((utc_day + 1)::timestamp AT TIME ZONE 'UTC');
  used_matches integer;
  used_advanced_seats integer;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;

  SELECT * INTO policy FROM app.ai_daily_quota_for_account(actor);
  SELECT u.used_matches, u.used_advanced_seats INTO used_matches, used_advanced_seats
  FROM app.ai_usage_daily u
  WHERE u.account_id = actor AND u.usage_day = utc_day;
  used_matches := COALESCE(used_matches, 0);
  used_advanced_seats := COALESCE(used_advanced_seats, 0);

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'tierName', policy.tier_name,
      'displayUnlimited', policy.display_unlimited,
      'dailyLimit', policy.daily_limit,
      'usedToday', used_matches,
      'remainingToday', CASE
        WHEN policy.display_unlimited AND used_advanced_seats < policy.hard_cap THEN NULL
        WHEN policy.display_unlimited THEN 0
        ELSE GREATEST(0, policy.daily_limit - used_matches)
      END,
      'normalUnlimited', true,
      'resetsAt', to_char(next_reset AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
    )
  );
END $$;
REVOKE ALL ON FUNCTION app.read_my_ai_quota()
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_ai_quota() TO authenticated;

-- Canonical per-seat AI tier mutation (host or AI slot sponsor in lobby/countdown).
CREATE FUNCTION app.set_ai_slot_tier(
  p_actor uuid,
  p_slot uuid,
  p_expected numeric,
  p_tier text,
  p_request uuid
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  receipt app.command_receipt;
  slot app.room_slot;
  lobby app.room;
  original_room uuid;
  original_host uuid;
  original_sponsor uuid;
  actors uuid[];
  changed boolean;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI tier change requires read committed retry';
  END IF;
  IF p_actor IS NULL OR p_slot IS NULL OR p_request IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_actor, p_slot, p_request)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
     OR p_tier IS NULL OR p_tier NOT IN ('normal', 'advanced') THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid AI tier envelope';
  END IF;

  receipt := app.begin_account_command(
    p_actor,
    p_request,
    'ai_tier_set_v1',
    p_slot::text,
    jsonb_build_object('basis', p_expected::text, 'tier', p_tier)
  );
  IF receipt.state <> 'pending' THEN
    RETURN receipt.response_payload;
  END IF;

  SELECT s.room_id, r.host_id, s.sponsor_id
  INTO original_room, original_host, original_sponsor
  FROM app.room_slot s
  JOIN app.room r ON r.id = s.room_id
  WHERE s.id = p_slot;

  SELECT array_agg(DISTINCT a ORDER BY a) INTO actors
  FROM unnest(ARRAY[p_actor, original_host, original_sponsor]) a
  WHERE a IS NOT NULL;
  PERFORM id FROM app.account WHERE id = ANY(actors) ORDER BY id FOR NO KEY UPDATE;
  PERFORM id FROM app.queue_entry WHERE claimed_room_id = original_room ORDER BY id FOR NO KEY UPDATE;

  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot FOR NO KEY UPDATE;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id FOR NO KEY UPDATE;
  IF (slot.room_id, slot.sponsor_id, lobby.host_id) IS DISTINCT FROM (original_room, original_sponsor, original_host) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI tier account scope changed before fence';
  END IF;

  IF EXISTS (SELECT 1 FROM app.account WHERE id = ANY(actors) AND state <> 'active') THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('account', 'unavailable'));
  END IF;
  IF slot.id IS NULL OR slot.retired_at IS NOT NULL OR slot.controller_kind <> 'ai'
     OR lobby.id IS NULL OR lobby.state NOT IN ('lobby', 'countdown')
     OR (p_actor <> lobby.host_id AND p_actor <> slot.sponsor_id) THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('slot', 'stale or unavailable'));
  END IF;
  IF lobby.revision <> p_expected THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('revision', 'conflict'));
  END IF;
  IF lobby.queue_claim_receipt_id IS NOT NULL THEN
    RETURN app.finish_account_command(receipt.id, false, jsonb_build_object('queue', 'cancel the claimed room before changing its roster'));
  END IF;

  changed := slot.ai_tier IS DISTINCT FROM p_tier;
  IF changed THEN
    UPDATE app.room_slot SET ai_tier = p_tier WHERE id = slot.id;
    UPDATE app.ai_runner_binding
    SET ai_tier = p_tier
    WHERE room_slot_id = slot.id AND desired_state = 'running';
    UPDATE app.room_slot
    SET ready = false
    WHERE room_id = lobby.id AND retired_at IS NULL AND controller_kind = 'human';
    UPDATE app.room
    SET state = 'lobby', countdown_at = NULL, revision = revision + 1
    WHERE id = lobby.id
    RETURNING * INTO lobby;
  END IF;

  RETURN app.finish_account_command(
    receipt.id,
    true,
    jsonb_build_object('resourceId', slot.id, 'revision', lobby.revision::text)
  );
END $$;

CREATE FUNCTION app.set_ai_slot_tier_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_slot uuid,
  p_expected numeric,
  p_tier text
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  actor_id uuid;
  original_room uuid;
  original_host uuid;
  original_sponsor uuid;
  slot app.room_slot;
  lobby app.room;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI tier change requires read committed retry';
  END IF;
  PERFORM id FROM auth.users WHERE id = p_subject FOR KEY SHARE;
  SELECT id INTO actor_id FROM app.account WHERE auth_subject = p_subject;
  IF actor_id IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'verified live account required';
  END IF;
  SELECT s.room_id, r.host_id, s.sponsor_id
  INTO original_room, original_host, original_sponsor
  FROM app.room_slot s
  JOIN app.room r ON r.id = s.room_id
  WHERE s.id = p_slot;
  PERFORM id FROM app.account WHERE id IN (actor_id, original_host, original_sponsor) ORDER BY id FOR NO KEY UPDATE;
  player := app.lock_session_account(p_subject, p_session);
  IF player.id IS DISTINCT FROM actor_id THEN
    RAISE EXCEPTION USING ERRCODE = '28000', MESSAGE = 'verified account changed';
  END IF;
  SELECT * INTO slot FROM app.room_slot WHERE id = p_slot;
  SELECT * INTO lobby FROM app.room WHERE id = slot.room_id;
  IF (slot.room_id, slot.sponsor_id, lobby.host_id) IS DISTINCT FROM (original_room, original_sponsor, original_host) THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI tier scope changed before session fence';
  END IF;
  IF p_request IS NULL OR p_slot IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_request, p_slot)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
     OR p_tier IS NULL OR p_tier NOT IN ('normal', 'advanced') THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  RETURN app.set_ai_slot_tier(player.id, p_slot, p_expected, p_tier, p_request);
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
  WHEN check_violation THEN
    IF SQLERRM = 'request identity was used for different command terms' THEN
      RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
    END IF;
    RAISE;
END $$;

CREATE FUNCTION app.read_set_ai_slot_tier_command(
  p_subject uuid,
  p_session uuid,
  p_request uuid,
  p_slot uuid,
  p_expected numeric,
  p_tier text
) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  player app.account;
  receipt app.command_receipt;
BEGIN
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'AI tier lookup requires read committed retry';
  END IF;
  player := app.lock_session_account(p_subject, p_session);
  IF p_request IS NULL OR p_slot IS NULL
     OR '00000000-0000-0000-0000-000000000000'::uuid IN (p_request, p_slot)
     OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
     OR p_tier IS NULL OR p_tier NOT IN ('normal', 'advanced') THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'invalid'));
  END IF;
  SELECT * INTO receipt
  FROM app.command_receipt
  WHERE principal_key = 'human:' || player.id::text
    AND scope = 'ai_tier_set_v1'
    AND request_ref = p_request::text;
  IF FOUND AND (
    receipt.target_ref IS DISTINCT FROM p_slot::text
    OR receipt.payload_hash IS DISTINCT FROM app.auth_json_hash(jsonb_build_object('basis', p_expected::text, 'tier', p_tier))
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('request', 'conflict'));
  END IF;
  RETURN jsonb_build_object('status', 'success', 'data', app.read_account_command(player.id, 'ai_tier_set_v1', p_request));
EXCEPTION
  WHEN invalid_authorization_specification THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('session', 'unauthorized'));
END $$;

REVOKE ALL ON FUNCTION app.set_ai_slot_tier(uuid, uuid, numeric, text, uuid),
  app.set_ai_slot_tier_command(uuid, uuid, uuid, uuid, numeric, text),
  app.read_set_ai_slot_tier_command(uuid, uuid, uuid, uuid, numeric, text)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.set_ai_slot_tier_command(uuid, uuid, uuid, uuid, numeric, text),
  app.read_set_ai_slot_tier_command(uuid, uuid, uuid, uuid, numeric, text) TO service_role;

-- Refund reserved Advanced AI usage when Spellwake fails before opening.
CREATE FUNCTION app.restore_ai_usage_reservation(p_match uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  res app.ai_usage_reservation;
BEGIN
  IF p_match IS NULL OR p_match = '00000000-0000-0000-0000-000000000000'::uuid THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid match identifier';
  END IF;
  SELECT * INTO res FROM app.ai_usage_reservation WHERE match_id = p_match FOR UPDATE;
  IF NOT FOUND THEN
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('restored', false));
  END IF;
  IF res.state = 'restored' THEN
    RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('restored', true));
  END IF;
  IF res.state <> 'reserved' THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('reservation', 'already consumed'));
  END IF;
  UPDATE app.ai_usage_daily
  SET used_matches = used_matches - 1,
      used_advanced_seats = used_advanced_seats - res.advanced_seats,
      updated_at = clock_timestamp()
  WHERE account_id = res.account_id AND usage_day = res.usage_day;
  UPDATE app.ai_usage_reservation
  SET state = 'restored', updated_at = clock_timestamp()
  WHERE match_id = p_match;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('restored', true, 'count', 1));
END $$;
REVOKE ALL ON FUNCTION app.restore_ai_usage_reservation(uuid)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.restore_ai_usage_reservation(uuid) TO service_role;

-- Update start_room_match (from 20261103018500_selected_clock_bank.sql) to:
-- 1) Atomically verify and reserve Advanced AI daily quota for the host account.
-- 2) Propagate room_slot.ai_tier to match_seat.ai_tier and ai_runner_binding.ai_tier.
CREATE OR REPLACE FUNCTION app.start_room_match(p_room uuid,p_expected numeric,p_release uuid,p_legality uuid,p_qualification text,p_inputs jsonb,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; lobby app.room; actors uuid[]; fingerprint text; reserved_match uuid; reserved_game uuid;
 item jsonb; slot app.room_slot; seat_id uuid; deck_inputs jsonb:='[]'; accepted jsonb; failure jsonb; rejected boolean:=false; accepted_at timestamptz;
 advanced_ai_count integer:=0; quota_policy record; quota_day date; current_matches integer; current_advanced_seats integer;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room start requires read committed retry'; END IF;
 IF p_room IS NULL OR p_request IS NULL OR p_request='00000000-0000-0000-0000-000000000000'::uuid OR p_expected IS NULL OR p_expected::text !~ '^(0|[1-9][0-9]{0,77})$'
 OR p_release IS NULL OR p_legality IS NULL OR p_qualification IS NULL OR length(btrim(p_qualification)) NOT BETWEEN 1 AND 2048
 OR jsonb_typeof(p_inputs) IS DISTINCT FROM 'array' OR octet_length(p_inputs::text)>1048576 THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room start envelope'; END IF;
 fingerprint:=encode(sha256(convert_to(jsonb_build_object('basis',p_expected::text,'release',p_release,'legality',p_legality,'qualification',p_qualification,'inputs',p_inputs)::text,'UTF8')),'hex');
 INSERT INTO app.command_receipt(principal_key,request_ref,scope,target_ref,payload_hash,state,result_ref)
 VALUES('room-start:'||p_room::text,p_request::text,'room_start_v1',p_room::text,fingerprint,'pending','') ON CONFLICT DO NOTHING RETURNING * INTO receipt;
 IF receipt.id IS NULL THEN
  SELECT * INTO STRICT receipt FROM app.command_receipt WHERE principal_key='room-start:'||p_room::text AND scope='room_start_v1' AND request_ref=p_request::text FOR UPDATE;
  IF receipt.payload_hash<>fingerprint OR receipt.target_ref<>p_room::text THEN RAISE EXCEPTION USING ERRCODE='23514',MESSAGE='room start request terms conflict'; END IF;
  IF receipt.state='pending' THEN RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room start disposition pending'; END IF;
  RETURN receipt.response_payload;
 END IF;
 SELECT array_agg(DISTINCT actor ORDER BY actor) INTO actors FROM(
  SELECT account_id actor FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND account_id IS NOT NULL
  UNION SELECT sponsor_id FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND sponsor_id IS NOT NULL
  UNION SELECT host_id FROM app.room WHERE id=p_room) roster;
 PERFORM id FROM app.account WHERE id=ANY(actors) ORDER BY id FOR NO KEY UPDATE;
 PERFORM id FROM app.queue_entry WHERE claimed_room_id=p_room ORDER BY id FOR NO KEY UPDATE;
 SELECT * INTO lobby FROM app.room WHERE id=p_room FOR NO KEY UPDATE;
 IF lobby.id IS NULL OR lobby.state<>'countdown' OR lobby.revision<>p_expected OR lobby.countdown_at IS NULL OR lobby.countdown_at>clock_timestamp()
 OR EXISTS(SELECT 1 FROM app.match WHERE room_id=p_room) THEN RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('room','stale, not due or already reserved')); END IF;
 IF EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND ((account_id IS NOT NULL AND NOT(account_id=ANY(coalesce(actors,'{}'::uuid[])))) OR (sponsor_id IS NOT NULL AND NOT(sponsor_id=ANY(coalesce(actors,'{}'::uuid[])))))) THEN
  RAISE EXCEPTION USING ERRCODE='40001',MESSAGE='room roster changed before account fence'; END IF;
 IF EXISTS(SELECT 1 FROM unnest(actors) members(actor_id) WHERE NOT EXISTS(SELECT 1 FROM app.account a WHERE a.id=members.actor_id AND a.state='active')) THEN
  failure:=jsonb_build_object('account','unavailable'); rejected:=true;
 END IF;
 IF NOT rejected AND EXISTS(SELECT 1 FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='ai' AND NOT app.ai_slot_ready(id)) THEN failure:=jsonb_build_object('ai','authenticated current-epoch readiness required'); rejected:=true; END IF;
 IF NOT rejected THEN
  SELECT count(*)::integer INTO advanced_ai_count
  FROM app.room_slot
  WHERE retired_at IS NULL AND room_id=p_room AND controller_kind='ai' AND ai_tier='advanced';
  IF advanced_ai_count > 0 THEN
   quota_day:=(clock_timestamp() AT TIME ZONE 'UTC')::date;
   SELECT * INTO quota_policy FROM app.ai_daily_quota_for_account(lobby.host_id);
   INSERT INTO app.ai_usage_daily(account_id,usage_day)
   VALUES(lobby.host_id,quota_day) ON CONFLICT (account_id,usage_day) DO NOTHING;
   SELECT used_matches,used_advanced_seats INTO STRICT current_matches,current_advanced_seats FROM app.ai_usage_daily
   WHERE account_id=lobby.host_id AND usage_day=quota_day FOR UPDATE;
   IF (quota_policy.daily_limit IS NOT NULL AND current_matches >= quota_policy.daily_limit)
      OR current_advanced_seats + advanced_ai_count > quota_policy.hard_cap THEN
    failure:=jsonb_build_object('aiQuota','daily advanced AI limit reached'); rejected:=true;
   END IF;
  END IF;
 END IF;
 IF NOT rejected THEN
  IF jsonb_array_length(p_inputs)<>(SELECT count(*) FROM app.room_slot WHERE retired_at IS NULL AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai')) THEN
   failure:=jsonb_build_object('inputs','complete occupied roster required'); rejected:=true;
  END IF;
 END IF;
 IF NOT rejected AND lobby.queue_claim_receipt_id IS NOT NULL THEN PERFORM app.check_queue_claim(p_room);END IF;
 IF NOT rejected THEN
  BEGIN
   INSERT INTO app.match(room_id,format_id,profile_id,legality_revision_id,mode,competitive,structure,state,revision)
   VALUES(p_room,lobby.format_id,lobby.profile_id,p_legality,lobby.mode,lobby.competitive,lobby.structure,'admitting',0) RETURNING id INTO reserved_match;
   FOR item IN SELECT * FROM jsonb_array_elements(p_inputs) LOOP
    IF jsonb_typeof(item) IS DISTINCT FROM 'object' OR jsonb_typeof(item->'slotId') IS DISTINCT FROM 'string' OR (item->>'slotId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'revisionId') IS DISTINCT FROM 'string' OR (item->>'revisionId') !~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
     OR jsonb_typeof(item->'snapshotRef') IS DISTINCT FROM 'string' OR length(btrim(item->>'snapshotRef')) NOT BETWEEN 1 AND 2048
     OR EXISTS(SELECT 1 FROM jsonb_object_keys(item) k WHERE k NOT IN ('slotId','revisionId','snapshotRef')) THEN RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid room start seat artifact'; END IF;
    SELECT * INTO slot FROM app.room_slot WHERE retired_at IS NULL AND id=(item->>'slotId')::uuid AND room_id=p_room AND (account_id IS NOT NULL OR controller_kind='ai') FOR NO KEY UPDATE;
    IF slot.id IS NULL OR slot.deck_revision_id IS DISTINCT FROM (item->>'revisionId')::uuid THEN failure:=jsonb_build_object('deck','selected revision changed'); RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='room admission rejected'; END IF;
    INSERT INTO app.match_seat(match_id,account_id,sponsor_id,seat_index,team_ref,controller_kind,ai_tier)
    VALUES(reserved_match,slot.account_id,slot.sponsor_id,slot.seat_index,slot.team_ref,slot.controller_kind,slot.ai_tier) RETURNING id INTO seat_id;
    deck_inputs:=deck_inputs||jsonb_build_array(jsonb_build_object('seatId',seat_id,'revisionId',item->>'revisionId','snapshotRef',item->>'snapshotRef'));
   END LOOP;
   accepted:=app.accept_match_decks(reserved_match,0,p_release,p_qualification,deck_inputs,receipt.id);
   IF accepted->>'status' IS DISTINCT FROM 'success' THEN failure:=accepted->'data'; RAISE EXCEPTION USING ERRCODE='SW001',MESSAGE='room admission rejected'; END IF;
   accepted_at:=clock_timestamp();
   INSERT INTO app.game(match_id,game_index,state,accepted_revision) VALUES(reserved_match,0,'reserved',0) RETURNING id INTO reserved_game;
   INSERT INTO app.match_clock(match_seat_id,remaining_ms,clock_revision,policy_ref,paused,last_accounted_at)
   SELECT seat.id,CASE WHEN lobby.structure='bo3' THEN 1800000 ELSE 0 END,0,CASE WHEN lobby.structure='bo3' THEN 'bo3-overall-bank-v1' ELSE 'bo1-bank-inactive-v1' END,true,accepted_at FROM app.match_seat seat WHERE seat.match_id=reserved_match;
   UPDATE app.match m SET state='ready',started_at=accepted_at,reserved_game_id=reserved_game WHERE m.id=reserved_match;
   PERFORM app.adopt_room_ai(p_room,reserved_match,receipt.id);
   UPDATE app.ai_runner_binding b SET ai_tier=s.ai_tier FROM app.room_slot s WHERE b.room_slot_id=s.id AND b.desired_state='running' AND s.room_id=p_room;
   IF advanced_ai_count > 0 THEN
    UPDATE app.ai_usage_daily SET used_matches=used_matches+1,
        used_advanced_seats=used_advanced_seats+advanced_ai_count,updated_at=accepted_at
    WHERE account_id=lobby.host_id AND usage_day=quota_day;
    INSERT INTO app.ai_usage_reservation(match_id,account_id,usage_day,advanced_seats,state,created_at,updated_at)
    VALUES(reserved_match,lobby.host_id,quota_day,advanced_ai_count,'reserved',accepted_at,accepted_at);
   END IF;
   PERFORM app.ensure_game_conversation(reserved_match);
   INSERT INTO app.durable_handoff(match_id,game_id,source_ref,destination,kind,state) VALUES(reserved_match,reserved_game,reserved_match::text,'game_host','ready_match','pending');
  EXCEPTION WHEN SQLSTATE 'SW001' THEN rejected:=true;
  END;
 END IF;
 IF rejected THEN
  UPDATE app.room_slot SET ready=false WHERE retired_at IS NULL AND room_id=p_room;
  UPDATE app.room SET state='lobby',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
  RETURN app.finish_account_command(receipt.id,false,failure);
 END IF;
 UPDATE app.queue_entry SET state='reserved',reserved_match_id=reserved_match WHERE claimed_room_id=p_room AND state='claimed';
 UPDATE app.room SET state='started',countdown_at=NULL,revision=revision+1 WHERE id=p_room;
 UPDATE app.invitation SET state='canceled' WHERE room_id=p_room AND state='active';
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',reserved_match,'gameId',reserved_game));
END $$;

-- Additive aiTier on read_room_ai_status.
CREATE OR REPLACE FUNCTION app.read_room_ai_status(p_room uuid,p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE actor uuid:=app.current_account_id();authority jsonb;host uuid;items jsonb;cursor_id uuid;
BEGIN
 IF actor IS NULL THEN authority:=app.read_current_session();IF authority->>'status'='fail' THEN RETURN authority;END IF;RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable'));END IF;
 IF p_room IS NULL OR '00000000-0000-0000-0000-000000000000'::uuid IN(p_room,p_after) OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('query','invalid'));END IF;
 SELECT host_id INTO host FROM app.room WHERE id=p_room;
 IF host IS NULL OR(actor<>host AND NOT EXISTS(SELECT 1 FROM app.room_slot WHERE room_id=p_room AND sponsor_id=actor AND controller_kind='ai')) THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('room','unavailable'));END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]'::jsonb) INTO items FROM(
 SELECT s.id,jsonb_build_object('slotId',s.id,'seatIndex',s.seat_index::text,'aiTier',s.ai_tier,'isSponsor',s.sponsor_id=actor,'isRetired',s.retired_at IS NOT NULL,'stopPending',EXISTS(SELECT 1 FROM app.ai_runner_binding historical CROSS JOIN LATERAL app.read_ai_stop_work(historical.id) pending WHERE historical.room_slot_id=s.id),'deckRevisionId',CASE WHEN s.sponsor_id=actor THEN s.deck_revision_id END,'isReady',s.retired_at IS NULL AND app.ai_slot_ready(s.id),'binding',CASE WHEN b.id IS NOT NULL THEN jsonb_build_object('id',b.id,'epoch',b.binding_epoch::text,'desiredState',b.desired_state,'observedState',b.observed_state) END) body FROM app.room_slot s LEFT JOIN LATERAL(SELECT id,binding_epoch,desired_state,observed_state FROM app.ai_runner_binding WHERE room_slot_id=s.id AND desired_state='running') b ON true
 WHERE s.room_id=p_room AND s.controller_kind='ai' AND(actor=host OR s.sponsor_id=actor) AND(p_after IS NULL OR s.id>p_after) ORDER BY s.id LIMIT p_limit) page;
 IF jsonb_array_length(items)=p_limit THEN cursor_id:=(items->-1->>'slotId')::uuid;END IF;
 RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,'roomId',p_room,'items',items,'nextCursor',cursor_id));
END $$;

-- Additive aiTier on read_room_enriched slot rows.
CREATE OR REPLACE FUNCTION app.read_room_enriched(p_room uuid) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  actor uuid := app.current_account_id();
  authority jsonb;
  r app.room;
  fmt app.format;
  slots jsonb;
BEGIN
  IF actor IS NULL THEN
    authority := app.read_current_session();
    IF authority->>'status' = 'fail' THEN RETURN authority; END IF;
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('account', 'unavailable'));
  END IF;
  SELECT * INTO r FROM app.room WHERE id = p_room;
  IF r.id IS NULL OR NOT EXISTS (
    SELECT 1 FROM app.room_slot s WHERE s.room_id = p_room AND s.account_id = actor AND s.retired_at IS NULL
  ) THEN
    RETURN jsonb_build_object('status', 'fail', 'data', jsonb_build_object('room', 'unavailable'));
  END IF;
  SELECT * INTO fmt FROM app.format WHERE id = r.format_id;

  SELECT COALESCE(jsonb_agg(jsonb_build_object(
    'slotId', s.id,
    'seatIndex', s.seat_index,
    'controllerKind', s.controller_kind,
    'aiTier', CASE WHEN s.controller_kind = 'ai' THEN s.ai_tier ELSE NULL END,
    'accountId', s.account_id,
    'username', CASE WHEN a.state = 'deleted' THEN 'Deleted player' ELSE a.username END,
    'supporter', CASE WHEN a.id IS NOT NULL THEN app.active_supporter_tier(a.id) IS NOT NULL ELSE false END,
    'selectedBadge', prof.selected_badge,
    'cardArtProfile', CASE WHEN prof.id IS NOT NULL THEN app.profile_art_snapshot(prof) ELSE NULL END,
    'deckRevisionId', s.deck_revision_id,
    'ready', s.ready,
    'teamRef', s.team_ref
  ) ORDER BY s.seat_index), '[]'::jsonb)
  INTO slots
  FROM app.room_slot s
  LEFT JOIN app.account a ON a.id = s.account_id
  LEFT JOIN app.profile prof ON prof.account_id = a.id
  WHERE s.room_id = p_room AND s.retired_at IS NULL;

  RETURN jsonb_build_object(
    'status', 'success',
    'data', jsonb_build_object(
      'accountId', actor,
      'room', jsonb_build_object(
        'id', r.id,
        'hostId', r.host_id,
        'formatId', r.format_id,
        'formatKey', fmt.key,
        'formatName', fmt.name,
        'profileId', r.profile_id,
        'mode', r.mode,
        'competitive', r.competitive,
        'visibility', r.visibility,
        'state', r.state,
        'revision', r.revision::text,
        'countdownAt', CASE WHEN r.countdown_at IS NOT NULL THEN to_char(r.countdown_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') ELSE NULL END,
        'aiDifficulty', r.ai_difficulty,
        'planechaseEnabled', r.planechase_enabled,
        'rangeOfInfluence', r.range_of_influence
      ),
      'slots', slots
    )
  );
END $$;

-- Additive fields on read_native_ai_binding_work so the Rust AI runtime worker
-- has tier, room configuration, format, mode, and selected deck in one call.
CREATE OR REPLACE FUNCTION app.read_native_ai_binding_work(p_after uuid,p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path='' AS $$
DECLARE rows jsonb;
BEGIN
 IF p_after='00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
  RAISE EXCEPTION USING ERRCODE='22023',MESSAGE='invalid native binding page';END IF;
 SELECT coalesce(jsonb_agg(body ORDER BY id),'[]') INTO rows FROM(
  SELECT b.id,jsonb_build_object('id',b.id,'epoch',b.binding_epoch::text,
   'desiredState',b.desired_state,'observedState',b.observed_state,
   'aiTier',s.ai_tier,'roomId',s.room_id,'slotId',s.id,'matchSeatId',b.match_seat_id,
   'deckRevisionId',s.deck_revision_id,'configurationId',r.configuration_id,
   'formatId',r.format_id,'mode',r.mode,
   'startWork',app.read_ai_start_work(b.id,b.binding_epoch),
   'hasPendingStops',EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)),
   'grant',(SELECT jsonb_build_object('id',g.id,'epoch',g.binding_epoch::text,'secretRef',g.grant_secret_ref,
    'deliveryRevision',g.delivery_revision::text,'deliveryState',g.delivery_state)
    FROM app.controller_grant g WHERE g.binding_id=b.id AND g.binding_epoch=b.binding_epoch
     AND g.match_seat_id IS NOT DISTINCT FROM b.match_seat_id AND g.state='active')) body
  FROM app.ai_runner_binding b JOIN app.room_slot s ON s.id=b.room_slot_id
  JOIN app.room r ON r.id=s.room_id JOIN app.account a ON a.id=s.sponsor_id
  WHERE(p_after IS NULL OR b.id>p_after) AND(
   EXISTS(SELECT 1 FROM app.read_ai_stop_work(b.id)) OR(
    b.bootstrap_receipt_id IS NOT NULL AND b.desired_state='running' AND s.retired_at IS NULL
    AND r.state<>'closed' AND(a.state='active' OR app.ai_sponsor_continuing_match(b.id,a.id))
    AND NOT EXISTS(SELECT 1 FROM app.match WHERE room_id=r.id AND state IN('completed','no_contest'))
    AND NOT app.ai_slot_ready(s.id))) ORDER BY b.id LIMIT p_limit+1) selected;
 RETURN app.native_bounded_work_page(rows,p_limit);
END $$;

-- Service-role discovery of active/reserved games that contain at least one AI seat.
CREATE FUNCTION app.read_native_ai_active_games(p_after uuid, p_limit integer) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE rows jsonb;
BEGIN
  IF p_after = '00000000-0000-0000-0000-000000000000'::uuid OR p_limit IS NULL OR p_limit NOT BETWEEN 1 AND 100 THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid native AI game page';
  END IF;
  SELECT COALESCE(jsonb_agg(body ORDER BY id), '[]'::jsonb) INTO rows FROM (
    SELECT g.id, jsonb_build_object(
      'id', g.id,
      'matchId', g.match_id,
      'gameState', g.state,
      'acceptedRevision', g.accepted_revision::text,
      'contextId', (SELECT c.id FROM app.game_context c WHERE c.game_id = g.id ORDER BY c.id DESC LIMIT 1),
      'aiSeats', COALESCE((
        SELECT jsonb_agg(jsonb_build_object(
          'seatId', ms.id,
          'aiTier', ms.ai_tier,
          'bindingId', b.id,
          'epoch', b.binding_epoch::text,
          'grantId', cg.id,
          'secretRef', cg.grant_secret_ref
        ) ORDER BY ms.seat_index)
        FROM app.match_seat ms
        JOIN app.ai_runner_binding b ON b.match_seat_id = ms.id AND b.desired_state = 'running'
        JOIN app.controller_grant cg
          ON cg.binding_id = b.id
         AND cg.binding_epoch = b.binding_epoch
         AND cg.match_seat_id = ms.id
         AND cg.state = 'active'
         AND cg.delivery_state = 'delivered'
        WHERE ms.match_id = g.match_id AND ms.controller_kind = 'ai'
      ), '[]'::jsonb)
    ) AS body
    FROM app.game g
    JOIN app.match m ON m.id = g.match_id
    WHERE (p_after IS NULL OR g.id > p_after)
      AND g.state IN ('reserved', 'active')
      AND m.state IN ('ready', 'active')
      AND m.availability = 'available'
      AND EXISTS (
        SELECT 1 FROM app.match_seat ms
        WHERE ms.match_id = g.match_id AND ms.controller_kind = 'ai'
      )
    ORDER BY g.id
    LIMIT p_limit + 1
  ) selected;
  RETURN app.native_bounded_work_page(rows, p_limit);
END $$;
REVOKE ALL ON FUNCTION app.read_native_ai_active_games(uuid, integer)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_native_ai_active_games(uuid, integer) TO service_role;

-- Service-role lookup of a sealed system-pool deck revision for Random Deck AI seats.
CREATE FUNCTION app.read_native_ai_default_deck(p_format_id uuid, p_mode text) RETURNS jsonb
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = '' AS $$
DECLARE rev_id uuid;
BEGIN
  IF p_format_id IS NULL OR p_format_id = '00000000-0000-0000-0000-000000000000'::uuid OR p_mode IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '22023', MESSAGE = 'invalid default AI deck query';
  END IF;
  SELECT r.id INTO rev_id
  FROM app.deck_revision r
  JOIN app.deck d ON d.id = r.deck_id
  WHERE d.source_kind = 'system_pool'
    AND d.format_id = p_format_id
    AND d.mode = p_mode
    AND r.sealed_at IS NOT NULL
  ORDER BY r.sealed_at DESC, r.id DESC
  LIMIT 1;
  RETURN jsonb_build_object('status', 'success', 'data', jsonb_build_object('deckRevisionId', rev_id));
END $$;
REVOKE ALL ON FUNCTION app.read_native_ai_default_deck(uuid, text)
  FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_native_ai_default_deck(uuid, text) TO service_role;

COMMIT;
