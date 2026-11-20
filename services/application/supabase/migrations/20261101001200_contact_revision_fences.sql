BEGIN;

ALTER TABLE app.account ADD COLUMN social_revision numeric NOT NULL DEFAULT 0
 CHECK (social_revision>=0 AND social_revision<power(10::numeric,78) AND scale(social_revision)=0);

CREATE FUNCTION app.lock_contact_pair(p_actor uuid,p_other uuid,p_active_peer boolean) RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE account_count bigint;
BEGIN
 IF p_actor IS NULL OR p_other IS NULL OR p_actor=p_other OR p_active_peer IS NULL THEN RETURN false; END IF;
 PERFORM id FROM app.account WHERE id IN (p_actor,p_other) ORDER BY id FOR NO KEY UPDATE;
 SELECT count(*) INTO account_count FROM app.account WHERE id IN (p_actor,p_other);
 IF account_count<>2 OR NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active')
 OR (p_active_peer AND NOT EXISTS(SELECT 1 FROM app.account WHERE id=p_other AND state='active')) THEN RETURN false; END IF;
 PERFORM pg_advisory_xact_lock(hashtextextended('contact:'||least(p_actor,p_other)::text||':'||greatest(p_actor,p_other)::text,0));
 -- Invalidate stale REPEATABLE READ snapshots of friendship/block/participation policy.
 UPDATE app.account SET social_revision=social_revision+1 WHERE id IN (p_actor,p_other);
 RETURN true;
END $$;

CREATE OR REPLACE FUNCTION app.lock_active_contact_accounts(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 SELECT app.lock_contact_pair(p_actor,p_other,true)
$$;

CREATE OR REPLACE FUNCTION app.contact_allowed(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 WITH evaluated AS (SELECT clock_timestamp() AS at)
 SELECT p_actor<>p_other AND EXISTS(SELECT 1 FROM app.account WHERE id=p_actor AND state='active')
 AND EXISTS(SELECT 1 FROM app.account WHERE id=p_other AND state='active')
 AND NOT app.contact_blocked(p_actor,p_other)
 AND (EXISTS(SELECT 1 FROM app.friendship WHERE account_low_id=least(p_actor,p_other)
 AND account_high_id=greatest(p_actor,p_other) AND state='accepted')
 OR EXISTS(SELECT 1 FROM app.game_participation a JOIN app.game_participation b
 ON a.game_id=b.game_id AND a.account_id=p_actor AND b.account_id=p_other CROSS JOIN evaluated e
 WHERE a.real_game AND b.real_game AND greatest(a.completed_at,b.completed_at)
 BETWEEN e.at-interval '336 hours' AND e.at))
$$;

-- Removing a relationship remains possible while blocked or after the peer starts deletion.
CREATE OR REPLACE FUNCTION app.change_friendship(p_actor uuid,p_other uuid,p_action text,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt; v_friendship app.friendship;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'social',p_other::text,
  jsonb_build_object('action',p_action,'other',p_other::text));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_action NOT IN ('request','accept','remove') OR p_action IS NULL THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('action','invalid friend action'));
 END IF;
 IF NOT app.lock_contact_pair(p_actor,p_other,p_action<>'remove')
 OR (p_action<>'remove' AND app.contact_blocked(p_actor,p_other)) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 SELECT * INTO v_friendship FROM app.friendship
 WHERE account_low_id=least(p_actor,p_other) AND account_high_id=greatest(p_actor,p_other) FOR UPDATE;
 IF p_action='request' THEN
  IF v_friendship.id IS NULL THEN
   INSERT INTO app.friendship(account_low_id,account_high_id,requested_by_id,state)
   VALUES(least(p_actor,p_other),greatest(p_actor,p_other),p_actor,'pending') RETURNING * INTO v_friendship;
  ELSIF v_friendship.state='removed' THEN
   UPDATE app.friendship SET requested_by_id=p_actor,state='pending',accepted_at=NULL WHERE id=v_friendship.id RETURNING * INTO v_friendship;
  END IF;
 ELSIF p_action='accept' THEN
  IF v_friendship.id IS NULL OR v_friendship.state<>'pending' OR v_friendship.requested_by_id=p_actor THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('friendship','no incoming request to accept'));
  END IF;
  UPDATE app.friendship SET state='accepted',accepted_at=clock_timestamp() WHERE id=v_friendship.id RETURNING * INTO v_friendship;
 ELSE
  IF v_friendship.id IS NULL THEN
   RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('friendship','friendship unavailable'));
  END IF;
  UPDATE app.friendship SET state='removed',accepted_at=NULL WHERE id=v_friendship.id RETURNING * INTO v_friendship;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',v_friendship.id::text));
END $$;

CREATE OR REPLACE FUNCTION app.change_block(p_actor uuid,p_other uuid,p_blocked boolean,p_request uuid) RETURNS jsonb
LANGUAGE plpgsql SECURITY DEFINER SET search_path='' AS $$
DECLARE receipt app.command_receipt;
BEGIN
 receipt:=app.begin_account_command(p_actor,p_request,'social',p_other::text,
  jsonb_build_object('action','block','other',p_other::text,'blocked',p_blocked));
 IF receipt.state<>'pending' THEN RETURN receipt.response_payload; END IF;
 IF p_blocked IS NULL OR NOT app.lock_contact_pair(p_actor,p_other,false) THEN
  RETURN app.finish_account_command(receipt.id,false,jsonb_build_object('contact','contact unavailable'));
 END IF;
 IF p_blocked THEN
  INSERT INTO app.block(blocker_id,blocked_id,created_at) VALUES(p_actor,p_other,clock_timestamp()) ON CONFLICT DO NOTHING;
 ELSE
  DELETE FROM app.block WHERE blocker_id=p_actor AND blocked_id=p_other;
 END IF;
 RETURN app.finish_account_command(receipt.id,true,jsonb_build_object('resourceId',p_other::text));
END $$;

ALTER TABLE app.message ADD CONSTRAINT message_not_blank CHECK (body ~ '[^[:space:]]');

CREATE TRIGGER message_history BEFORE UPDATE OR DELETE ON app.message
 FOR EACH ROW EXECUTE FUNCTION app.reject_history_change();

COMMIT;
