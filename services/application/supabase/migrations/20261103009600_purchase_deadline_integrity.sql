BEGIN;

-- CHECK accepts UNKNOWN. Require both members explicitly when establishing this tuple.
ALTER TABLE app.purchase DROP CONSTRAINT purchase_check1;
ALTER TABLE app.purchase ADD CONSTRAINT purchase_creation_deadline_tuple CHECK (
 (first_create_attempt_at IS NULL AND create_deadline_at IS NULL)
 OR (first_create_attempt_at IS NOT NULL AND create_deadline_at IS NOT NULL
  AND isfinite(first_create_attempt_at) AND isfinite(create_deadline_at)
  AND create_deadline_at=first_create_attempt_at+interval '23 hours')
);

COMMIT;
