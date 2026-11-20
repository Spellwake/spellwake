-- Resolve an accepted printing and an existing fresh daily source, without creating source evidence.
BEGIN;
CREATE FUNCTION app.read_my_prediction_probe_basis(p_printing uuid) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path='' SET DateStyle='ISO, YMD' SET TimeZone='UTC' AS $$
DECLARE actor uuid:=app.current_account_id(); authority jsonb; printing app.printing; external_id uuid;
 current_slot date:=((clock_timestamp() AT TIME ZONE 'UTC'-interval '12 hours')::date); candidate record; manifest jsonb;
BEGIN
 IF actor IS NULL THEN
  authority:=app.read_current_session(); IF authority->>'status'='fail' THEN RETURN authority; END IF;
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 IF NOT EXISTS(SELECT 1 FROM app.account WHERE id=actor AND state='active') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('account','unavailable')); END IF;
 SELECT * INTO printing FROM app.printing WHERE id=p_printing AND app.catalogue_source_visible(catalogue_receipt_id);
 IF NOT FOUND THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('printing','unavailable')); END IF;
 IF printing.finish NOT IN ('nonfoil','foil','etched') THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('price','unavailable')); END IF;
 BEGIN external_id:=printing.external_printing_ref::uuid;
 EXCEPTION WHEN invalid_text_representation THEN RETURN jsonb_build_object('status','fail','data',jsonb_build_object('printing','unavailable')); END;
 IF external_id IS NULL OR external_id='00000000-0000-0000-0000-000000000000'::uuid THEN
  RETURN jsonb_build_object('status','fail','data',jsonb_build_object('printing','unavailable')); END IF;
 -- Only these two fixed UTC daily windows can have a still-fresh accepted artifact.
 FOR candidate IN SELECT r.id,r.target_ref FROM app.command_receipt r
  WHERE r.principal_key='collector:scryfall-v1' AND r.scope='price_slot' AND r.state='accepted'
   AND r.target_ref IN (current_slot::text,(current_slot-1)::text) ORDER BY r.target_ref DESC LOOP
  manifest:=app.read_price_manifest(candidate.id);
  IF manifest->>'state'='valid' AND (manifest->>'acquiredAt')::timestamptz BETWEEN clock_timestamp()-interval '24 hours' AND clock_timestamp() THEN
   RETURN jsonb_build_object('status','success','data',jsonb_build_object('accountId',actor,
    'basis',jsonb_build_object('printingId',printing.id,'externalPrintingId',external_id,'slot',candidate.target_ref)));
  END IF;
 END LOOP;
 RETURN jsonb_build_object('status','fail','data',jsonb_build_object('price','unavailable'));
END $$;
REVOKE ALL ON FUNCTION app.read_my_prediction_probe_basis(uuid) FROM PUBLIC,anon,authenticated,service_role,supabase_auth_admin;
GRANT EXECUTE ON FUNCTION app.read_my_prediction_probe_basis(uuid) TO authenticated;
COMMIT;
