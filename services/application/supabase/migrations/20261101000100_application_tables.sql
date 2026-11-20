-- Generated from schema/tables.json by tools/build_schema.py; review changes before application.

-- This migration requires managed auth.users and the standard Supabase roles.

BEGIN;

CREATE SCHEMA app;

CREATE SCHEMA IF NOT EXISTS api;

REVOKE ALL ON SCHEMA app FROM PUBLIC, anon, authenticated;

GRANT USAGE ON SCHEMA app TO service_role;

CREATE TABLE "app"."account" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "auth_subject" uuid,
  "username" text NOT NULL,
  "sign_in_method" text NOT NULL,
  "created_at" timestamptz NOT NULL,
  "state" text NOT NULL,
  "deletion_requested_at" timestamptz,
  "retirement_operation_id" uuid,
  "deletion_finalized_at" timestamptz,
  UNIQUE ("auth_subject"),
  UNIQUE ("username"),
  UNIQUE ("retirement_operation_id")
);

CREATE TABLE "app"."profile" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "printing_id" uuid,
  "crop_x" double precision CHECK ("crop_x" > '-Infinity'::float8 AND "crop_x" < 'Infinity'::float8),
  "crop_y" double precision CHECK ("crop_y" > '-Infinity'::float8 AND "crop_y" < 'Infinity'::float8),
  "crop_scale" double precision CHECK ("crop_scale" > '-Infinity'::float8 AND "crop_scale" < 'Infinity'::float8),
  "default_back_id" uuid NOT NULL,
  "default_sleeve_id" uuid NOT NULL,
  "default_dice_id" uuid NOT NULL,
  "default_playmat_id" uuid,
  UNIQUE ("account_id")
);

CREATE TABLE "app"."asset_owner" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("account_id")
);

CREATE TABLE "app"."cosmetic" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "kind" text NOT NULL,
  "asset_ref" text NOT NULL,
  "state" text NOT NULL
);

CREATE TABLE "app"."supporter_tier" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "patreon_tier_ref" text NOT NULL,
  "name" text NOT NULL,
  "rank" bigint NOT NULL,
  UNIQUE ("patreon_tier_ref")
);

CREATE TABLE "app"."tier_cosmetic" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "tier_id" uuid NOT NULL,
  "cosmetic_id" uuid NOT NULL,
  UNIQUE ("tier_id", "cosmetic_id")
);

CREATE TABLE "app"."provider_event" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "provider" text NOT NULL,
  "external_event_ref" text NOT NULL,
  "account_id" uuid,
  "received_at" timestamptz NOT NULL,
  "event_type" text NOT NULL,
  "payload_ref" text NOT NULL,
  "processing_state" text NOT NULL,
  UNIQUE ("provider", "external_event_ref")
);

CREATE TABLE "app"."purchase" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "cosmetic_id" uuid NOT NULL,
  "provider_request_ref" text NOT NULL,
  "provider_event_id" uuid,
  "stripe_payment_ref" text,
  "stripe_checkout_ref" text,
  "requested_at" timestamptz NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("provider_request_ref"),
  UNIQUE ("stripe_payment_ref"),
  UNIQUE ("stripe_checkout_ref")
);

CREATE TABLE "app"."patreon_membership" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "patreon_member_ref" text NOT NULL,
  "active_tier_id" uuid,
  "active" boolean NOT NULL,
  "source_event_id" uuid NOT NULL,
  UNIQUE ("account_id")
);

CREATE TABLE "app"."patreon_payment" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "membership_id" uuid NOT NULL,
  "tier_id" uuid NOT NULL,
  "provider_event_id" uuid NOT NULL,
  "payment_ref" text NOT NULL,
  "paid_at" timestamptz NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("payment_ref")
);

CREATE TABLE "app"."cosmetic_grant" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "cosmetic_id" uuid NOT NULL,
  "source_kind" text NOT NULL,
  "purchase_id" uuid,
  "patreon_payment_id" uuid,
  "state" text NOT NULL,
  UNIQUE ("account_id", "cosmetic_id", "purchase_id"),
  UNIQUE ("account_id", "cosmetic_id", "patreon_payment_id")
);


CREATE TABLE "app"."card_set" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "external_set_ref" text NOT NULL,
  "name" text NOT NULL,
  UNIQUE ("external_set_ref")
);

CREATE TABLE "app"."card_definition" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "oracle_ref" text NOT NULL,
  "name" text NOT NULL,
  UNIQUE ("oracle_ref")
);

CREATE TABLE "app"."card_definition_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "definition_id" uuid NOT NULL,
  "revision_ref" text NOT NULL,
  "rules_text_ref" text NOT NULL,
  UNIQUE ("definition_id", "revision_ref")
);

CREATE TABLE "app"."card_face" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "definition_revision_id" uuid NOT NULL,
  "face_index" bigint NOT NULL,
  "name" text NOT NULL,
  "characteristics_ref" text NOT NULL,
  UNIQUE ("definition_revision_id", "face_index")
);

CREATE TABLE "app"."printing" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "definition_id" uuid NOT NULL,
  "set_id" uuid NOT NULL,
  "external_printing_ref" text NOT NULL,
  "rarity" text NOT NULL,
  "finish" text NOT NULL,
  "image_ref" text NOT NULL,
  UNIQUE ("external_printing_ref", "finish")
);

CREATE TABLE "app"."format" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "key" text NOT NULL,
  "name" text NOT NULL,
  "next_competitive_sequence" numeric(78,0) NOT NULL CHECK ("next_competitive_sequence" >= 0 AND "next_competitive_sequence" <> 'NaN'::numeric),
  "last_ranked_sequence" numeric(78,0) NOT NULL CHECK ("last_ranked_sequence" >= 0 AND "last_ranked_sequence" <> 'NaN'::numeric),
  UNIQUE ("key")
);

CREATE TABLE "app"."legality_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "source_ref" text NOT NULL,
  "effective_at" timestamptz NOT NULL,
  "imported_at" timestamptz NOT NULL
);

CREATE TABLE "app"."card_legality" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "definition_id" uuid NOT NULL,
  "format_id" uuid NOT NULL,
  "legality_revision_id" uuid NOT NULL,
  "status" text NOT NULL,
  UNIQUE ("definition_id", "format_id", "legality_revision_id")
);

CREATE TABLE "app"."booster_product" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "set_id" uuid NOT NULL,
  "name" text NOT NULL,
  "external_product_ref" text NOT NULL
);

CREATE TABLE "app"."booster_recipe" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "product_id" uuid NOT NULL,
  "revision_ref" text NOT NULL,
  "evidence_ref" text NOT NULL,
  UNIQUE ("product_id", "revision_ref")
);

CREATE TABLE "app"."booster_slot" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "recipe_id" uuid NOT NULL,
  "slot_key" text NOT NULL,
  "draws" numeric(78,0) NOT NULL CHECK ("draws" >= 0 AND "draws" <> 'NaN'::numeric),
  "collation_ref" text,
  UNIQUE ("recipe_id", "slot_key")
);

CREATE TABLE "app"."booster_slot_card" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "slot_id" uuid NOT NULL,
  "printing_id" uuid NOT NULL,
  "weight" numeric(78,0) NOT NULL CHECK ("weight" >= 0 AND "weight" <> 'NaN'::numeric),
  UNIQUE ("slot_id", "printing_id")
);

CREATE TABLE "app"."table_profile_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "key" text NOT NULL,
  "revision_ref" text NOT NULL,
  "rules_ref" text NOT NULL,
  "planar_pool_ref" text,
  UNIQUE ("key", "revision_ref")
);

CREATE TABLE "app"."qualified_release" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "release_ref" text NOT NULL,
  "kernel_ref" text NOT NULL,
  "rules_ref" text NOT NULL,
  "program_manifest_ref" text NOT NULL,
  "evidence_ref" text NOT NULL,
  UNIQUE ("release_ref")
);

CREATE TABLE "app"."release_card_program" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "release_id" uuid NOT NULL,
  "definition_revision_id" uuid NOT NULL,
  "program_ref" text NOT NULL,
  UNIQUE ("release_id", "definition_revision_id")
);

CREATE TABLE "app"."deck" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid,
  "source_kind" text NOT NULL,
  "system_pool_ref" text,
  "source_manifest_ref" text,
  "mode" text NOT NULL,
  "format_id" uuid NOT NULL,
  "name" text NOT NULL,
  "current_revision_id" uuid,
  "back_id" uuid NOT NULL,
  "sleeve_id" uuid NOT NULL,
  "dice_id" uuid NOT NULL,
  "playmat_id" uuid
);

CREATE TABLE "app"."deck_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "deck_id" uuid NOT NULL,
  "revision_number" numeric(78,0) NOT NULL CHECK ("revision_number" >= 0 AND "revision_number" <> 'NaN'::numeric),
  "created_at" timestamptz NOT NULL,
  UNIQUE ("deck_id", "revision_number")
);

CREATE TABLE "app"."deck_entry" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "revision_id" uuid NOT NULL,
  "definition_id" uuid NOT NULL,
  "printing_id" uuid,
  "section" text NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric)
);

CREATE TABLE "app"."asset_definition" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "kind" text NOT NULL,
  "currency_key" text,
  "printing_id" uuid,
  "recipe_id" uuid,
  UNIQUE ("currency_key"),
  UNIQUE ("printing_id"),
  UNIQUE ("recipe_id")
);

CREATE TABLE "app"."inventory_asset" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "owner_id" uuid NOT NULL,
  "asset_definition_id" uuid NOT NULL,
  "owned_quantity" numeric(78,0) NOT NULL CHECK ("owned_quantity" >= 0 AND "owned_quantity" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("owner_id", "asset_definition_id")
);

CREATE TABLE "app"."ledger_operation" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "kind" text NOT NULL,
  "source_ref" text NOT NULL,
  "source_scope" text NOT NULL,
  "actor_account_id" uuid,
  "created_at" timestamptz NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("source_scope", "source_ref")
);

CREATE TABLE "app"."ledger_posting" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "operation_id" uuid NOT NULL,
  "inventory_asset_id" uuid NOT NULL,
  "direction" text NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "source_kind" text NOT NULL,
  "source_ref" text NOT NULL
);

CREATE TABLE "app"."asset_hold" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "owner_id" uuid NOT NULL,
  "purpose" text NOT NULL,
  "commitment_ref" text NOT NULL,
  "state" text NOT NULL,
  "operation_id" uuid NOT NULL,
  UNIQUE ("purpose", "commitment_ref", "owner_id")
);

CREATE TABLE "app"."hold_line" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "hold_id" uuid NOT NULL,
  "inventory_asset_id" uuid NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  UNIQUE ("hold_id", "inventory_asset_id")
);

CREATE TABLE "app"."command_receipt" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid,
  "controller_grant_id" uuid,
  "principal_key" text NOT NULL,
  "request_ref" text NOT NULL,
  "scope" text NOT NULL,
  "target_ref" text NOT NULL,
  "payload_hash" text NOT NULL,
  "operation_id" uuid,
  "state" text NOT NULL,
  "result_ref" text NOT NULL,
  UNIQUE ("principal_key", "scope", "target_ref", "request_ref")
);

CREATE TABLE "app"."valuation_policy_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "revision_ref" text NOT NULL,
  "formula_ref" text NOT NULL,
  "price_anchor_ref" text NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("revision_ref")
);

CREATE TABLE "app"."pack_open_receipt" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "inventory_asset_id" uuid NOT NULL,
  "recipe_id" uuid NOT NULL,
  "operation_id" uuid NOT NULL,
  "result_ref" text NOT NULL,
  UNIQUE ("operation_id")
);

CREATE TABLE "app"."friendship" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_low_id" uuid NOT NULL,
  "account_high_id" uuid NOT NULL,
  "requested_by_id" uuid NOT NULL,
  "state" text NOT NULL,
  "accepted_at" timestamptz,
  UNIQUE ("account_low_id", "account_high_id")
);

CREATE TABLE "app"."block" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "blocker_id" uuid NOT NULL,
  "blocked_id" uuid NOT NULL,
  "created_at" timestamptz NOT NULL,
  UNIQUE ("blocker_id", "blocked_id")
);

CREATE TABLE "app"."conversation" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "kind" text NOT NULL,
  "match_id" uuid,
  "created_at" timestamptz NOT NULL
);

CREATE TABLE "app"."conversation_member" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "conversation_id" uuid NOT NULL,
  "account_id" uuid NOT NULL,
  UNIQUE ("conversation_id", "account_id")
);

CREATE TABLE "app"."message" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "conversation_id" uuid NOT NULL,
  "sender_id" uuid NOT NULL,
  "send_request_ref" text NOT NULL,
  "body" text NOT NULL,
  "sent_at" timestamptz NOT NULL,
  UNIQUE ("sender_id", "send_request_ref")
);

CREATE TABLE "app"."notification" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "recipient_id" uuid NOT NULL,
  "source_kind" text NOT NULL,
  "source_ref" text NOT NULL,
  "body" text NOT NULL,
  "read_at" timestamptz,
  UNIQUE ("recipient_id", "source_kind", "source_ref")
);

CREATE TABLE "app"."trade_offer" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "sender_id" uuid NOT NULL,
  "recipient_id" uuid NOT NULL,
  "sender_hold_id" uuid NOT NULL,
  "terms_revision" numeric(78,0) NOT NULL CHECK ("terms_revision" >= 0 AND "terms_revision" <> 'NaN'::numeric),
  "state" text NOT NULL,
  "settlement_operation_id" uuid,
  "created_at" timestamptz NOT NULL,
  UNIQUE ("sender_hold_id")
);

CREATE TABLE "app"."trade_line" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "trade_id" uuid NOT NULL,
  "asset_definition_id" uuid NOT NULL,
  "side" text NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  UNIQUE ("trade_id", "side", "asset_definition_id")
);

CREATE TABLE "app"."market_listing" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "seller_id" uuid NOT NULL,
  "asset_definition_id" uuid NOT NULL,
  "hold_id" uuid NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "total_price_coins" numeric(78,0) NOT NULL CHECK ("total_price_coins" >= 0 AND "total_price_coins" <> 'NaN'::numeric),
  "state" text NOT NULL,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  "buyer_id" uuid,
  "settlement_operation_id" uuid,
  "paid_coins" numeric(78,0) CHECK ("paid_coins" >= 0 AND "paid_coins" <> 'NaN'::numeric),
  "completed_at" timestamptz,
  UNIQUE ("hold_id"),
  UNIQUE ("settlement_operation_id")
);

CREATE TABLE "app"."prediction_market" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "creator_id" uuid NOT NULL,
  "printing_id" uuid NOT NULL,
  "price_metric_ref" text NOT NULL,
  "target_usd_decimal" text NOT NULL,
  "condition" text NOT NULL,
  "deadline" timestamptz NOT NULL,
  "trading_closes_at" timestamptz NOT NULL,
  "observation_starts_at" timestamptz NOT NULL,
  "sampling_policy_ref" text NOT NULL,
  "terminal_slot_ref" text NOT NULL,
  "rules_revision" text NOT NULL,
  "payout_coins" numeric(78,0) NOT NULL CHECK ("payout_coins" >= 0 AND "payout_coins" <> 'NaN'::numeric),
  "next_order_sequence" numeric(78,0) NOT NULL CHECK ("next_order_sequence" >= 0 AND "next_order_sequence" <> 'NaN'::numeric),
  "state" text NOT NULL,
  "resolution_evidence_ref" text,
  "resolution_rules_revision" text,
  "outcome" text,
  "settlement_operation_id" uuid,
  "resolved_at" timestamptz,
  UNIQUE ("settlement_operation_id")
);

CREATE TABLE "app"."prediction_order" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "market_id" uuid NOT NULL,
  "account_id" uuid NOT NULL,
  "hold_id" uuid NOT NULL,
  "side" text NOT NULL,
  "limit_cost_coins" numeric(78,0) NOT NULL CHECK ("limit_cost_coins" >= 0 AND "limit_cost_coins" <> 'NaN'::numeric),
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "filled_quantity" numeric(78,0) NOT NULL CHECK ("filled_quantity" >= 0 AND "filled_quantity" <> 'NaN'::numeric),
  "accepted_sequence" numeric(78,0) NOT NULL CHECK ("accepted_sequence" >= 0 AND "accepted_sequence" <> 'NaN'::numeric),
  "state" text NOT NULL,
  UNIQUE ("hold_id"),
  UNIQUE ("market_id", "accepted_sequence")
);

CREATE TABLE "app"."prediction_pair" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "market_id" uuid NOT NULL,
  "yes_order_id" uuid NOT NULL,
  "no_order_id" uuid NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "fixed_payout_coins" numeric(78,0) NOT NULL CHECK ("fixed_payout_coins" >= 0 AND "fixed_payout_coins" <> 'NaN'::numeric),
  "yes_actual_cost_coins" numeric(78,0) NOT NULL CHECK ("yes_actual_cost_coins" >= 0 AND "yes_actual_cost_coins" <> 'NaN'::numeric),
  "no_actual_cost_coins" numeric(78,0) NOT NULL CHECK ("no_actual_cost_coins" >= 0 AND "no_actual_cost_coins" <> 'NaN'::numeric),
  "match_operation_id" uuid NOT NULL,
  UNIQUE ("match_operation_id")
);

CREATE TABLE "app"."price_observation" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "market_id" uuid NOT NULL,
  "printing_id" uuid NOT NULL,
  "source_ref" text NOT NULL,
  "observed_at" timestamptz NOT NULL,
  "scheduled_slot_ref" text NOT NULL,
  "source_artifact_updated_at" timestamptz,
  "price_field" text NOT NULL,
  "price_usd_decimal" text,
  "evidence_ref" text NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("market_id", "scheduled_slot_ref")
);

CREATE TABLE "app"."room" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "host_id" uuid NOT NULL,
  "format_id" uuid NOT NULL,
  "profile_id" uuid NOT NULL,
  "mode" text NOT NULL,
  "competitive" boolean NOT NULL,
  "visibility" text NOT NULL,
  "state" text NOT NULL,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  "countdown_at" timestamptz
);

CREATE TABLE "app"."room_slot" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "room_id" uuid NOT NULL,
  "account_id" uuid,
  "sponsor_id" uuid,
  "deck_revision_id" uuid,
  "seat_index" bigint NOT NULL,
  "team_ref" text,
  "controller_kind" text NOT NULL,
  "ready" boolean NOT NULL,
  UNIQUE ("room_id", "seat_index")
);

CREATE TABLE "app"."invitation" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "room_id" uuid NOT NULL,
  "issuer_id" uuid NOT NULL,
  "token_hash" text NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("token_hash")
);

CREATE TABLE "app"."queue_entry" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "deck_revision_id" uuid NOT NULL,
  "mode" text NOT NULL,
  "competitive" boolean NOT NULL,
  "preferences_ref" text NOT NULL,
  "reserved_match_id" uuid,
  "state" text NOT NULL
);

CREATE TABLE "app"."match" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "room_id" uuid,
  "format_id" uuid NOT NULL,
  "profile_id" uuid NOT NULL,
  "legality_revision_id" uuid NOT NULL,
  "mode" text NOT NULL,
  "competitive" boolean NOT NULL,
  "structure" text NOT NULL,
  "state" text NOT NULL,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric)
);

CREATE TABLE "app"."match_seat" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "account_id" uuid,
  "sponsor_id" uuid,
  "seat_index" bigint NOT NULL,
  "team_ref" text,
  "controller_kind" text NOT NULL,
  "source_revision_id" uuid,
  "deck_snapshot_ref" text,
  "deck_accepted_at" timestamptz,
  UNIQUE ("match_id", "seat_index")
);

CREATE TABLE "app"."match_clock" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_seat_id" uuid NOT NULL,
  "remaining_ms" numeric(78,0) NOT NULL CHECK ("remaining_ms" >= 0 AND "remaining_ms" <> 'NaN'::numeric),
  "clock_revision" numeric(78,0) NOT NULL CHECK ("clock_revision" >= 0 AND "clock_revision" <> 'NaN'::numeric),
  "policy_ref" text NOT NULL,
  "paused" boolean NOT NULL,
  "last_accounted_at" timestamptz NOT NULL,
  UNIQUE ("match_seat_id")
);

CREATE TABLE "app"."between_game_decision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "seat_id" uuid NOT NULL,
  "decision_kind" text NOT NULL,
  "basis_revision" numeric(78,0) NOT NULL CHECK ("basis_revision" >= 0 AND "basis_revision" <> 'NaN'::numeric),
  "reply_ref" text,
  "deadline_at" timestamptz,
  "clock_revision" numeric(78,0) NOT NULL CHECK ("clock_revision" >= 0 AND "clock_revision" <> 'NaN'::numeric),
  "state" text NOT NULL
);

CREATE TABLE "app"."match_result" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "source_ref" text NOT NULL,
  "outcome" text NOT NULL,
  "winners_ref" text NOT NULL,
  "committed_at" timestamptz NOT NULL,
  "ranking_format_id" uuid,
  "rank_sequence" numeric(78,0) CHECK ("rank_sequence" >= 0 AND "rank_sequence" <> 'NaN'::numeric),
  UNIQUE ("match_id"),
  UNIQUE ("source_ref"),
  UNIQUE ("ranking_format_id", "rank_sequence")
);

CREATE TABLE "app"."game" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "game_index" numeric(78,0) NOT NULL CHECK ("game_index" >= 0 AND "game_index" <> 'NaN'::numeric),
  "state" text NOT NULL,
  "accepted_revision" numeric(78,0) NOT NULL CHECK ("accepted_revision" >= 0 AND "accepted_revision" <> 'NaN'::numeric),
  UNIQUE ("match_id", "game_index")
);

CREATE TABLE "app"."game_context" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "release_id" uuid NOT NULL,
  "profile_id" uuid NOT NULL,
  "snapshot_ref" text NOT NULL,
  "context_revision" text NOT NULL,
  UNIQUE ("game_id")
);

CREATE TABLE "app"."game_checkpoint" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "context_id" uuid NOT NULL,
  "accepted_revision" numeric(78,0) NOT NULL CHECK ("accepted_revision" >= 0 AND "accepted_revision" <> 'NaN'::numeric),
  "private_state_ref" text NOT NULL,
  "facts_ref" text NOT NULL,
  "clock_revision" numeric(78,0) NOT NULL CHECK ("clock_revision" >= 0 AND "clock_revision" <> 'NaN'::numeric),
  UNIQUE ("game_id", "accepted_revision")
);

CREATE TABLE "app"."game_event" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "context_id" uuid NOT NULL,
  "sequence" numeric(78,0) NOT NULL CHECK ("sequence" >= 0 AND "sequence" <> 'NaN'::numeric),
  "accepted_revision" numeric(78,0) NOT NULL CHECK ("accepted_revision" >= 0 AND "accepted_revision" <> 'NaN'::numeric),
  "private_payload_ref" text NOT NULL,
  UNIQUE ("game_id", "sequence")
);

CREATE TABLE "app"."game_result" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "source_event_id" uuid NOT NULL,
  "outcome" text NOT NULL,
  "winners_ref" text NOT NULL,
  "end_reason" text NOT NULL,
  UNIQUE ("game_id")
);

CREATE TABLE "app"."operational_state" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "owner_instance_ref" text NOT NULL,
  "fence_epoch" numeric(78,0) NOT NULL CHECK ("fence_epoch" >= 0 AND "fence_epoch" <> 'NaN'::numeric),
  "availability" text NOT NULL,
  "incident_ref" text,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("game_id")
);

CREATE TABLE "app"."durable_handoff" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "game_id" uuid,
  "source_ref" text NOT NULL,
  "destination" text NOT NULL,
  "kind" text NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("kind", "source_ref")
);

CREATE TABLE "app"."replay_manifest" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "context_id" uuid NOT NULL,
  "artifact_ref" text NOT NULL,
  "visibility_index_ref" text NOT NULL,
  "schema_revision" text NOT NULL,
  UNIQUE ("game_id")
);

CREATE TABLE "app"."ai_runner_binding" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "room_slot_id" uuid NOT NULL,
  "match_seat_id" uuid,
  "provider_runner_ref" text,
  "binding_epoch" numeric(78,0) NOT NULL CHECK ("binding_epoch" >= 0 AND "binding_epoch" <> 'NaN'::numeric),
  "desired_state" text NOT NULL,
  "observed_state" text NOT NULL
);

CREATE TABLE "app"."controller_grant" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "binding_id" uuid NOT NULL,
  "binding_epoch" numeric(78,0) NOT NULL CHECK ("binding_epoch" >= 0 AND "binding_epoch" <> 'NaN'::numeric),
  "match_seat_id" uuid,
  "grant_secret_ref" text NOT NULL,
  "state" text NOT NULL
);

CREATE TABLE "app"."ai_loss_bucket" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "sponsor_id" uuid NOT NULL,
  "utc_day" text NOT NULL,
  "limit_coins" numeric(78,0) NOT NULL CHECK ("limit_coins" >= 0 AND "limit_coins" <> 'NaN'::numeric),
  "lost_coins" numeric(78,0) NOT NULL CHECK ("lost_coins" >= 0 AND "lost_coins" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("sponsor_id", "utc_day")
);

CREATE TABLE "app"."ai_loss_exposure" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "bucket_id" uuid NOT NULL,
  "match_id" uuid NOT NULL,
  "commitment_id" uuid NOT NULL,
  "reserved_coins" numeric(78,0) NOT NULL CHECK ("reserved_coins" >= 0 AND "reserved_coins" <> 'NaN'::numeric),
  "state" text NOT NULL,
  UNIQUE ("bucket_id", "match_id")
);

CREATE TABLE "app"."ante_commitment" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "match_id" uuid NOT NULL,
  "terms_revision" numeric(78,0) NOT NULL CHECK ("terms_revision" >= 0 AND "terms_revision" <> 'NaN'::numeric),
  "state" text NOT NULL,
  "reservation_operation_id" uuid,
  "settlement_operation_id" uuid,
  "allocation_evidence_ref" text,
  "valuation_evidence_ref" text,
  UNIQUE ("match_id")
);

CREATE TABLE "app"."ante_stake" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "commitment_id" uuid NOT NULL,
  "match_seat_id" uuid NOT NULL,
  "funding_kind" text NOT NULL,
  "hold_id" uuid,
  "estimated_value_coins" numeric(78,0) NOT NULL CHECK ("estimated_value_coins" >= 0 AND "estimated_value_coins" <> 'NaN'::numeric),
  "valuation_policy_id" uuid NOT NULL,
  "ai_exposure_id" uuid,
  UNIQUE ("commitment_id", "match_seat_id"),
  UNIQUE ("hold_id")
);

CREATE TABLE "app"."game_participation" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "match_seat_id" uuid NOT NULL,
  "account_id" uuid NOT NULL,
  "own_turns" numeric(78,0) NOT NULL CHECK ("own_turns" >= 0 AND "own_turns" <> 'NaN'::numeric),
  "real_game" boolean NOT NULL,
  "reason" text NOT NULL,
  "completed_at" timestamptz NOT NULL,
  UNIQUE ("game_id", "account_id")
);

CREATE TABLE "app"."game_activity_fact" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "game_id" uuid NOT NULL,
  "source_event_id" uuid NOT NULL,
  "account_id" uuid NOT NULL,
  "kind" text NOT NULL,
  "value" numeric(78,0) NOT NULL CHECK ("value" >= 0 AND "value" <> 'NaN'::numeric),
  "fact_revision" text NOT NULL,
  UNIQUE ("source_event_id", "account_id", "kind", "fact_revision")
);

CREATE TABLE "app"."activity_consumer_cursor" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "consumer_key" text NOT NULL,
  "game_id" uuid NOT NULL,
  "last_sequence" numeric(78,0) NOT NULL CHECK ("last_sequence" >= 0 AND "last_sequence" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("consumer_key", "game_id")
);

CREATE TABLE "app"."season" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "year" bigint NOT NULL,
  "quarter" bigint NOT NULL,
  "start_at" timestamptz NOT NULL,
  "end_at" timestamptz NOT NULL,
  "result_cutoff_ref" text,
  "policy_ref" text NOT NULL,
  "state" text NOT NULL,
  UNIQUE ("year", "quarter")
);

CREATE TABLE "app"."set_ballot" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "season_id" uuid NOT NULL,
  "account_id" uuid NOT NULL,
  "set_id" uuid NOT NULL,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("season_id", "account_id", "set_id")
);

CREATE TABLE "app"."monthly_account_activity" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "utc_month" text NOT NULL,
  "activity_policy_ref" text NOT NULL,
  "source_ref" text NOT NULL,
  UNIQUE ("account_id", "utc_month", "activity_policy_ref")
);

CREATE TABLE "app"."supply_drop" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "season_id" uuid NOT NULL,
  "set_id" uuid NOT NULL,
  "population_snapshot_ref" text NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "policy_ref" text NOT NULL,
  UNIQUE ("season_id", "set_id")
);

CREATE TABLE "app"."supply_pricing_policy" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "product_id" uuid NOT NULL,
  "revision_ref" text NOT NULL,
  "base_price_coins" numeric(78,0) NOT NULL CHECK ("base_price_coins" >= 0 AND "base_price_coins" <> 'NaN'::numeric),
  "product_modifier_ref" text NOT NULL,
  "window_starting_quantity" numeric(78,0) NOT NULL CHECK ("window_starting_quantity" >= 0 AND "window_starting_quantity" <> 'NaN'::numeric),
  "normal_quantity" numeric(78,0) NOT NULL CHECK ("normal_quantity" >= 0 AND "normal_quantity" <> 'NaN'::numeric),
  "published_at" timestamptz NOT NULL,
  "formula_ref" text NOT NULL,
  UNIQUE ("revision_ref")
);

CREATE TABLE "app"."supply_stock" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "recipe_id" uuid NOT NULL,
  "drop_id" uuid,
  "origin" text NOT NULL,
  "pricing_policy_id" uuid NOT NULL,
  "starting_quantity" numeric(78,0) NOT NULL CHECK ("starting_quantity" >= 0 AND "starting_quantity" <> 'NaN'::numeric),
  "remaining_quantity" numeric(78,0) NOT NULL CHECK ("remaining_quantity" >= 0 AND "remaining_quantity" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric)
);

CREATE TABLE "app"."supply_purchase" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "stock_id" uuid NOT NULL,
  "pricing_policy_id" uuid NOT NULL,
  "buyer_id" uuid NOT NULL,
  "operation_id" uuid NOT NULL,
  "quantity" numeric(78,0) NOT NULL CHECK ("quantity" >= 0 AND "quantity" <> 'NaN'::numeric),
  "paid_coins" numeric(78,0) NOT NULL CHECK ("paid_coins" >= 0 AND "paid_coins" <> 'NaN'::numeric),
  UNIQUE ("operation_id", "stock_id")
);

CREATE TABLE "app"."quest_definition_revision" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "definition_key" text NOT NULL,
  "revision" text NOT NULL,
  "objective_ref" text NOT NULL,
  "target" numeric(78,0) NOT NULL CHECK ("target" >= 0 AND "target" <> 'NaN'::numeric),
  "reward_coins" numeric(78,0) NOT NULL CHECK ("reward_coins" >= 0 AND "reward_coins" <> 'NaN'::numeric),
  UNIQUE ("definition_key", "revision")
);

CREATE TABLE "app"."quest_assignment" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "definition_id" uuid NOT NULL,
  "source_day" text NOT NULL,
  "assignment_ref" text NOT NULL,
  "progress" numeric(78,0) NOT NULL CHECK ("progress" >= 0 AND "progress" <> 'NaN'::numeric),
  "completed_at" timestamptz,
  "replaced_by_id" uuid,
  "reward_operation_id" uuid,
  "reward_source_fact_id" uuid,
  UNIQUE ("account_id", "assignment_ref"),
  UNIQUE ("reward_operation_id")
);

CREATE TABLE "app"."daily_quest_state" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "utc_day" text NOT NULL,
  "generated_assignment_id" uuid,
  "rerolls_used" numeric(78,0) NOT NULL CHECK ("rerolls_used" >= 0 AND "rerolls_used" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("account_id", "utc_day")
);

CREATE TABLE "app"."competitive_result" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "match_result_id" uuid NOT NULL,
  "source_ref" text NOT NULL,
  "format_id" uuid NOT NULL,
  "win" boolean NOT NULL,
  "completed_at" timestamptz NOT NULL,
  "policy_ref" text NOT NULL,
  UNIQUE ("account_id", "source_ref")
);

CREATE TABLE "app"."rank" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "format_id" uuid NOT NULL,
  "season_id" uuid NOT NULL,
  "tier" text NOT NULL,
  "rating_ref" text NOT NULL,
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  "finalized" boolean NOT NULL,
  UNIQUE ("account_id", "format_id", "season_id")
);

CREATE TABLE "app"."daily_win_bucket" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "account_id" uuid NOT NULL,
  "utc_day" text NOT NULL,
  "wins" numeric(78,0) NOT NULL CHECK ("wins" >= 0 AND "wins" <> 'NaN'::numeric),
  "issued_coins" numeric(78,0) NOT NULL CHECK ("issued_coins" >= 0 AND "issued_coins" <> 'NaN'::numeric),
  "revision" numeric(78,0) NOT NULL CHECK ("revision" >= 0 AND "revision" <> 'NaN'::numeric),
  UNIQUE ("account_id", "utc_day")
);

ALTER TABLE "app"."account" ADD FOREIGN KEY ("auth_subject") REFERENCES "auth"."users" (id) ON DELETE SET NULL DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."account" ("auth_subject");

ALTER TABLE "app"."account" ADD FOREIGN KEY ("retirement_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."account" ("retirement_operation_id");

ALTER TABLE "app"."account" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."account" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."account" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."account" TO service_role;

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("account_id");

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("printing_id");

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("default_back_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("default_back_id");

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("default_sleeve_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("default_sleeve_id");

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("default_dice_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("default_dice_id");

ALTER TABLE "app"."profile" ADD FOREIGN KEY ("default_playmat_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."profile" ("default_playmat_id");

ALTER TABLE "app"."profile" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."profile" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."profile" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."profile" TO service_role;

ALTER TABLE "app"."asset_owner" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."asset_owner" ("account_id");

ALTER TABLE "app"."asset_owner" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."asset_owner" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."asset_owner" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."asset_owner" TO service_role;

ALTER TABLE "app"."cosmetic" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."cosmetic" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."cosmetic" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."cosmetic" TO service_role;

ALTER TABLE "app"."supporter_tier" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."supporter_tier" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."supporter_tier" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."supporter_tier" TO service_role;

ALTER TABLE "app"."tier_cosmetic" ADD FOREIGN KEY ("tier_id") REFERENCES "app"."supporter_tier" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."tier_cosmetic" ("tier_id");

ALTER TABLE "app"."tier_cosmetic" ADD FOREIGN KEY ("cosmetic_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."tier_cosmetic" ("cosmetic_id");

ALTER TABLE "app"."tier_cosmetic" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."tier_cosmetic" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."tier_cosmetic" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."tier_cosmetic" TO service_role;

ALTER TABLE "app"."provider_event" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."provider_event" ("account_id");

ALTER TABLE "app"."provider_event" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."provider_event" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."provider_event" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."provider_event" TO service_role;

ALTER TABLE "app"."purchase" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."purchase" ("account_id");

ALTER TABLE "app"."purchase" ADD FOREIGN KEY ("cosmetic_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."purchase" ("cosmetic_id");

ALTER TABLE "app"."purchase" ADD FOREIGN KEY ("provider_event_id") REFERENCES "app"."provider_event" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."purchase" ("provider_event_id");

ALTER TABLE "app"."purchase" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."purchase" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."purchase" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."purchase" TO service_role;

ALTER TABLE "app"."patreon_membership" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_membership" ("account_id");

ALTER TABLE "app"."patreon_membership" ADD FOREIGN KEY ("active_tier_id") REFERENCES "app"."supporter_tier" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_membership" ("active_tier_id");

ALTER TABLE "app"."patreon_membership" ADD FOREIGN KEY ("source_event_id") REFERENCES "app"."provider_event" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_membership" ("source_event_id");

ALTER TABLE "app"."patreon_membership" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."patreon_membership" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."patreon_membership" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."patreon_membership" TO service_role;

ALTER TABLE "app"."patreon_payment" ADD FOREIGN KEY ("membership_id") REFERENCES "app"."patreon_membership" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_payment" ("membership_id");

ALTER TABLE "app"."patreon_payment" ADD FOREIGN KEY ("tier_id") REFERENCES "app"."supporter_tier" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_payment" ("tier_id");

ALTER TABLE "app"."patreon_payment" ADD FOREIGN KEY ("provider_event_id") REFERENCES "app"."provider_event" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."patreon_payment" ("provider_event_id");

ALTER TABLE "app"."patreon_payment" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."patreon_payment" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."patreon_payment" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."patreon_payment" TO service_role;

ALTER TABLE "app"."cosmetic_grant" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."cosmetic_grant" ("account_id");

ALTER TABLE "app"."cosmetic_grant" ADD FOREIGN KEY ("cosmetic_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."cosmetic_grant" ("cosmetic_id");

ALTER TABLE "app"."cosmetic_grant" ADD FOREIGN KEY ("purchase_id") REFERENCES "app"."purchase" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."cosmetic_grant" ("purchase_id");

ALTER TABLE "app"."cosmetic_grant" ADD FOREIGN KEY ("patreon_payment_id") REFERENCES "app"."patreon_payment" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."cosmetic_grant" ("patreon_payment_id");

ALTER TABLE "app"."cosmetic_grant" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."cosmetic_grant" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."cosmetic_grant" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."cosmetic_grant" TO service_role;











ALTER TABLE "app"."card_set" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."card_set" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."card_set" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."card_set" TO service_role;

ALTER TABLE "app"."card_definition" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."card_definition" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."card_definition" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."card_definition" TO service_role;

ALTER TABLE "app"."card_definition_revision" ADD FOREIGN KEY ("definition_id") REFERENCES "app"."card_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."card_definition_revision" ("definition_id");

ALTER TABLE "app"."card_definition_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."card_definition_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."card_definition_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."card_definition_revision" TO service_role;

ALTER TABLE "app"."card_face" ADD FOREIGN KEY ("definition_revision_id") REFERENCES "app"."card_definition_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."card_face" ("definition_revision_id");

ALTER TABLE "app"."card_face" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."card_face" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."card_face" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."card_face" TO service_role;

ALTER TABLE "app"."printing" ADD FOREIGN KEY ("definition_id") REFERENCES "app"."card_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."printing" ("definition_id");

ALTER TABLE "app"."printing" ADD FOREIGN KEY ("set_id") REFERENCES "app"."card_set" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."printing" ("set_id");

ALTER TABLE "app"."printing" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."printing" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."printing" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."printing" TO service_role;

ALTER TABLE "app"."format" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."format" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."format" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."format" TO service_role;

ALTER TABLE "app"."legality_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."legality_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."legality_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."legality_revision" TO service_role;

ALTER TABLE "app"."card_legality" ADD FOREIGN KEY ("definition_id") REFERENCES "app"."card_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."card_legality" ("definition_id");

ALTER TABLE "app"."card_legality" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."card_legality" ("format_id");

ALTER TABLE "app"."card_legality" ADD FOREIGN KEY ("legality_revision_id") REFERENCES "app"."legality_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."card_legality" ("legality_revision_id");

ALTER TABLE "app"."card_legality" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."card_legality" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."card_legality" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."card_legality" TO service_role;

ALTER TABLE "app"."booster_product" ADD FOREIGN KEY ("set_id") REFERENCES "app"."card_set" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."booster_product" ("set_id");

ALTER TABLE "app"."booster_product" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."booster_product" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."booster_product" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."booster_product" TO service_role;

ALTER TABLE "app"."booster_recipe" ADD FOREIGN KEY ("product_id") REFERENCES "app"."booster_product" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."booster_recipe" ("product_id");

ALTER TABLE "app"."booster_recipe" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."booster_recipe" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."booster_recipe" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."booster_recipe" TO service_role;

ALTER TABLE "app"."booster_slot" ADD FOREIGN KEY ("recipe_id") REFERENCES "app"."booster_recipe" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."booster_slot" ("recipe_id");

ALTER TABLE "app"."booster_slot" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."booster_slot" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."booster_slot" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."booster_slot" TO service_role;

ALTER TABLE "app"."booster_slot_card" ADD FOREIGN KEY ("slot_id") REFERENCES "app"."booster_slot" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."booster_slot_card" ("slot_id");

ALTER TABLE "app"."booster_slot_card" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."booster_slot_card" ("printing_id");

ALTER TABLE "app"."booster_slot_card" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."booster_slot_card" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."booster_slot_card" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."booster_slot_card" TO service_role;

ALTER TABLE "app"."table_profile_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."table_profile_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."table_profile_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."table_profile_revision" TO service_role;

ALTER TABLE "app"."qualified_release" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."qualified_release" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."qualified_release" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."qualified_release" TO service_role;

ALTER TABLE "app"."release_card_program" ADD FOREIGN KEY ("release_id") REFERENCES "app"."qualified_release" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."release_card_program" ("release_id");

ALTER TABLE "app"."release_card_program" ADD FOREIGN KEY ("definition_revision_id") REFERENCES "app"."card_definition_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."release_card_program" ("definition_revision_id");

ALTER TABLE "app"."release_card_program" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."release_card_program" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."release_card_program" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."release_card_program" TO service_role;

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("account_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("format_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("current_revision_id") REFERENCES "app"."deck_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("current_revision_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("back_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("back_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("sleeve_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("sleeve_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("dice_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("dice_id");

ALTER TABLE "app"."deck" ADD FOREIGN KEY ("playmat_id") REFERENCES "app"."cosmetic" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck" ("playmat_id");

ALTER TABLE "app"."deck" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."deck" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."deck" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."deck" TO service_role;

ALTER TABLE "app"."deck_revision" ADD FOREIGN KEY ("deck_id") REFERENCES "app"."deck" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck_revision" ("deck_id");

ALTER TABLE "app"."deck_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."deck_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."deck_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."deck_revision" TO service_role;

ALTER TABLE "app"."deck_entry" ADD FOREIGN KEY ("revision_id") REFERENCES "app"."deck_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck_entry" ("revision_id");

ALTER TABLE "app"."deck_entry" ADD FOREIGN KEY ("definition_id") REFERENCES "app"."card_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck_entry" ("definition_id");

ALTER TABLE "app"."deck_entry" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."deck_entry" ("printing_id");

ALTER TABLE "app"."deck_entry" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."deck_entry" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."deck_entry" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."deck_entry" TO service_role;

ALTER TABLE "app"."asset_definition" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."asset_definition" ("printing_id");

ALTER TABLE "app"."asset_definition" ADD FOREIGN KEY ("recipe_id") REFERENCES "app"."booster_recipe" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."asset_definition" ("recipe_id");

ALTER TABLE "app"."asset_definition" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."asset_definition" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."asset_definition" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."asset_definition" TO service_role;

ALTER TABLE "app"."inventory_asset" ADD FOREIGN KEY ("owner_id") REFERENCES "app"."asset_owner" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."inventory_asset" ("owner_id");

ALTER TABLE "app"."inventory_asset" ADD FOREIGN KEY ("asset_definition_id") REFERENCES "app"."asset_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."inventory_asset" ("asset_definition_id");

ALTER TABLE "app"."inventory_asset" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."inventory_asset" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."inventory_asset" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."inventory_asset" TO service_role;

ALTER TABLE "app"."ledger_operation" ADD FOREIGN KEY ("actor_account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ledger_operation" ("actor_account_id");

ALTER TABLE "app"."ledger_operation" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ledger_operation" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ledger_operation" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ledger_operation" TO service_role;

ALTER TABLE "app"."ledger_posting" ADD FOREIGN KEY ("operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ledger_posting" ("operation_id");

ALTER TABLE "app"."ledger_posting" ADD FOREIGN KEY ("inventory_asset_id") REFERENCES "app"."inventory_asset" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ledger_posting" ("inventory_asset_id");

ALTER TABLE "app"."ledger_posting" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ledger_posting" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ledger_posting" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ledger_posting" TO service_role;

ALTER TABLE "app"."asset_hold" ADD FOREIGN KEY ("owner_id") REFERENCES "app"."asset_owner" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."asset_hold" ("owner_id");

ALTER TABLE "app"."asset_hold" ADD FOREIGN KEY ("operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."asset_hold" ("operation_id");

ALTER TABLE "app"."asset_hold" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."asset_hold" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."asset_hold" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."asset_hold" TO service_role;

ALTER TABLE "app"."hold_line" ADD FOREIGN KEY ("hold_id") REFERENCES "app"."asset_hold" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."hold_line" ("hold_id");

ALTER TABLE "app"."hold_line" ADD FOREIGN KEY ("inventory_asset_id") REFERENCES "app"."inventory_asset" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."hold_line" ("inventory_asset_id");

ALTER TABLE "app"."hold_line" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."hold_line" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."hold_line" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."hold_line" TO service_role;

ALTER TABLE "app"."command_receipt" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."command_receipt" ("account_id");

ALTER TABLE "app"."command_receipt" ADD FOREIGN KEY ("controller_grant_id") REFERENCES "app"."controller_grant" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."command_receipt" ("controller_grant_id");

ALTER TABLE "app"."command_receipt" ADD FOREIGN KEY ("operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."command_receipt" ("operation_id");

ALTER TABLE "app"."command_receipt" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."command_receipt" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."command_receipt" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."command_receipt" TO service_role;

ALTER TABLE "app"."valuation_policy_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."valuation_policy_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."valuation_policy_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."valuation_policy_revision" TO service_role;

ALTER TABLE "app"."pack_open_receipt" ADD FOREIGN KEY ("inventory_asset_id") REFERENCES "app"."inventory_asset" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."pack_open_receipt" ("inventory_asset_id");

ALTER TABLE "app"."pack_open_receipt" ADD FOREIGN KEY ("recipe_id") REFERENCES "app"."booster_recipe" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."pack_open_receipt" ("recipe_id");

ALTER TABLE "app"."pack_open_receipt" ADD FOREIGN KEY ("operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."pack_open_receipt" ("operation_id");

ALTER TABLE "app"."pack_open_receipt" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."pack_open_receipt" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."pack_open_receipt" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."pack_open_receipt" TO service_role;

ALTER TABLE "app"."friendship" ADD FOREIGN KEY ("account_low_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."friendship" ("account_low_id");

ALTER TABLE "app"."friendship" ADD FOREIGN KEY ("account_high_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."friendship" ("account_high_id");

ALTER TABLE "app"."friendship" ADD FOREIGN KEY ("requested_by_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."friendship" ("requested_by_id");

ALTER TABLE "app"."friendship" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."friendship" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."friendship" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."friendship" TO service_role;

ALTER TABLE "app"."block" ADD FOREIGN KEY ("blocker_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."block" ("blocker_id");

ALTER TABLE "app"."block" ADD FOREIGN KEY ("blocked_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."block" ("blocked_id");

ALTER TABLE "app"."block" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."block" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."block" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."block" TO service_role;

ALTER TABLE "app"."conversation" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."conversation" ("match_id");

ALTER TABLE "app"."conversation" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."conversation" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."conversation" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."conversation" TO service_role;

ALTER TABLE "app"."conversation_member" ADD FOREIGN KEY ("conversation_id") REFERENCES "app"."conversation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."conversation_member" ("conversation_id");

ALTER TABLE "app"."conversation_member" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."conversation_member" ("account_id");

ALTER TABLE "app"."conversation_member" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."conversation_member" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."conversation_member" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."conversation_member" TO service_role;

ALTER TABLE "app"."message" ADD FOREIGN KEY ("conversation_id") REFERENCES "app"."conversation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."message" ("conversation_id");

ALTER TABLE "app"."message" ADD FOREIGN KEY ("sender_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."message" ("sender_id");

ALTER TABLE "app"."message" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."message" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."message" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."message" TO service_role;

ALTER TABLE "app"."notification" ADD FOREIGN KEY ("recipient_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."notification" ("recipient_id");

ALTER TABLE "app"."notification" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."notification" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."notification" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."notification" TO service_role;

ALTER TABLE "app"."trade_offer" ADD FOREIGN KEY ("sender_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_offer" ("sender_id");

ALTER TABLE "app"."trade_offer" ADD FOREIGN KEY ("recipient_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_offer" ("recipient_id");

ALTER TABLE "app"."trade_offer" ADD FOREIGN KEY ("sender_hold_id") REFERENCES "app"."asset_hold" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_offer" ("sender_hold_id");

ALTER TABLE "app"."trade_offer" ADD FOREIGN KEY ("settlement_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_offer" ("settlement_operation_id");

ALTER TABLE "app"."trade_offer" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."trade_offer" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."trade_offer" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."trade_offer" TO service_role;

ALTER TABLE "app"."trade_line" ADD FOREIGN KEY ("trade_id") REFERENCES "app"."trade_offer" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_line" ("trade_id");

ALTER TABLE "app"."trade_line" ADD FOREIGN KEY ("asset_definition_id") REFERENCES "app"."asset_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."trade_line" ("asset_definition_id");

ALTER TABLE "app"."trade_line" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."trade_line" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."trade_line" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."trade_line" TO service_role;

ALTER TABLE "app"."market_listing" ADD FOREIGN KEY ("seller_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."market_listing" ("seller_id");

ALTER TABLE "app"."market_listing" ADD FOREIGN KEY ("asset_definition_id") REFERENCES "app"."asset_definition" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."market_listing" ("asset_definition_id");

ALTER TABLE "app"."market_listing" ADD FOREIGN KEY ("hold_id") REFERENCES "app"."asset_hold" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."market_listing" ("hold_id");

ALTER TABLE "app"."market_listing" ADD FOREIGN KEY ("buyer_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."market_listing" ("buyer_id");

ALTER TABLE "app"."market_listing" ADD FOREIGN KEY ("settlement_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."market_listing" ("settlement_operation_id");

ALTER TABLE "app"."market_listing" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."market_listing" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."market_listing" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."market_listing" TO service_role;

ALTER TABLE "app"."prediction_market" ADD FOREIGN KEY ("creator_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_market" ("creator_id");

ALTER TABLE "app"."prediction_market" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_market" ("printing_id");

ALTER TABLE "app"."prediction_market" ADD FOREIGN KEY ("settlement_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_market" ("settlement_operation_id");

ALTER TABLE "app"."prediction_market" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."prediction_market" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."prediction_market" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."prediction_market" TO service_role;

ALTER TABLE "app"."prediction_order" ADD FOREIGN KEY ("market_id") REFERENCES "app"."prediction_market" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_order" ("market_id");

ALTER TABLE "app"."prediction_order" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_order" ("account_id");

ALTER TABLE "app"."prediction_order" ADD FOREIGN KEY ("hold_id") REFERENCES "app"."asset_hold" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_order" ("hold_id");

ALTER TABLE "app"."prediction_order" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."prediction_order" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."prediction_order" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."prediction_order" TO service_role;

ALTER TABLE "app"."prediction_pair" ADD FOREIGN KEY ("market_id") REFERENCES "app"."prediction_market" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_pair" ("market_id");

ALTER TABLE "app"."prediction_pair" ADD FOREIGN KEY ("yes_order_id") REFERENCES "app"."prediction_order" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_pair" ("yes_order_id");

ALTER TABLE "app"."prediction_pair" ADD FOREIGN KEY ("no_order_id") REFERENCES "app"."prediction_order" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_pair" ("no_order_id");

ALTER TABLE "app"."prediction_pair" ADD FOREIGN KEY ("match_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."prediction_pair" ("match_operation_id");

ALTER TABLE "app"."prediction_pair" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."prediction_pair" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."prediction_pair" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."prediction_pair" TO service_role;

ALTER TABLE "app"."price_observation" ADD FOREIGN KEY ("market_id") REFERENCES "app"."prediction_market" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."price_observation" ("market_id");

ALTER TABLE "app"."price_observation" ADD FOREIGN KEY ("printing_id") REFERENCES "app"."printing" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."price_observation" ("printing_id");

ALTER TABLE "app"."price_observation" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."price_observation" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."price_observation" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."price_observation" TO service_role;

ALTER TABLE "app"."room" ADD FOREIGN KEY ("host_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room" ("host_id");

ALTER TABLE "app"."room" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room" ("format_id");

ALTER TABLE "app"."room" ADD FOREIGN KEY ("profile_id") REFERENCES "app"."table_profile_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room" ("profile_id");

ALTER TABLE "app"."room" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."room" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."room" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."room" TO service_role;

ALTER TABLE "app"."room_slot" ADD FOREIGN KEY ("room_id") REFERENCES "app"."room" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room_slot" ("room_id");

ALTER TABLE "app"."room_slot" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room_slot" ("account_id");

ALTER TABLE "app"."room_slot" ADD FOREIGN KEY ("sponsor_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room_slot" ("sponsor_id");

ALTER TABLE "app"."room_slot" ADD FOREIGN KEY ("deck_revision_id") REFERENCES "app"."deck_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."room_slot" ("deck_revision_id");

ALTER TABLE "app"."room_slot" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."room_slot" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."room_slot" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."room_slot" TO service_role;

ALTER TABLE "app"."invitation" ADD FOREIGN KEY ("room_id") REFERENCES "app"."room" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."invitation" ("room_id");

ALTER TABLE "app"."invitation" ADD FOREIGN KEY ("issuer_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."invitation" ("issuer_id");

ALTER TABLE "app"."invitation" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."invitation" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."invitation" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."invitation" TO service_role;

ALTER TABLE "app"."queue_entry" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."queue_entry" ("account_id");

ALTER TABLE "app"."queue_entry" ADD FOREIGN KEY ("deck_revision_id") REFERENCES "app"."deck_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."queue_entry" ("deck_revision_id");

ALTER TABLE "app"."queue_entry" ADD FOREIGN KEY ("reserved_match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."queue_entry" ("reserved_match_id");

ALTER TABLE "app"."queue_entry" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."queue_entry" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."queue_entry" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."queue_entry" TO service_role;

ALTER TABLE "app"."match" ADD FOREIGN KEY ("room_id") REFERENCES "app"."room" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match" ("room_id");

ALTER TABLE "app"."match" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match" ("format_id");

ALTER TABLE "app"."match" ADD FOREIGN KEY ("profile_id") REFERENCES "app"."table_profile_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match" ("profile_id");

ALTER TABLE "app"."match" ADD FOREIGN KEY ("legality_revision_id") REFERENCES "app"."legality_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match" ("legality_revision_id");

ALTER TABLE "app"."match" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."match" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."match" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."match" TO service_role;

ALTER TABLE "app"."match_seat" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_seat" ("match_id");

ALTER TABLE "app"."match_seat" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_seat" ("account_id");

ALTER TABLE "app"."match_seat" ADD FOREIGN KEY ("sponsor_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_seat" ("sponsor_id");

ALTER TABLE "app"."match_seat" ADD FOREIGN KEY ("source_revision_id") REFERENCES "app"."deck_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_seat" ("source_revision_id");

ALTER TABLE "app"."match_seat" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."match_seat" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."match_seat" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."match_seat" TO service_role;

ALTER TABLE "app"."match_clock" ADD FOREIGN KEY ("match_seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_clock" ("match_seat_id");

ALTER TABLE "app"."match_clock" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."match_clock" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."match_clock" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."match_clock" TO service_role;

ALTER TABLE "app"."between_game_decision" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."between_game_decision" ("match_id");

ALTER TABLE "app"."between_game_decision" ADD FOREIGN KEY ("seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."between_game_decision" ("seat_id");

ALTER TABLE "app"."between_game_decision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."between_game_decision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."between_game_decision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."between_game_decision" TO service_role;

ALTER TABLE "app"."match_result" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_result" ("match_id");

ALTER TABLE "app"."match_result" ADD FOREIGN KEY ("ranking_format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."match_result" ("ranking_format_id");

ALTER TABLE "app"."match_result" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."match_result" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."match_result" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."match_result" TO service_role;

ALTER TABLE "app"."game" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game" ("match_id");

ALTER TABLE "app"."game" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game" TO service_role;

ALTER TABLE "app"."game_context" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_context" ("game_id");

ALTER TABLE "app"."game_context" ADD FOREIGN KEY ("release_id") REFERENCES "app"."qualified_release" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_context" ("release_id");

ALTER TABLE "app"."game_context" ADD FOREIGN KEY ("profile_id") REFERENCES "app"."table_profile_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_context" ("profile_id");

ALTER TABLE "app"."game_context" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_context" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_context" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_context" TO service_role;

ALTER TABLE "app"."game_checkpoint" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_checkpoint" ("game_id");

ALTER TABLE "app"."game_checkpoint" ADD FOREIGN KEY ("context_id") REFERENCES "app"."game_context" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_checkpoint" ("context_id");

ALTER TABLE "app"."game_checkpoint" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_checkpoint" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_checkpoint" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_checkpoint" TO service_role;

ALTER TABLE "app"."game_event" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_event" ("game_id");

ALTER TABLE "app"."game_event" ADD FOREIGN KEY ("context_id") REFERENCES "app"."game_context" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_event" ("context_id");

ALTER TABLE "app"."game_event" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_event" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_event" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_event" TO service_role;

ALTER TABLE "app"."game_result" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_result" ("game_id");

ALTER TABLE "app"."game_result" ADD FOREIGN KEY ("source_event_id") REFERENCES "app"."game_event" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_result" ("source_event_id");

ALTER TABLE "app"."game_result" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_result" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_result" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_result" TO service_role;

ALTER TABLE "app"."operational_state" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."operational_state" ("game_id");

ALTER TABLE "app"."operational_state" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."operational_state" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."operational_state" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."operational_state" TO service_role;

ALTER TABLE "app"."durable_handoff" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."durable_handoff" ("match_id");

ALTER TABLE "app"."durable_handoff" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."durable_handoff" ("game_id");

ALTER TABLE "app"."durable_handoff" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."durable_handoff" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."durable_handoff" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."durable_handoff" TO service_role;

ALTER TABLE "app"."replay_manifest" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."replay_manifest" ("game_id");

ALTER TABLE "app"."replay_manifest" ADD FOREIGN KEY ("context_id") REFERENCES "app"."game_context" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."replay_manifest" ("context_id");

ALTER TABLE "app"."replay_manifest" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."replay_manifest" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."replay_manifest" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."replay_manifest" TO service_role;

ALTER TABLE "app"."ai_runner_binding" ADD FOREIGN KEY ("room_slot_id") REFERENCES "app"."room_slot" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_runner_binding" ("room_slot_id");

ALTER TABLE "app"."ai_runner_binding" ADD FOREIGN KEY ("match_seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_runner_binding" ("match_seat_id");

ALTER TABLE "app"."ai_runner_binding" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ai_runner_binding" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ai_runner_binding" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ai_runner_binding" TO service_role;

ALTER TABLE "app"."controller_grant" ADD FOREIGN KEY ("binding_id") REFERENCES "app"."ai_runner_binding" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."controller_grant" ("binding_id");

ALTER TABLE "app"."controller_grant" ADD FOREIGN KEY ("match_seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."controller_grant" ("match_seat_id");

ALTER TABLE "app"."controller_grant" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."controller_grant" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."controller_grant" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."controller_grant" TO service_role;

ALTER TABLE "app"."ai_loss_bucket" ADD FOREIGN KEY ("sponsor_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_loss_bucket" ("sponsor_id");

ALTER TABLE "app"."ai_loss_bucket" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ai_loss_bucket" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ai_loss_bucket" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ai_loss_bucket" TO service_role;

ALTER TABLE "app"."ai_loss_exposure" ADD FOREIGN KEY ("bucket_id") REFERENCES "app"."ai_loss_bucket" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_loss_exposure" ("bucket_id");

ALTER TABLE "app"."ai_loss_exposure" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_loss_exposure" ("match_id");

ALTER TABLE "app"."ai_loss_exposure" ADD FOREIGN KEY ("commitment_id") REFERENCES "app"."ante_commitment" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ai_loss_exposure" ("commitment_id");

ALTER TABLE "app"."ai_loss_exposure" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ai_loss_exposure" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ai_loss_exposure" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ai_loss_exposure" TO service_role;

ALTER TABLE "app"."ante_commitment" ADD FOREIGN KEY ("match_id") REFERENCES "app"."match" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_commitment" ("match_id");

ALTER TABLE "app"."ante_commitment" ADD FOREIGN KEY ("reservation_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_commitment" ("reservation_operation_id");

ALTER TABLE "app"."ante_commitment" ADD FOREIGN KEY ("settlement_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_commitment" ("settlement_operation_id");

ALTER TABLE "app"."ante_commitment" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ante_commitment" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ante_commitment" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ante_commitment" TO service_role;

ALTER TABLE "app"."ante_stake" ADD FOREIGN KEY ("commitment_id") REFERENCES "app"."ante_commitment" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_stake" ("commitment_id");

ALTER TABLE "app"."ante_stake" ADD FOREIGN KEY ("match_seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_stake" ("match_seat_id");

ALTER TABLE "app"."ante_stake" ADD FOREIGN KEY ("hold_id") REFERENCES "app"."asset_hold" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_stake" ("hold_id");

ALTER TABLE "app"."ante_stake" ADD FOREIGN KEY ("valuation_policy_id") REFERENCES "app"."valuation_policy_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_stake" ("valuation_policy_id");

ALTER TABLE "app"."ante_stake" ADD FOREIGN KEY ("ai_exposure_id") REFERENCES "app"."ai_loss_exposure" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."ante_stake" ("ai_exposure_id");

ALTER TABLE "app"."ante_stake" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."ante_stake" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."ante_stake" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."ante_stake" TO service_role;

ALTER TABLE "app"."game_participation" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_participation" ("game_id");

ALTER TABLE "app"."game_participation" ADD FOREIGN KEY ("match_seat_id") REFERENCES "app"."match_seat" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_participation" ("match_seat_id");

ALTER TABLE "app"."game_participation" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_participation" ("account_id");

ALTER TABLE "app"."game_participation" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_participation" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_participation" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_participation" TO service_role;

ALTER TABLE "app"."game_activity_fact" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_activity_fact" ("game_id");

ALTER TABLE "app"."game_activity_fact" ADD FOREIGN KEY ("source_event_id") REFERENCES "app"."game_event" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_activity_fact" ("source_event_id");

ALTER TABLE "app"."game_activity_fact" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."game_activity_fact" ("account_id");

ALTER TABLE "app"."game_activity_fact" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."game_activity_fact" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."game_activity_fact" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."game_activity_fact" TO service_role;

ALTER TABLE "app"."activity_consumer_cursor" ADD FOREIGN KEY ("game_id") REFERENCES "app"."game" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."activity_consumer_cursor" ("game_id");

ALTER TABLE "app"."activity_consumer_cursor" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."activity_consumer_cursor" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."activity_consumer_cursor" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."activity_consumer_cursor" TO service_role;

ALTER TABLE "app"."season" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."season" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."season" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."season" TO service_role;

ALTER TABLE "app"."set_ballot" ADD FOREIGN KEY ("season_id") REFERENCES "app"."season" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."set_ballot" ("season_id");

ALTER TABLE "app"."set_ballot" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."set_ballot" ("account_id");

ALTER TABLE "app"."set_ballot" ADD FOREIGN KEY ("set_id") REFERENCES "app"."card_set" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."set_ballot" ("set_id");

ALTER TABLE "app"."set_ballot" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."set_ballot" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."set_ballot" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."set_ballot" TO service_role;

ALTER TABLE "app"."monthly_account_activity" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."monthly_account_activity" ("account_id");

ALTER TABLE "app"."monthly_account_activity" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."monthly_account_activity" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."monthly_account_activity" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."monthly_account_activity" TO service_role;

ALTER TABLE "app"."supply_drop" ADD FOREIGN KEY ("season_id") REFERENCES "app"."season" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_drop" ("season_id");

ALTER TABLE "app"."supply_drop" ADD FOREIGN KEY ("set_id") REFERENCES "app"."card_set" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_drop" ("set_id");

ALTER TABLE "app"."supply_drop" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."supply_drop" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."supply_drop" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."supply_drop" TO service_role;

ALTER TABLE "app"."supply_pricing_policy" ADD FOREIGN KEY ("product_id") REFERENCES "app"."booster_product" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_pricing_policy" ("product_id");

ALTER TABLE "app"."supply_pricing_policy" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."supply_pricing_policy" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."supply_pricing_policy" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."supply_pricing_policy" TO service_role;

ALTER TABLE "app"."supply_stock" ADD FOREIGN KEY ("recipe_id") REFERENCES "app"."booster_recipe" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_stock" ("recipe_id");

ALTER TABLE "app"."supply_stock" ADD FOREIGN KEY ("drop_id") REFERENCES "app"."supply_drop" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_stock" ("drop_id");

ALTER TABLE "app"."supply_stock" ADD FOREIGN KEY ("pricing_policy_id") REFERENCES "app"."supply_pricing_policy" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_stock" ("pricing_policy_id");

ALTER TABLE "app"."supply_stock" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."supply_stock" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."supply_stock" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."supply_stock" TO service_role;

ALTER TABLE "app"."supply_purchase" ADD FOREIGN KEY ("stock_id") REFERENCES "app"."supply_stock" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_purchase" ("stock_id");

ALTER TABLE "app"."supply_purchase" ADD FOREIGN KEY ("pricing_policy_id") REFERENCES "app"."supply_pricing_policy" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_purchase" ("pricing_policy_id");

ALTER TABLE "app"."supply_purchase" ADD FOREIGN KEY ("buyer_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_purchase" ("buyer_id");

ALTER TABLE "app"."supply_purchase" ADD FOREIGN KEY ("operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."supply_purchase" ("operation_id");

ALTER TABLE "app"."supply_purchase" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."supply_purchase" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."supply_purchase" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."supply_purchase" TO service_role;

ALTER TABLE "app"."quest_definition_revision" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."quest_definition_revision" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."quest_definition_revision" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."quest_definition_revision" TO service_role;

ALTER TABLE "app"."quest_assignment" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."quest_assignment" ("account_id");

ALTER TABLE "app"."quest_assignment" ADD FOREIGN KEY ("definition_id") REFERENCES "app"."quest_definition_revision" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."quest_assignment" ("definition_id");

ALTER TABLE "app"."quest_assignment" ADD FOREIGN KEY ("replaced_by_id") REFERENCES "app"."quest_assignment" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."quest_assignment" ("replaced_by_id");

ALTER TABLE "app"."quest_assignment" ADD FOREIGN KEY ("reward_operation_id") REFERENCES "app"."ledger_operation" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."quest_assignment" ("reward_operation_id");

ALTER TABLE "app"."quest_assignment" ADD FOREIGN KEY ("reward_source_fact_id") REFERENCES "app"."game_activity_fact" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."quest_assignment" ("reward_source_fact_id");

ALTER TABLE "app"."quest_assignment" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."quest_assignment" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."quest_assignment" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."quest_assignment" TO service_role;

ALTER TABLE "app"."daily_quest_state" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."daily_quest_state" ("account_id");

ALTER TABLE "app"."daily_quest_state" ADD FOREIGN KEY ("generated_assignment_id") REFERENCES "app"."quest_assignment" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."daily_quest_state" ("generated_assignment_id");

ALTER TABLE "app"."daily_quest_state" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."daily_quest_state" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."daily_quest_state" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."daily_quest_state" TO service_role;

ALTER TABLE "app"."competitive_result" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."competitive_result" ("account_id");

ALTER TABLE "app"."competitive_result" ADD FOREIGN KEY ("match_result_id") REFERENCES "app"."match_result" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."competitive_result" ("match_result_id");

ALTER TABLE "app"."competitive_result" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."competitive_result" ("format_id");

ALTER TABLE "app"."competitive_result" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."competitive_result" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."competitive_result" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."competitive_result" TO service_role;

ALTER TABLE "app"."rank" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."rank" ("account_id");

ALTER TABLE "app"."rank" ADD FOREIGN KEY ("format_id") REFERENCES "app"."format" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."rank" ("format_id");

ALTER TABLE "app"."rank" ADD FOREIGN KEY ("season_id") REFERENCES "app"."season" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."rank" ("season_id");

ALTER TABLE "app"."rank" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."rank" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."rank" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."rank" TO service_role;

ALTER TABLE "app"."daily_win_bucket" ADD FOREIGN KEY ("account_id") REFERENCES "app"."account" (id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX ON "app"."daily_win_bucket" ("account_id");

ALTER TABLE "app"."daily_win_bucket" ENABLE ROW LEVEL SECURITY;

ALTER TABLE "app"."daily_win_bucket" FORCE ROW LEVEL SECURITY;

REVOKE ALL ON "app"."daily_win_bucket" FROM PUBLIC, anon, authenticated;

GRANT SELECT, INSERT, UPDATE, DELETE ON "app"."daily_win_bucket" TO service_role;

ALTER DEFAULT PRIVILEGES IN SCHEMA app REVOKE ALL ON TABLES FROM PUBLIC, anon, authenticated;

ALTER DEFAULT PRIVILEGES IN SCHEMA app REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC, anon, authenticated;

COMMIT;
