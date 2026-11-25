-- Stage 1.1 & 1.2: Additive custom-card tables, RLS, triggers, and Open Play isolation invariants.
-- Strictly additive: zero changes to existing read RPC payload shapes so live deployed
-- application_api binaries continue working unchanged prior to Phase 3 read enablement.
BEGIN;

-- ---------------------------------------------------------------------------
-- 1. Custom Card Vault, Revisions, Revision Art, Deck Custom Entries & Match Reveals
-- ---------------------------------------------------------------------------

CREATE TABLE app.custom_card (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  account_id uuid NOT NULL REFERENCES app.account(id) ON DELETE RESTRICT,
  current_revision_id uuid NULL,
  archived_at timestamptz NULL CHECK (archived_at IS NULL OR isfinite(archived_at)),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX custom_card_owner_active_idx
  ON app.custom_card (account_id, updated_at DESC, id DESC)
  WHERE archived_at IS NULL;

CREATE INDEX custom_card_owner_all_idx
  ON app.custom_card (account_id, id);

CREATE TABLE app.custom_card_revision (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  custom_card_id uuid NOT NULL REFERENCES app.custom_card(id) ON DELETE RESTRICT,
  revision_number numeric(78,0) NOT NULL CHECK (revision_number > 0),
  rules_sha256 text NOT NULL CHECK (rules_sha256 ~ '^[0-9a-f]{64}$'),
  name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
  layout text NOT NULL CHECK (layout IN ('normal', 'transform', 'modal_dfc', 'adventure', 'split', 'planeswalker', 'saga')),
  rarity text NOT NULL CHECK (rarity IN ('common', 'uncommon', 'rare', 'mythic')),
  faces_rules_json jsonb NOT NULL CHECK (
    jsonb_typeof(faces_rules_json) = 'array'
    AND jsonb_array_length(faces_rules_json) BETWEEN 1 AND 2
  ),
  visual_frame_accents_json jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(visual_frame_accents_json) = 'array'),
  flavor_texts_json jsonb NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(flavor_texts_json) = 'array'),
  collector_note text NULL CHECK (collector_note IS NULL OR length(collector_note) <= 120),
  stage1_parsed boolean NOT NULL,
  stage2_playable boolean NOT NULL,
  compiler_revision text NOT NULL CHECK (length(btrim(compiler_revision)) BETWEEN 1 AND 64),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  UNIQUE (custom_card_id, revision_number)
);

CREATE INDEX custom_card_revision_card_idx
  ON app.custom_card_revision (custom_card_id, revision_number DESC);

ALTER TABLE app.custom_card
  ADD CONSTRAINT custom_card_current_revision_fkey
  FOREIGN KEY (current_revision_id)
  REFERENCES app.custom_card_revision(id)
  ON DELETE RESTRICT
  DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE app.custom_card_revision_art (
  revision_id uuid NOT NULL REFERENCES app.custom_card_revision(id) ON DELETE RESTRICT,
  face_index smallint NOT NULL CHECK (face_index BETWEEN 0 AND 1),
  mime_type text NOT NULL CHECK (mime_type IN ('image/webp', 'image/png')),
  art_bytes bytea NULL CHECK (art_bytes IS NULL OR octet_length(art_bytes) BETWEEN 1 AND 131072),
  art_sha256 text NOT NULL CHECK (art_sha256 ~ '^[0-9a-f]{64}$'),
  crop_x double precision NOT NULL DEFAULT 0 CHECK (crop_x >= -1 AND crop_x <= 1),
  crop_y double precision NOT NULL DEFAULT 0 CHECK (crop_y >= -1 AND crop_y <= 1),
  zoom double precision NOT NULL DEFAULT 1 CHECK (zoom >= 0.5 AND zoom <= 4),
  artist text NULL CHECK (artist IS NULL OR length(artist) <= 80),
  PRIMARY KEY (revision_id, face_index)
);

CREATE TABLE app.deck_custom_entry (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  revision_id uuid NOT NULL REFERENCES app.deck_revision(id) ON DELETE RESTRICT,
  custom_card_revision_id uuid NOT NULL REFERENCES app.custom_card_revision(id) ON DELETE RESTRICT,
  section text NOT NULL CHECK (section ~ '^[a-z][a-z0-9_]{0,31}$'),
  quantity numeric(78,0) NOT NULL CHECK (quantity > 0),
  UNIQUE (revision_id, custom_card_revision_id, section)
);

CREATE INDEX deck_custom_entry_revision_idx
  ON app.deck_custom_entry (revision_id, id);

CREATE INDEX deck_custom_entry_card_rev_idx
  ON app.deck_custom_entry (custom_card_revision_id);

CREATE TABLE app.match_revealed_custom_card (
  game_id uuid NOT NULL REFERENCES app.game(id) ON DELETE RESTRICT,
  context_id uuid NOT NULL REFERENCES app.game_context(id) ON DELETE RESTRICT,
  seat_index smallint NOT NULL CHECK (seat_index BETWEEN 0 AND 5),
  custom_card_revision_id uuid NOT NULL REFERENCES app.custom_card_revision(id) ON DELETE RESTRICT,
  revealed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (game_id, context_id, seat_index, custom_card_revision_id)
);

-- ---------------------------------------------------------------------------
-- 2. Immutability, Ownership, 250-Cap, and Scoped Session-Flag Triggers
-- ---------------------------------------------------------------------------

CREATE FUNCTION app.guard_custom_card() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  active_count bigint;
BEGIN
  IF TG_OP = 'UPDATE' THEN
    IF (NEW.id, NEW.account_id, NEW.created_at) IS DISTINCT FROM (OLD.id, OLD.account_id, OLD.created_at) THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card identity and owner are immutable';
    END IF;
  END IF;

  IF NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM app.custom_card_revision r
    WHERE r.id = NEW.current_revision_id AND r.custom_card_id = NEW.id
  ) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'current custom card revision must belong to this custom card';
  END IF;

  IF NEW.archived_at IS NULL AND (TG_OP = 'INSERT' OR OLD.archived_at IS NOT NULL) THEN
    PERFORM id FROM app.account WHERE id = NEW.account_id FOR NO KEY UPDATE;
    SELECT count(*) INTO active_count
    FROM app.custom_card
    WHERE account_id = NEW.account_id
      AND archived_at IS NULL
      AND id <> NEW.id;
    IF active_count >= 250 THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card limit of 250 active cards reached';
    END IF;
  END IF;

  RETURN NEW;
END $$;

CREATE TRIGGER custom_card_guard
  BEFORE INSERT OR UPDATE ON app.custom_card
  FOR EACH ROW EXECUTE FUNCTION app.guard_custom_card();

CREATE FUNCTION app.guard_custom_card_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
BEGIN
  IF TG_OP = 'UPDATE'
     AND current_setting('spellwake.retiring_account', true) = 'on'
     AND (
       NEW.id, NEW.custom_card_id, NEW.revision_number, NEW.rules_sha256,
       NEW.name, NEW.layout, NEW.rarity, NEW.faces_rules_json,
       NEW.visual_frame_accents_json, NEW.stage1_parsed, NEW.stage2_playable,
       NEW.compiler_revision, NEW.created_at
     ) IS NOT DISTINCT FROM (
       OLD.id, OLD.custom_card_id, OLD.revision_number, OLD.rules_sha256,
       OLD.name, OLD.layout, OLD.rarity, OLD.faces_rules_json,
       OLD.visual_frame_accents_json, OLD.stage1_parsed, OLD.stage2_playable,
       OLD.compiler_revision, OLD.created_at
     )
     AND NEW.flavor_texts_json = '[]'::jsonb
     AND NEW.collector_note IS NULL THEN
    RETURN NEW;
  END IF;

  IF TG_OP = 'DELETE'
     AND current_setting('spellwake.deleting_unreferenced_custom_card', true) = 'on' THEN
    RETURN OLD;
  END IF;

  RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card revision is immutable';
END $$;

CREATE TRIGGER custom_card_revision_immutable
  BEFORE UPDATE OR DELETE ON app.custom_card_revision
  FOR EACH ROW EXECUTE FUNCTION app.guard_custom_card_revision();

CREATE FUNCTION app.guard_custom_card_revision_art() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
BEGIN
  IF TG_OP = 'INSERT' THEN
    IF NEW.art_bytes IS NULL THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card revision art bytes required on insert';
    END IF;
    RETURN NEW;
  END IF;

  IF TG_OP = 'UPDATE'
     AND current_setting('spellwake.retiring_account', true) = 'on'
     AND (
       NEW.revision_id, NEW.face_index, NEW.mime_type, NEW.art_sha256,
       NEW.crop_x, NEW.crop_y, NEW.zoom
     ) IS NOT DISTINCT FROM (
       OLD.revision_id, OLD.face_index, OLD.mime_type, OLD.art_sha256,
       OLD.crop_x, OLD.crop_y, OLD.zoom
     )
     AND NEW.art_bytes IS NULL
     AND NEW.artist IS NULL THEN
    RETURN NEW;
  END IF;

  IF TG_OP = 'DELETE'
     AND current_setting('spellwake.deleting_unreferenced_custom_card', true) = 'on' THEN
    RETURN OLD;
  END IF;

  RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card revision art is immutable';
END $$;

CREATE TRIGGER custom_card_revision_art_immutable
  BEFORE INSERT OR UPDATE OR DELETE ON app.custom_card_revision_art
  FOR EACH ROW EXECUTE FUNCTION app.guard_custom_card_revision_art();

CREATE FUNCTION app.guard_deck_custom_entry() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE
  rev_sealed timestamptz;
  parent_deck app.deck;
  card_owner uuid;
BEGIN
  IF TG_OP = 'DELETE' THEN
    IF current_setting('spellwake.deleting_unreferenced_deck', true) = 'on' THEN
      RETURN OLD;
    END IF;
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck custom entry is immutable';
  END IF;

  IF TG_OP = 'UPDATE' THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck custom entry is immutable';
  END IF;

  SELECT r.sealed_at, d.*
  INTO rev_sealed, parent_deck
  FROM app.deck_revision r
  JOIN app.deck d ON d.id = r.deck_id
  WHERE r.id = NEW.revision_id
  FOR NO KEY UPDATE OF r;

  IF NOT FOUND OR rev_sealed IS NOT NULL THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'deck revision does not accept custom entries';
  END IF;

  IF parent_deck.source_kind <> 'account' OR parent_deck.mode <> 'open' THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom cards are restricted to open play account decks';
  END IF;

  SELECT c.account_id INTO card_owner
  FROM app.custom_card_revision cr
  JOIN app.custom_card c ON c.id = cr.custom_card_id
  WHERE cr.id = NEW.custom_card_revision_id;

  IF card_owner IS NULL OR card_owner IS DISTINCT FROM parent_deck.account_id THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card revision must belong to deck owner';
  END IF;

  RETURN NEW;
END $$;

CREATE TRIGGER deck_custom_entry_history
  BEFORE INSERT OR UPDATE OR DELETE ON app.deck_custom_entry
  FOR EACH ROW EXECUTE FUNCTION app.guard_deck_custom_entry();

CREATE FUNCTION app.guard_match_revealed_custom_card() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
BEGIN
  RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'match revealed custom card proof is append-only';
END $$;

CREATE TRIGGER match_revealed_custom_card_append_only
  BEFORE UPDATE OR DELETE ON app.match_revealed_custom_card
  FOR EACH ROW EXECUTE FUNCTION app.guard_match_revealed_custom_card();

-- Add scoped session-flag exemption strictly for unreferenced delete_deck_command
-- on guard_deck_revision and guard_deck_entry (resolving pre-existing 23514 failure).
CREATE OR REPLACE FUNCTION app.guard_deck_revision() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    IF current_setting('spellwake.deleting_unreferenced_deck', true) = 'on' THEN
      RETURN OLD;
    END IF;
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck revision is immutable';
  END IF;

  IF OLD.sealed_at IS NOT NULL OR
     (NEW.id, NEW.deck_id, NEW.revision_number, NEW.created_at) IS DISTINCT FROM
     (OLD.id, OLD.deck_id, OLD.revision_number, OLD.created_at) OR NEW.sealed_at IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck revision is immutable';
  END IF;
  RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION app.guard_deck_entry() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
DECLARE sealed timestamptz;
BEGIN
  IF TG_OP = 'DELETE' THEN
    IF current_setting('spellwake.deleting_unreferenced_deck', true) = 'on' THEN
      RETURN OLD;
    END IF;
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck entry is immutable';
  END IF;

  IF TG_OP <> 'INSERT' THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'saved deck entry is immutable';
  END IF;

  SELECT sealed_at INTO sealed FROM app.deck_revision WHERE id = NEW.revision_id FOR NO KEY UPDATE;
  IF NOT FOUND OR sealed IS NOT NULL THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'deck revision does not accept entries';
  END IF;
  IF NEW.printing_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM app.printing WHERE id = NEW.printing_id AND definition_id = NEW.definition_id
  ) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'printing does not belong to card definition';
  END IF;
  RETURN NEW;
END $$;

-- ---------------------------------------------------------------------------
-- 3. Additive Extensions on app.deck, app.room, and app.match
-- ---------------------------------------------------------------------------

ALTER TABLE app.deck
  ADD COLUMN face_custom_card_revision_id uuid NULL
    REFERENCES app.custom_card_revision(id) ON DELETE RESTRICT DEFERRABLE INITIALLY IMMEDIATE;

CREATE INDEX deck_face_custom_card_revision_idx
  ON app.deck (face_custom_card_revision_id)
  WHERE face_custom_card_revision_id IS NOT NULL;

ALTER TABLE app.deck
  ADD CONSTRAINT deck_face_card_mutual_exclusivity
  CHECK (NOT (face_definition_id IS NOT NULL AND face_custom_card_revision_id IS NOT NULL));

CREATE FUNCTION app.guard_deck_custom_mode() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
BEGIN
  IF NEW.mode <> 'open' AND (
    NEW.face_custom_card_revision_id IS NOT NULL
    OR EXISTS (
      SELECT 1
      FROM app.deck_revision r
      JOIN app.deck_custom_entry ce ON ce.revision_id = r.id
      WHERE r.deck_id = NEW.id
    )
  ) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'custom card decks must remain in open play';
  END IF;
  RETURN NEW;
END $$;

CREATE TRIGGER deck_custom_mode_guard
  BEFORE INSERT OR UPDATE OF mode, face_custom_card_revision_id ON app.deck
  FOR EACH ROW EXECUTE FUNCTION app.guard_deck_custom_mode();

ALTER TABLE app.room
  ADD COLUMN allow_custom_cards boolean NOT NULL DEFAULT false,
  ADD COLUMN share_host_decks boolean NOT NULL DEFAULT false;

ALTER TABLE app.room
  ADD CONSTRAINT room_allow_custom_cards_open_only
    CHECK (NOT allow_custom_cards OR (mode = 'open' AND NOT competitive)),
  ADD CONSTRAINT room_share_host_decks_open_only
    CHECK (NOT share_host_decks OR (mode = 'open' AND NOT competitive));

CREATE FUNCTION app.guard_room_supporter_features() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
BEGIN
  IF (
    (NEW.allow_custom_cards AND (TG_OP = 'INSERT' OR NOT OLD.allow_custom_cards))
    OR (NEW.share_host_decks AND (TG_OP = 'INSERT' OR NOT OLD.share_host_decks))
  ) AND app.active_supporter_tier(NEW.host_id) IS NULL THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'supporter_required';
  END IF;
  RETURN NEW;
END $$;

CREATE TRIGGER room_supporter_features_guard
  BEFORE INSERT OR UPDATE OF allow_custom_cards, share_host_decks, host_id ON app.room
  FOR EACH ROW EXECUTE FUNCTION app.guard_room_supporter_features();

CREATE OR REPLACE FUNCTION app.stamp_room_configuration() RETURNS trigger
LANGUAGE plpgsql SET search_path = '' AS $$
BEGIN
  IF (NEW.profile_id, NEW.structure, NEW.format_id, NEW.mode, NEW.allow_custom_cards, NEW.share_host_decks)
     IS DISTINCT FROM
     (OLD.profile_id, OLD.structure, OLD.format_id, OLD.mode, OLD.allow_custom_cards, OLD.share_host_decks) THEN
    NEW.configuration_id := gen_random_uuid();
  END IF;
  RETURN NEW;
END $$;

ALTER TABLE app.match
  ADD COLUMN allow_custom_cards boolean NOT NULL DEFAULT false,
  ADD COLUMN share_host_decks boolean NOT NULL DEFAULT false,
  ADD COLUMN host_id uuid NULL REFERENCES app.account(id) ON DELETE RESTRICT;

ALTER TABLE app.match
  ADD CONSTRAINT match_allow_custom_cards_open_only
    CHECK (NOT allow_custom_cards OR (mode = 'open' AND NOT competitive)),
  ADD CONSTRAINT match_share_host_decks_open_only
    CHECK (NOT share_host_decks OR (mode = 'open' AND NOT competitive));

CREATE OR REPLACE FUNCTION app.guard_accepted_match_inputs() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = '' AS $$
DECLARE targets uuid[];
BEGIN
  IF TG_TABLE_NAME = 'match' THEN
    IF (NEW.id, NEW.room_id, NEW.format_id, NEW.profile_id, NEW.legality_revision_id, NEW.mode, NEW.competitive, NEW.structure, NEW.allow_custom_cards, NEW.share_host_decks, NEW.host_id) IS NOT DISTINCT FROM
       (OLD.id, OLD.room_id, OLD.format_id, OLD.profile_id, OLD.legality_revision_id, OLD.mode, OLD.competitive, OLD.structure, OLD.allow_custom_cards, OLD.share_host_decks, OLD.host_id) THEN
      RETURN NULL;
    END IF;
    targets := ARRAY[OLD.id, NEW.id];
  ELSE
    IF TG_OP = 'UPDATE' AND (NEW.id, NEW.match_id, NEW.account_id, NEW.sponsor_id, NEW.seat_index, NEW.team_ref, NEW.controller_kind) IS NOT DISTINCT FROM
       (OLD.id, OLD.match_id, OLD.account_id, OLD.sponsor_id, OLD.seat_index, OLD.team_ref, OLD.controller_kind) THEN
      RETURN NULL;
    END IF;
    IF TG_OP <> 'INSERT' AND OLD.deck_accepted_at IS NOT NULL THEN
      RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'accepted match configuration and roster are immutable';
    END IF;
    IF TG_OP = 'INSERT' THEN targets := ARRAY[NEW.match_id];
    ELSIF TG_OP = 'DELETE' THEN targets := ARRAY[OLD.match_id];
    ELSE targets := ARRAY[OLD.match_id, NEW.match_id];
    END IF;
  END IF;
  IF current_setting('transaction_isolation') <> 'read committed' THEN
    RAISE EXCEPTION USING ERRCODE = '40001', MESSAGE = 'accepted match input changes require read committed retry';
  END IF;
  PERFORM id FROM app.match WHERE id = ANY(targets) ORDER BY id FOR NO KEY UPDATE;
  IF EXISTS (SELECT 1 FROM app.match_seat WHERE match_id = ANY(targets) AND deck_accepted_at IS NOT NULL) THEN
    RAISE EXCEPTION USING ERRCODE = '23514', MESSAGE = 'accepted match configuration and roster are immutable';
  END IF;
  RETURN NULL;
END $$;

-- ---------------------------------------------------------------------------
-- 4. Row-Level Security & Least-Privilege Grants
-- ---------------------------------------------------------------------------

ALTER TABLE app.custom_card ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.custom_card FORCE ROW LEVEL SECURITY;
ALTER TABLE app.custom_card_revision ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.custom_card_revision FORCE ROW LEVEL SECURITY;
ALTER TABLE app.custom_card_revision_art ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.custom_card_revision_art FORCE ROW LEVEL SECURITY;
ALTER TABLE app.deck_custom_entry ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.deck_custom_entry FORCE ROW LEVEL SECURITY;
ALTER TABLE app.match_revealed_custom_card ENABLE ROW LEVEL SECURITY;
ALTER TABLE app.match_revealed_custom_card FORCE ROW LEVEL SECURITY;

REVOKE ALL ON
  app.custom_card,
  app.custom_card_revision,
  app.custom_card_revision_art,
  app.deck_custom_entry,
  app.match_revealed_custom_card
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

-- Owner-only SELECT on custom_card, custom_card_revision, and deck_custom_entry.
-- custom_card_revision_art and match_revealed_custom_card stay service-role / SECURITY DEFINER ONLY.
GRANT SELECT ON app.custom_card, app.custom_card_revision, app.deck_custom_entry TO authenticated;

CREATE FUNCTION app.can_read_custom_card(p_card uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = '' AS $$
  SELECT EXISTS (
    SELECT 1 FROM app.custom_card
    WHERE id = p_card AND account_id = app.current_account_id()
  )
$$;

REVOKE ALL ON FUNCTION
  app.guard_custom_card(),
  app.guard_custom_card_revision(),
  app.guard_custom_card_revision_art(),
  app.guard_deck_custom_entry(),
  app.guard_match_revealed_custom_card(),
  app.guard_deck_custom_mode(),
  app.guard_room_supporter_features(),
  app.can_read_custom_card(uuid)
FROM PUBLIC, anon, authenticated, service_role, supabase_auth_admin;

GRANT EXECUTE ON FUNCTION app.can_read_custom_card(uuid) TO authenticated;

CREATE POLICY own_custom_card_read ON app.custom_card
  FOR SELECT TO authenticated
  USING (account_id = app.current_account_id());

CREATE POLICY own_custom_card_revision_read ON app.custom_card_revision
  FOR SELECT TO authenticated
  USING (app.can_read_custom_card(custom_card_id));

CREATE POLICY own_deck_custom_entry_read ON app.deck_custom_entry
  FOR SELECT TO authenticated
  USING (EXISTS (
    SELECT 1 FROM app.deck_revision r
    WHERE r.id = revision_id AND app.can_read_deck(r.deck_id)
  ));

COMMIT;
