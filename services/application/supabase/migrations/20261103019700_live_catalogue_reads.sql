BEGIN;
ALTER POLICY active_cosmetic_read ON app.cosmetic USING(app.current_account_id() IS NOT NULL AND state='active');
ALTER POLICY tier_catalogue_read ON app.supporter_tier USING(app.current_account_id() IS NOT NULL);
ALTER POLICY tier_cosmetic_catalogue_read ON app.tier_cosmetic USING(app.current_account_id() IS NOT NULL);
ALTER POLICY format_catalogue_read ON app.format USING(app.current_account_id() IS NOT NULL);
ALTER POLICY definition_read ON app.asset_definition USING(app.current_account_id() IS NOT NULL);
COMMIT;
