BEGIN;

-- API/worker roles call source-specific definer functions. Raw posting or
-- operation writes must not become an alternative mint, transfer or burn path.
REVOKE INSERT,UPDATE,DELETE ON app.ledger_operation,app.ledger_posting FROM service_role;

COMMIT;
