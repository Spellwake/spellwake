BEGIN;

-- The stronger principal/scope/request index now binds target changes; the old index is redundant.
ALTER TABLE app.command_receipt DROP CONSTRAINT command_receipt_principal_key_scope_target_ref_request_ref_key;

-- Private explicit-time policy makes elapsed cutoffs testable. Public service wrapper owns the clock.
CREATE FUNCTION app.contact_allowed_at(p_actor uuid,p_other uuid,p_evaluated timestamptz) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path='' AS $$
 WITH evaluated AS (SELECT p_evaluated AS at)
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

CREATE OR REPLACE FUNCTION app.contact_allowed(p_actor uuid,p_other uuid) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path='' AS $$
 SELECT app.contact_allowed_at(p_actor,p_other,clock_timestamp())
$$;

COMMIT;
