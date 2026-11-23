//! Lossless `compact_v1` prompt compiler and bidirectional short-handle codec.
//!
//! Replaces 39-character `n1.<uuid>` handles with zone-typed scope-local handles
//! (`h1`, `b1`, `m1`, `e1`, `g1`, `eg1`, `st1`, `d1`, `opt1`, `you`, `opp`),
//! deduplicates card definitions, orders static instructions first for provider
//! prompt caching, and expands compact LLM replies fail-closed into validated
//! native `PlayerCommand` values.
use super::scheduler::WakeCondition;
use crate::player_wire::{PlayerAction, PlayerBasis, PlayerCommand, PlayerSnapshot};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};
use uuid::Uuid;

/// Static compact action guide placed at the start of every AI prompt so provider
/// prefix caching matches across turns and games.
pub const COMPACT_ACTION_GUIDE: &str = "\
You are playing Magic: The Gathering on Spellwake's native rules engine.\n\
Return one JSON object matching the output schema: {\"action\": ..., \"delegate_pass\": bool, \"wake\": [...], \"max_skips\": int}.\n\
\n\
HANDLE GRAMMAR (use ONLY short handles from this snapshot):\n\
- \"you\" = your seat; \"opp\" (or \"opp1\", \"opp2\", ...) = opponent seat.\n\
- Hand cards: \"h1\", \"h2\", ...; your battlefield: \"b1\", ...; your mana abilities: \"m1\", ...;\n\
  opponent battlefield: \"e1\", ...; graveyards: \"g1\" / \"eg1\"; stack: \"st1\", ...;\n\
  decisions: \"d1\", ...; options: \"opt1\", ...; exile/other: \"x1\", ....\n\
\n\
BOUNDARY RULE (CRITICAL):\n\
- Check \"boundary\" FIRST.\n\
- If \"boundary\" is \"required_decision\" (\"decisions\" is non-empty), you MUST return {\"kind\": \"choose\", \"decision\": \"d1\", \"response\": ...}.\n\
- If \"boundary\" is \"priority\" (\"decisions\" is empty), NEVER return \"choose\". Return \"pass\", \"play-land\", \"activate-mana\", \"cast-untargeted\", \"cast-targeted\", or \"cast-batch\".\n\
\n\
ACTION EXAMPLES:\n\
- Pass: {\"kind\":\"pass\"}\n\
- Play land (ONLY from \"can_play_lands\"): {\"kind\":\"play-land\",\"source\":\"h1\"}\n\
- Tap mana (ONLY from \"can_tap_for_mana\"): {\"kind\":\"activate-mana\",\"source\":\"b1\",\"ability\":\"m1\"}\n\
- Cast with mana in one atomic batch:\n\
  {\"kind\":\"cast-batch\",\"mana\":[{\"kind\":\"activate-mana\",\"source\":\"b1\",\"ability\":\"m1\"}],\"cast\":{\"kind\":\"cast-untargeted\",\"source\":\"h2\",\"payment\":{\"spend\":[{\"color\":\"green\",\"amount\":\"1\"}]}}}\n\
- Targeted cast (creature/permanent target or seat target):\n\
  \"target\":{\"kind\":\"object\",\"handle\":\"e1\"}  or  \"target\":{\"kind\":\"seat\",\"seatId\":\"opp\"}  or  \"target\":{\"kind\":\"stack\",\"handle\":\"st1\"}\n\
- Choose option (mulligan/optional): {\"kind\":\"choose\",\"decision\":\"d1\",\"response\":{\"kind\":\"option\",\"option\":\"opt1\"}}\n\
- Choose objects (bottom/discard/sacrifice): {\"kind\":\"choose\",\"decision\":\"d1\",\"response\":{\"kind\":\"objects\",\"objects\":[\"h1\"]}}\n\
- Declare attackers: {\"kind\":\"choose\",\"decision\":\"d1\",\"response\":{\"kind\":\"attackers\",\"assignments\":[{\"attacker\":\"b3\",\"defender\":{\"kind\":\"seat\",\"seatId\":\"opp\"}}]}}\n\
- Declare blockers: {\"kind\":\"choose\",\"decision\":\"d1\",\"response\":{\"kind\":\"blockers\",\"assignments\":[{\"blocker\":\"b4\",\"attackers\":[\"e2\"]}]}}\n\
- Assign combat damage: {\"kind\":\"choose\",\"decision\":\"d1\",\"response\":{\"kind\":\"combat-damage\",\"assignments\":[{\"source\":\"b3\",\"target\":{\"kind\":\"object\",\"handle\":\"e2\"},\"amount\":\"2\"}]}}\n\
\n\
MAIN-PHASE PRIORITY & DELEGATION RULE:\n\
- When taking a setup play during your main phase (such as \"play-land\" or a first \"cast-batch\") and you still have another affordable spell or play you want to make in the SAME main phase, set \"delegate_pass\": false so you keep priority.\n\
- When you have NO remaining affordable plays in the current phase (for example, Turn 1 land play with no 1-mana plays, or passing with nothing to cast), set \"delegate_pass\": true with appropriate \"wake\" triggers (e.g. [\"own_main\",\"opponent_action\",\"stack_changed\",\"board_changed\"]) and \"max_skips\": 8.\n\
- Check \"can_play_lands\" before playing a land (empty means you cannot play a land now). Check exact mana colors in \"can_tap_for_mana\" and \"mana_pool\" before casting.";

/// Errors produced when compacting a snapshot or expanding an AI reply.
#[derive(Debug)]
pub enum CompactError {
    SerializeSnapshot(serde_json::Error),
    InvalidReplyJson(serde_json::Error),
    InvalidReplyShape(String),
    UnknownHandle(String),
    InvalidNativeCommand(String),
}

impl fmt::Display for CompactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SerializeSnapshot(_) => f.write_str("failed to serialize native snapshot"),
            Self::InvalidReplyJson(_) => f.write_str("AI reply was not valid JSON"),
            Self::InvalidReplyShape(msg) => write!(f, "invalid AI reply shape: {msg}"),
            Self::UnknownHandle(handle) => write!(f, "unknown compact handle: {handle}"),
            Self::InvalidNativeCommand(msg) => {
                write!(f, "expanded command failed validation: {msg}")
            }
        }
    }
}

impl Error for CompactError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SerializeSnapshot(err) | Self::InvalidReplyJson(err) => Some(err),
            _ => None,
        }
    }
}

/// Validated native execution plan expanded from one compact AI reply.
#[derive(Clone)]
pub enum AiActionPlan {
    Single(Box<PlayerCommand>),
    CastBatch(Vec<PlayerCommand>),
}

/// Complete decoded and validated AI reply.
#[derive(Clone)]
pub struct ExpandedAiReply {
    pub plan: AiActionPlan,
    pub delegate_pass: bool,
    pub wake: BTreeSet<WakeCondition>,
    pub max_skips: u32,
}

/// Bidirectional scope-local handle table for one snapshot.
#[derive(Debug, Default, Clone)]
pub struct HandleCodec {
    native_to_short: BTreeMap<String, String>,
    short_to_native: BTreeMap<String, String>,
    counters: BTreeMap<String, usize>,
}

impl HandleCodec {
    /// Allocate or return the existing deterministic short handle for `native` under `prefix`.
    fn alias(&mut self, native: &str, prefix: &str) -> String {
        if let Some(existing) = self.native_to_short.get(native) {
            return existing.clone();
        }
        let next = self.counters.entry(prefix.to_owned()).or_insert(0);
        *next += 1;
        let short = format!("{prefix}{next}");
        self.native_to_short
            .insert(native.to_owned(), short.clone());
        self.short_to_native
            .insert(short.clone(), native.to_owned());
        short
    }

    fn bind_exact(&mut self, native: &str, short: &str) {
        self.native_to_short
            .insert(native.to_owned(), short.to_owned());
        self.short_to_native
            .insert(short.to_owned(), native.to_owned());
    }

    /// Resolve a short handle back to its exact native handle or seat UUID.
    /// # Errors
    /// Returns `CompactError::UnknownHandle` if `short` was not issued in this snapshot.
    pub fn resolve(&self, short: &str) -> Result<&str, CompactError> {
        self.short_to_native
            .get(short)
            .map(String::as_str)
            .ok_or_else(|| CompactError::UnknownHandle(short.to_owned()))
    }

    /// Short alias for a native handle if registered.
    pub fn short_of(&self, native: &str) -> Option<&str> {
        self.native_to_short.get(native).map(String::as_str)
    }

    fn rewrite_value_to_short(&mut self, val: &Value) -> Value {
        match val {
            Value::String(s) => {
                if let Some(short) = self.native_to_short.get(s) {
                    Value::String(short.clone())
                } else if s.starts_with("n1.") {
                    Value::String(self.alias(s, "x"))
                } else {
                    Value::String(s.clone())
                }
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|v| self.rewrite_value_to_short(v))
                    .collect(),
            ),
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), self.rewrite_value_to_short(v));
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }

    fn expand_value_to_native(
        &self,
        val: &Value,
        key_hint: Option<&str>,
    ) -> Result<Value, CompactError> {
        match val {
            Value::String(s) => {
                let is_handle_field = matches!(
                    key_hint,
                    Some(
                        "source"
                            | "ability"
                            | "handle"
                            | "decision"
                            | "option"
                            | "attacker"
                            | "blocker"
                            | "seatId"
                            | "conflict"
                            | "keep"
                            | "object"
                            | "card"
                            | "attempt"
                            | "permanent"
                            | "graveyardPermission"
                            | "alternativeCostSource"
                    )
                );
                if is_handle_field {
                    let native = self.resolve(s)?;
                    Ok(Value::String(native.to_owned()))
                } else {
                    Ok(Value::String(s.clone()))
                }
            }
            Value::Array(items) => {
                let is_handle_list = matches!(
                    key_hint,
                    Some("objects" | "attackers" | "order" | "ordered" | "cards" | "choices")
                );
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    if is_handle_list && let Some(s) = item.as_str() {
                        out.push(Value::String(self.resolve(s)?.to_owned()));
                    } else {
                        out.push(self.expand_value_to_native(item, None)?);
                    }
                }
                Ok(Value::Array(out))
            }
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, v) in map {
                    out.insert(k.clone(), self.expand_value_to_native(v, Some(k.as_str()))?);
                }
                Ok(Value::Object(out))
            }
            other => Ok(other.clone()),
        }
    }

    /// Expand and validate a raw JSON AI reply into a native `ExpandedAiReply`.
    /// # Errors
    /// Fails closed on invalid JSON, unknown handles, missing required fields, or
    /// any `PlayerCommand` that fails `PlayerCommand::validate()`.
    pub fn expand_reply(
        &self,
        raw_json: &str,
        basis: &PlayerBasis,
    ) -> Result<ExpandedAiReply, CompactError> {
        let root: Value = serde_json::from_str(raw_json).map_err(CompactError::InvalidReplyJson)?;
        let obj = root
            .as_object()
            .ok_or_else(|| CompactError::InvalidReplyShape("top-level must be an object".into()))?;
        let action_val = obj
            .get("action")
            .ok_or_else(|| CompactError::InvalidReplyShape("missing action".into()))?;
        let delegate_pass = obj
            .get("delegate_pass")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_skips = obj
            .get("max_skips")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        let mut wake = BTreeSet::new();
        if let Some(arr) = obj.get("wake").and_then(Value::as_array) {
            for item in arr {
                if let Some(s) = item.as_str()
                    && let Some(cond) = WakeCondition::from_wire(s)
                {
                    wake.insert(cond);
                }
            }
        }

        let action_obj = action_val
            .as_object()
            .ok_or_else(|| CompactError::InvalidReplyShape("action must be an object".into()))?;
        let kind = action_obj
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| CompactError::InvalidReplyShape("action missing kind".into()))?;

        let plan = if kind == "cast-batch" {
            let mana_arr = action_obj
                .get("mana")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    CompactError::InvalidReplyShape("cast-batch missing mana array".into())
                })?;
            let cast_val = action_obj
                .get("cast")
                .ok_or_else(|| CompactError::InvalidReplyShape("cast-batch missing cast".into()))?;
            if mana_arr.len() > 8 {
                return Err(CompactError::InvalidReplyShape(
                    "cast-batch supports at most 8 mana activations".into(),
                ));
            }
            let mut commands = Vec::with_capacity(mana_arr.len() + 1);
            for step in mana_arr {
                let expanded = self.expand_value_to_native(step, None)?;
                let action: PlayerAction = serde_json::from_value(expanded)
                    .map_err(|e| CompactError::InvalidNativeCommand(e.to_string()))?;
                let cmd = PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: basis.clone(),
                    action,
                };
                cmd.validate()
                    .map_err(|_| CompactError::InvalidNativeCommand("invalid mana step".into()))?;
                commands.push(cmd);
            }
            let expanded_cast = self.expand_value_to_native(cast_val, None)?;
            let cast_action: PlayerAction = serde_json::from_value(expanded_cast)
                .map_err(|e| CompactError::InvalidNativeCommand(e.to_string()))?;
            let cast_cmd = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: basis.clone(),
                action: cast_action,
            };
            cast_cmd
                .validate()
                .map_err(|_| CompactError::InvalidNativeCommand("invalid cast step".into()))?;
            commands.push(cast_cmd);
            AiActionPlan::CastBatch(commands)
        } else {
            let expanded = self.expand_value_to_native(action_val, None)?;
            let action: PlayerAction = serde_json::from_value(expanded)
                .map_err(|e| CompactError::InvalidNativeCommand(e.to_string()))?;
            let cmd = PlayerCommand {
                request_id: Uuid::new_v4(),
                basis: basis.clone(),
                action,
            };
            cmd.validate()
                .map_err(|_| CompactError::InvalidNativeCommand("invalid player command".into()))?;
            AiActionPlan::Single(Box::new(cmd))
        };

        Ok(ExpandedAiReply {
            plan,
            delegate_pass,
            wake,
            max_skips,
        })
    }
}

/// Compile a seat-filtered `PlayerSnapshot` and verified `basic_actions` into a
/// static-first `compact_v1` prompt string and its lossless `HandleCodec`.
/// # Errors
/// Returns `CompactError::SerializeSnapshot` if serialization fails.
pub fn compact_prompt(
    snapshot: &PlayerSnapshot,
    basic_actions: &[PlayerAction],
    own_recent: &[String],
    feedback: Option<&str>,
) -> Result<(String, HandleCodec), CompactError> {
    let raw = serde_json::to_value(snapshot).map_err(CompactError::SerializeSnapshot)?;
    let mut codec = HandleCodec::default();

    let viewer = snapshot.viewer_seat_id.to_string();
    codec.bind_exact(&viewer, "you");

    let mut opp_ids: Vec<String> = snapshot
        .players
        .iter()
        .map(|p| p.seat_id.to_string())
        .filter(|id| id != &viewer)
        .collect();
    opp_ids.sort();
    opp_ids.dedup();
    if opp_ids.len() == 1 {
        codec.bind_exact(&opp_ids[0], "opp");
    } else {
        for (idx, opp) in opp_ids.iter().enumerate() {
            codec.bind_exact(opp, &format!("opp{}", idx + 1));
        }
    }

    let mut zone_kinds: BTreeMap<String, String> = BTreeMap::new();
    if let Some(zones) = raw.get("zones").and_then(Value::as_array) {
        for z in zones {
            if let (Some(h), Some(zt)) = (
                z.get("handle").and_then(Value::as_str),
                z.get("zoneType").and_then(Value::as_str),
            ) {
                zone_kinds.insert(h.to_owned(), zt.to_owned());
            }
        }
    }

    let mut cards: BTreeMap<String, Value> = BTreeMap::new();
    let mut hand = Vec::new();
    let mut your_board = Vec::new();
    let mut opp_board = Vec::new();
    let mut your_gy = Vec::new();
    let mut opp_gy = Vec::new();
    let mut stack_objs = Vec::new();
    let mut obj_names: BTreeMap<String, String> = BTreeMap::new();

    if let Some(visible) = raw.get("visibleObjects").and_then(Value::as_array) {
        for obj in visible {
            let Some(handle) = obj.get("handle").and_then(Value::as_str) else {
                continue;
            };
            let zone_raw = obj.get("zone").and_then(Value::as_str).unwrap_or("");
            let zone = zone_kinds
                .get(zone_raw)
                .map(String::as_str)
                .unwrap_or(zone_raw);
            let owner = obj.get("ownerSeatId").and_then(Value::as_str).unwrap_or("");
            let ctrl = obj
                .get("controllerSeatId")
                .and_then(Value::as_str)
                .unwrap_or(owner);
            let is_mine = ctrl == viewer || (zone == "hand" && owner == viewer);

            let prefix = match (zone, is_mine) {
                ("hand", true) => "h",
                ("battlefield", true) => "b",
                ("battlefield", false) => "e",
                ("graveyard", true) => "g",
                ("graveyard", false) => "eg",
                ("stack", _) => "so",
                _ => "x",
            };
            let short = codec.alias(handle, prefix);

            let mut name = "Concealed".to_owned();
            if let Some(ident) = obj.get("identity").and_then(Value::as_object)
                && ident.get("kind").and_then(Value::as_str) == Some("known")
                && let Some(ch) = ident.get("characteristics").and_then(Value::as_object)
                && let Some(n) = ch.get("name").and_then(Value::as_str)
            {
                name = n.to_owned();
                cards.entry(name.clone()).or_insert_with(|| {
                    let mut def = Map::new();
                    if let Some(mc) = ch.get("manaCost").and_then(Value::as_str)
                        && !mc.is_empty()
                    {
                        def.insert("cost".into(), Value::String(mc.to_owned()));
                    }
                    if let Some(tl) = ch.get("typeLine").and_then(Value::as_str) {
                        def.insert("type".into(), Value::String(tl.to_owned()));
                    }
                    if let Some(rt) = ch.get("rulesText").and_then(Value::as_str)
                        && !rt.is_empty()
                    {
                        def.insert("text".into(), Value::String(rt.to_owned()));
                    }
                    if let (Some(p), Some(t)) = (
                        ch.get("printedPower").and_then(Value::as_str),
                        ch.get("printedToughness").and_then(Value::as_str),
                    ) {
                        def.insert("pt".into(), Value::String(format!("{p}/{t}")));
                    }
                    Value::Object(def)
                });
            }
            obj_names.insert(handle.to_owned(), name.clone());

            let mut entry = Map::new();
            entry.insert("id".into(), Value::String(short));
            entry.insert("card".into(), Value::String(name));
            if obj.get("tapped").and_then(Value::as_bool) == Some(true) {
                entry.insert("tapped".into(), Value::Bool(true));
            }
            if obj.get("summoningSick").and_then(Value::as_bool) == Some(true) {
                entry.insert("sick".into(), Value::Bool(true));
            }
            if let Some(dmg) = obj.get("damageMarked").and_then(Value::as_str)
                && dmg != "0"
            {
                entry.insert("dmg".into(), Value::String(dmg.to_owned()));
            }
            if let Some(counters) = obj.get("counters").and_then(Value::as_array)
                && !counters.is_empty()
            {
                entry.insert("counters".into(), Value::Array(counters.clone()));
            }
            if let Some(props) = obj.get("publicProperties").and_then(Value::as_object) {
                if let (Some(p), Some(t)) = (
                    props.get("power").and_then(Value::as_str),
                    props.get("toughness").and_then(Value::as_str),
                ) {
                    entry.insert("pt".into(), Value::String(format!("{p}/{t}")));
                }
                if let Some(kws) = props.get("keywords").and_then(Value::as_array)
                    && !kws.is_empty()
                {
                    entry.insert("kw".into(), Value::Array(kws.clone()));
                }
            }
            if is_mine
                && zone == "battlefield"
                && let Some(abilities) = obj.get("abilities").and_then(Value::as_array)
                && !abilities.is_empty()
            {
                let mut ab_list = Vec::new();
                for ab in abilities {
                    let Some(ah) = ab.get("handle").and_then(Value::as_str) else {
                        continue;
                    };
                    let ak = ab
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("activated");
                    let ab_short = codec.alias(ah, if ak == "mana" { "m" } else { "a" });
                    ab_list.push(json!({
                        "id": ab_short,
                        "kind": ak,
                        "label": ab.get("label").and_then(Value::as_str).unwrap_or(""),
                    }));
                }
                if !ab_list.is_empty() {
                    entry.insert("abilities".into(), Value::Array(ab_list));
                }
            }

            let val = Value::Object(entry);
            match (zone, is_mine) {
                ("hand", true) => hand.push(val),
                ("battlefield", true) => your_board.push(val),
                ("battlefield", false) => opp_board.push(val),
                ("graveyard", true) => your_gy.push(val),
                ("graveyard", false) => opp_gy.push(val),
                ("stack", _) => stack_objs.push(val),
                _ => {}
            }
        }
    }

    // Stack entries with enriched card name and short handles.
    let mut stack = Vec::new();
    if let Some(st_arr) = raw.get("stack").and_then(Value::as_array) {
        for item in st_arr {
            let Some(sh) = item.get("handle").and_then(Value::as_str) else {
                continue;
            };
            let short = codec.alias(sh, "st");
            let mut st_obj = Map::new();
            st_obj.insert("id".into(), Value::String(short));
            if let Some(ctrl) = item.get("controllerSeatId").and_then(Value::as_str) {
                st_obj.insert(
                    "controller".into(),
                    Value::String(codec.short_of(ctrl).unwrap_or(ctrl).to_owned()),
                );
            }
            if let Some(src) = item.get("sourceObject").and_then(Value::as_str)
                && let Some(cname) = obj_names.get(src)
            {
                st_obj.insert("card".into(), Value::String(cname.clone()));
            }
            if let Some(lbl) = item.get("label").and_then(Value::as_str) {
                st_obj.insert("label".into(), Value::String(lbl.to_owned()));
            }
            if let Some(targets) = item.get("targets").and_then(Value::as_array)
                && !targets.is_empty()
            {
                st_obj.insert(
                    "targets".into(),
                    codec.rewrite_value_to_short(&Value::Array(targets.clone())),
                );
            }
            stack.push(Value::Object(st_obj));
        }
    }

    // Required decisions (strip raw basis/deadlineAt UUID metadata).
    let mut decisions = Vec::new();
    if let Some(dec_arr) = raw.get("decisions").and_then(Value::as_array) {
        for dec in dec_arr {
            let Some(dh) = dec
                .get("decision")
                .or_else(|| dec.get("handle"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let short = codec.alias(dh, "d");
            // Pre-register option handles under `opt` prefix so they read cleanly.
            if let Some(opts) = dec
                .get("domain")
                .and_then(|d| d.get("options"))
                .and_then(Value::as_array)
            {
                for opt in opts {
                    if let Some(oh) = opt.get("handle").and_then(Value::as_str) {
                        codec.alias(oh, "opt");
                    }
                }
            }
            let mut d_obj = Map::new();
            d_obj.insert("id".into(), Value::String(short));
            if let Some(k) = dec.get("kind") {
                d_obj.insert("kind".into(), k.clone());
            }
            if let Some(lbl) = dec.get("subjectLabel") {
                d_obj.insert("subject".into(), lbl.clone());
            }
            if let Some(domain) = dec.get("domain") {
                d_obj.insert("domain".into(), codec.rewrite_value_to_short(domain));
            }
            decisions.push(Value::Object(d_obj));
        }
    }

    // Verified basic affordances (`can_play_lands`, `can_tap_for_mana`) always emitted.
    let mut can_play_lands = Vec::new();
    let mut can_tap_for_mana = Vec::new();
    for action in basic_actions {
        match action {
            PlayerAction::PlayLand { source } => {
                if let Some(s) = codec.short_of(source.as_str()) {
                    can_play_lands.push(Value::String(s.to_owned()));
                }
            }
            PlayerAction::ActivateMana { source, ability } => {
                if let (Some(s), Some(a)) = (
                    codec.short_of(source.as_str()),
                    codec.short_of(ability.as_str()),
                ) {
                    can_tap_for_mana.push(json!({"source": s, "ability": a}));
                }
            }
            _ => {}
        }
    }

    // Graveyard counts by seat derived from `zones`.
    let mut gy_counts: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(zones) = raw.get("zones").and_then(Value::as_array) {
        for z in zones {
            if z.get("zoneType").and_then(Value::as_str) == Some("graveyard")
                && let Some(owner) = z.get("ownerSeatId").and_then(Value::as_str)
            {
                let count = z
                    .get("objects")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                gy_counts.insert(owner.to_owned(), count);
            }
        }
    }

    // Players summary + own non-zero mana pool.
    let mut players = Vec::new();
    let mut mana_pool = Value::Array(Vec::new());
    if let Some(p_arr) = raw.get("players").and_then(Value::as_array) {
        for p in p_arr {
            let sid = p.get("seatId").and_then(Value::as_str).unwrap_or("");
            let short_seat = codec.short_of(sid).unwrap_or(sid).to_owned();
            if short_seat == "you"
                && let Some(mp) = p.get("manaPool").and_then(Value::as_array)
            {
                let nonzero: Vec<Value> = mp
                    .iter()
                    .filter(|entry| {
                        entry
                            .get("amount")
                            .and_then(Value::as_str)
                            .is_some_and(|amt| amt != "0")
                    })
                    .cloned()
                    .collect();
                mana_pool = Value::Array(nonzero);
            }
            players.push(json!({
                "seat": short_seat,
                "life": p.get("life"),
                "hand": p.get("handCount"),
                "lib": p.get("libraryCount"),
                "gy": gy_counts.get(sid).copied().unwrap_or(0).to_string(),
            }));
        }
    }

    // Compact recent public history (last 8 entries, no raw UUIDs).
    let mut public_history = Vec::new();
    if let Some(hist) = raw.get("publicHistory").and_then(Value::as_array) {
        let start = hist.len().saturating_sub(8);
        for item in &hist[start..] {
            if let Some(summary) = item.get("summary").and_then(Value::as_str) {
                public_history.push(Value::String(summary.to_owned()));
            }
        }
    }

    let boundary = if decisions.is_empty() {
        "priority"
    } else {
        "required_decision"
    };

    let mut state = Map::new();
    state.insert("boundary".into(), Value::String(boundary.into()));
    state.insert(
        "turn".into(),
        codec.rewrite_value_to_short(raw.get("turn").unwrap_or(&Value::Null)),
    );
    state.insert("players".into(), Value::Array(players));
    state.insert("mana_pool".into(), mana_pool);
    state.insert("decisions".into(), Value::Array(decisions));
    state.insert("can_play_lands".into(), Value::Array(can_play_lands));
    state.insert("can_tap_for_mana".into(), Value::Array(can_tap_for_mana));
    state.insert("hand".into(), Value::Array(hand));
    state.insert("your_board".into(), Value::Array(your_board));
    state.insert("opp_board".into(), Value::Array(opp_board));
    if !your_gy.is_empty() {
        state.insert("your_gy".into(), Value::Array(your_gy));
    }
    if !opp_gy.is_empty() {
        state.insert("opp_gy".into(), Value::Array(opp_gy));
    }
    state.insert("stack".into(), Value::Array(stack));
    if !stack_objs.is_empty() {
        state.insert("stack_objects".into(), Value::Array(stack_objs));
    }
    state.insert("cards".into(), Value::Object(cards.into_iter().collect()));
    if !public_history.is_empty() {
        state.insert("public_history".into(), Value::Array(public_history));
    }
    if !own_recent.is_empty() {
        state.insert(
            "own_recent".into(),
            Value::Array(own_recent.iter().cloned().map(Value::String).collect()),
        );
    }
    if let Some(fb) = feedback {
        state.insert("previous_rejection".into(), Value::String(fb.to_owned()));
    }

    let state_json =
        serde_json::to_string(&Value::Object(state)).map_err(CompactError::SerializeSnapshot)?;
    let prompt = format!("{COMPACT_ACTION_GUIDE}\n\nSTATE:\n{state_json}");
    Ok((prompt, codec))
}

/// Select the boundary-locked strict JSON schema for `snapshot`.
///
/// Uses two static schemas (`required_decision` -> `choose` only; `priority` ->
/// priority actions only) so structured decoding makes boundary mismatches
/// impossible while keeping schema prefixes static for provider prompt caching.
pub fn compact_reply_schema_for_snapshot(snapshot: &PlayerSnapshot) -> Value {
    if snapshot.decisions.is_empty() {
        compact_priority_reply_schema()
    } else {
        compact_decision_reply_schema()
    }
}

/// Strict JSON schema allowing only `choose` actions for `required_decision` boundaries.
pub fn compact_decision_reply_schema() -> Value {
    build_compact_reply_schema(SchemaMode::DecisionOnly)
}

/// Strict JSON schema allowing only priority actions (never `choose`) for `priority` boundaries.
pub fn compact_priority_reply_schema() -> Value {
    build_compact_reply_schema(SchemaMode::PriorityOnly)
}

/// Strict JSON schema allowing all compact action variants (used for offline testing).
pub fn compact_reply_json_schema() -> Value {
    build_compact_reply_schema(SchemaMode::All)
}

#[derive(Clone, Copy)]
enum SchemaMode {
    All,
    DecisionOnly,
    PriorityOnly,
}

fn build_compact_reply_schema(mode: SchemaMode) -> Value {
    fn strict_obj(props: Vec<(&str, Value)>) -> Value {
        let mut required = Vec::with_capacity(props.len());
        let mut map = Map::new();
        for (k, v) in props {
            required.push(Value::String(k.to_owned()));
            map.insert(k.to_owned(), v);
        }
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": required,
            "properties": Value::Object(map),
        })
    }

    fn variant(kind: &str, mut fields: Vec<(&str, Value)>) -> Value {
        fields.insert(0, ("kind", json!({ "type": "string", "enum": [kind] })));
        strict_obj(fields)
    }

    let s = || json!({ "type": "string" });
    let arr = |items: Value, max: u64| json!({ "type": "array", "items": items, "maxItems": max });

    let target = json!({
        "anyOf": [
            variant("seat", vec![("seatId", s())]),
            variant("object", vec![("handle", s())]),
            variant("stack", vec![("handle", s())]),
        ]
    });
    let payment = strict_obj(vec![(
        "spend",
        arr(
            strict_obj(vec![
                (
                    "color",
                    json!({
                        "type": "string",
                        "enum": ["white", "blue", "black", "red", "green", "colourless"]
                    }),
                ),
                ("amount", s()),
            ]),
            6,
        ),
    )]);
    let opt_payment = json!({ "anyOf": [payment.clone(), { "type": "null" }] });
    let choice = json!({
        "anyOf": [
            variant("option", vec![("option", s())]),
            variant("objects", vec![("objects", arr(s(), 1000))]),
            variant("ordering", vec![("ordered", arr(s(), 1000))]),
            variant("number", vec![("value", s())]),
            variant("target", vec![("target", target.clone())]),
            variant("legend", vec![("conflict", s()), ("keep", s())]),
            variant("pay", vec![("payment", payment)]),
            variant("decline", vec![]),
            variant("prevention", vec![("allocations", arr(strict_obj(vec![("attempt", s()), ("amount", s())]), 1000))]),
            variant("distribution", vec![("allocations", arr(strict_obj(vec![("target", target.clone()), ("amount", s())]), 1000))]),
            variant("attackers", vec![("assignments", arr(strict_obj(vec![("attacker", s()), ("defender", target.clone())]), 1000))]),
            variant("blockers", vec![("assignments", arr(strict_obj(vec![("blocker", s()), ("attackers", arr(s(), 1000))]), 1000))]),
            variant("combat-damage", vec![("assignments", arr(strict_obj(vec![("source", s()), ("target", target.clone()), ("amount", s())]), 1000))]),
        ]
    });
    let choose_variant = variant("choose", vec![("decision", s()), ("response", choice)]);
    let mana = variant("activate-mana", vec![("source", s()), ("ability", s())]);
    let cast_untargeted = variant(
        "cast-untargeted",
        vec![("source", s()), ("payment", opt_payment.clone())],
    );
    let cast_targeted = variant(
        "cast-targeted",
        vec![
            ("source", s()),
            ("target", target.clone()),
            ("payment", opt_payment.clone()),
        ],
    );
    let priority_variants = vec![
        variant("pass", vec![]),
        variant("play-land", vec![("source", s())]),
        mana.clone(),
        cast_untargeted.clone(),
        cast_targeted.clone(),
        variant(
            "activate-ability",
            vec![
                ("source", s()),
                ("ability", s()),
                ("target", target),
                ("payment", opt_payment.clone()),
            ],
        ),
        variant(
            "activate-untargeted-ability",
            vec![("source", s()), ("ability", s()), ("payment", opt_payment)],
        ),
        variant(
            "cast-batch",
            vec![
                ("mana", arr(mana, 8)),
                ("cast", json!({ "anyOf": [cast_untargeted, cast_targeted] })),
            ],
        ),
    ];
    let action = match mode {
        SchemaMode::DecisionOnly => choose_variant,
        SchemaMode::PriorityOnly => json!({ "anyOf": priority_variants }),
        SchemaMode::All => {
            let mut all = priority_variants;
            all.push(choose_variant);
            json!({ "anyOf": all })
        }
    };
    strict_obj(vec![
        ("action", action),
        ("delegate_pass", json!({ "type": "boolean" })),
        (
            "wake",
            arr(
                json!({
                    "type": "string",
                    "enum": [
                        "board_changed",
                        "combat",
                        "hand_changed",
                        "life_changed",
                        "opponent_action",
                        "own_main",
                        "stack_changed"
                    ]
                }),
                7,
            ),
        ),
        (
            "max_skips",
            json!({ "type": "integer", "minimum": 0, "maximum": 32 }),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_wire::NativeRevision;

    fn sample_basis() -> PlayerBasis {
        PlayerBasis {
            game_id: Uuid::from_u128(10),
            context_id: Uuid::from_u128(11),
            epoch: NativeRevision::from(0_u64),
            revision: NativeRevision::from(5_u64),
        }
    }

    #[test]
    fn handle_codec_expands_cast_batch_and_rejects_unknown_handles() {
        let mut codec = HandleCodec::default();
        let h_land = format!("n1.{}", Uuid::from_u128(100));
        let h_mana = format!("n1.{}", Uuid::from_u128(101));
        let h_spell = format!("n1.{}", Uuid::from_u128(102));
        let seat_opp = Uuid::from_u128(200).to_string();
        assert_eq!(codec.alias(&h_land, "b"), "b1");
        assert_eq!(codec.alias(&h_mana, "m"), "m1");
        assert_eq!(codec.alias(&h_spell, "h"), "h1");
        codec.bind_exact(&seat_opp, "opp");

        let reply = json!({
            "action": {
                "kind": "cast-batch",
                "mana": [{"kind": "activate-mana", "source": "b1", "ability": "m1"}],
                "cast": {
                    "kind": "cast-targeted",
                    "source": "h1",
                    "target": {"kind": "seat", "seatId": "opp"},
                    "payment": {"spend": [{"color": "red", "amount": "1"}]}
                }
            },
            "delegate_pass": false,
            "wake": ["opponent_action", "own_main"],
            "max_skips": 4
        })
        .to_string();

        let expanded = codec.expand_reply(&reply, &sample_basis()).expect("valid");
        assert!(!expanded.delegate_pass);
        assert!(expanded.wake.contains(&WakeCondition::OpponentAction));
        let AiActionPlan::CastBatch(cmds) = expanded.plan else {
            panic!("expected cast batch");
        };
        assert_eq!(cmds.len(), 2);

        let bad_reply = json!({
            "action": {"kind": "play-land", "source": "h99"},
            "delegate_pass": true,
            "wake": [],
            "max_skips": 0
        })
        .to_string();
        assert!(matches!(
            codec.expand_reply(&bad_reply, &sample_basis()),
            Err(CompactError::UnknownHandle(h)) if h == "h99"
        ));
    }
}
