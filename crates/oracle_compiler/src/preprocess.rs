//! Deterministic Oracle text preprocessing: reminder stripping, self-reference normalization,
//! CR 207.2c ability-word stripping, mana-sequence checking, and quote-aware splitting.

/// Unicode em-dash separator used in Scryfall Oracle text (`U+2014`).
pub const EM_DASH: char = '\u{2014}';
/// Unicode bullet used in Scryfall modal spells (`U+2022`).
pub const BULLET: char = '\u{2022}';
/// Unicode minus used in planeswalker loyalty costs (`U+2212`).
pub const MINUS: char = '\u{2212}';

/// CR 702 keywords that legitimately take an em-dash before a non-mana cost.
const DASH_COST_KEYWORDS: &[&str] = &[
    "ward",
    "equip",
    "kicker",
    "multikicker",
    "flashback",
    "cycling",
    "madness",
    "morph",
    "megamorph",
    "disguise",
    "escape",
    "evoke",
    "dash",
    "blitz",
    "unearth",
    "encore",
    "disturb",
    "embalm",
    "eternalize",
    "scavenge",
    "outlast",
    "reconfigure",
    "ninjutsu",
    "commander ninjutsu",
    "bestow",
    "emerge",
    "overload",
    "cleave",
    "buyback",
    "entwine",
    "replicate",
    "cumulative upkeep",
    "echo",
    "suspend",
    "foretell",
    "plot",
    "warp",
    "prowl",
    "spectacle",
    "surge",
    "harmonize",
    "freerunning",
    "craft with",
    "prototype",
    "squad",
    "offspring",
    "escalate",
    "more than meets the eye",
    "reinforce",
    "splice onto",
    "transmute",
    "transfigure",
    "plainscycling",
    "islandcycling",
    "swampcycling",
    "mountaincycling",
    "forestcycling",
    "basic landcycling",
    "landcycling",
    "typecycling",
    "wizardcycling",
    "slivercycling",
    "level up",
    "fortify",
    "aura swap",
    "champion",
    "offering",
    "awaken",
    "augment",
    "partner",
    "companion",
    "renew",
    "gift",
    "recover",
    "forecast",
    "station",
    "firebending",
    "earthbend",
    "airbend",
    "waterbend",
    "endure",
    "blight",
];

const SELF_NOUNS: &[&str] = &[
    "creature",
    "permanent",
    "artifact",
    "enchantment",
    "land",
    "planeswalker",
    "spell",
    "Aura",
    "Equipment",
    "Vehicle",
    "battle",
    "Siege",
    "Saga",
    "Spacecraft",
    "Room",
    "Class",
    "Case",
    "token",
    "card",
    "plane",
    "phenomenon",
    "scheme",
    "conspiracy",
    "emblem",
];

/// Strip parenthetical reminder text across multiple lines while preserving newlines outside
/// parentheses, so multi-line Saga or keyword reminder blocks do not leak orphan fragments.
#[must_use]
pub fn strip_reminder_multiline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth: u32 = 0;
    for ch in text.chars() {
        if ch == '(' {
            depth = depth.saturating_add(1);
        } else if ch == ')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            out.push(ch);
        }
    }
    out
}

/// Strip parenthetical reminder text and collapse repeated ASCII whitespace.
#[must_use]
pub fn strip_reminder(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth: u32 = 0;
    let mut prev_space = true;
    for ch in text.chars() {
        if ch == '(' {
            depth = depth.saturating_add(1);
        } else if ch == ')' && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            if ch.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                out.push(ch);
                prev_space = false;
            }
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Normalize literal card name, legendary short name, and `this <type>` self-references to `~`.
#[must_use]
pub fn normalize_self(text: &str, face_name: &str, type_line: &str) -> String {
    let mut s = if !face_name.is_empty() && text.contains(face_name) {
        let replaced = text.replace(face_name, "~");
        if matches!(
            face_name,
            "Regenerate"
                | "Exile"
                | "Destroy"
                | "Sacrifice"
                | "Counter"
                | "Discard"
                | "Return"
                | "Draw"
                | "Tap"
                | "Untap"
                | "Mill"
        ) && replaced.contains("~ target ")
        {
            replaced.replace("~ target ", &format!("{face_name} target "))
        } else if replaced.contains("Target ~ creature") {
            replaced.replace("Target ~ creature", &format!("Target {face_name} creature"))
        } else {
            replaced
        }
    } else {
        text.to_string()
    };
    // Unwrap Cleave optional brackets `[` / `]` so sentence splitting does not emit stray `]`.
    if s.contains('[') && s.contains(']') {
        s = s.replace(['[', ']'], "");
    }

    if let Some((short, _)) = face_name.split_once(',') {
        let short = short.trim();
        if short.len() >= 2 && s.contains(short) {
            s = replace_word(&s, short, "~");
        }
    } else if type_line.contains("Legendary") {
        // Pick the earliest matching title delimiter so names like "Hero-Name and the Relic"
        // extract "Hero-Name" rather than "Hero-Name and".
        let earliest_short = [" of ", " the ", " the,", " and ", " and the "]
            .into_iter()
            .filter_map(|delim| face_name.find(delim).map(|pos| (pos, &face_name[..pos])))
            .min_by_key(|(pos, _)| *pos);
        if let Some((_, short)) = earliest_short {
            let short = short.trim();
            if short.len() >= 3 && s.contains(short) {
                s = replace_word(&s, short, "~");
            }
        } else if (type_line.contains("Creature") || type_line.contains("Planeswalker"))
            && let Some((first, rest)) = face_name.split_once(' ')
            && !rest.is_empty()
            && first.len() >= 4
            && first.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && !is_common_non_proper_prefix(first)
            && s.contains(first)
        {
            s = replace_word(&s, first, "~");
        }
        // Legendary Roman-numeral suffix short name (e.g. "King Darien XLVIII" -> "King Darien")
        if let Some((prefix, last_tok)) = face_name.rsplit_once(' ')
            && !prefix.is_empty()
            && !last_tok.is_empty()
            && last_tok
                .bytes()
                .all(|b| matches!(b, b'I' | b'V' | b'X' | b'L' | b'C' | b'D' | b'M'))
            && s.contains(prefix)
        {
            s = replace_word(&s, prefix, "~");
        }
    } else if let Some((short, _)) = face_name.split_once(" and the ") {
        let short = short.trim();
        if short.len() >= 4 && s.contains(short) {
            s = replace_word(&s, short, "~");
        }
    }

    for (from_pron, to_pron) in [
        ("you cast him", "you cast it"),
        ("you cast her", "you cast it"),
        ("attached to him", "attached to it"),
        ("attached to her", "attached to it"),
        ("put him ", "put it "),
        ("put her ", "put it "),
        ("exile him", "exile it"),
        ("exile her", "exile it"),
        ("sacrifice him", "sacrifice it"),
        ("sacrifice her", "sacrifice it"),
        ("destroy him", "destroy it"),
        ("destroy her", "destroy it"),
        ("return him ", "return it "),
        ("return her ", "return it "),
        ("tap him", "tap it"),
        ("tap her", "tap it"),
        ("untap him", "untap it"),
        ("untap her", "untap it"),
        ("under her owner's control", "under its owner's control"),
        ("under his owner's control", "under its owner's control"),
        ("counter on him", "counter on it"),
        ("counter on her", "counter on it"),
        ("counters on him", "counters on it"),
        ("counters on her", "counters on it"),
        ("her loyalty abilities", "~'s loyalty abilities"),
        ("his loyalty abilities", "~'s loyalty abilities"),
        ("with him on the battlefield", "with ~ on the battlefield"),
        ("his power", "~'s power"),
        ("her controller", "its controller"),
        ("his controller", "its controller"),
        ("his text box", "~'s text box"),
        ("her text box", "~'s text box"),
        ("his name", "~'s name"),
        ("her name", "~'s name"),
    ] {
        if s.contains(from_pron) {
            s = s.replace(from_pron, to_pron);
        }
    }

    for (from_word, to_word) in [
        ("she enters", "~ enters"),
        ("he enters", "~ enters"),
        ("she has", "it has"),
        ("he has", "it has"),
        ("she was", "it was"),
        ("he was", "it was"),
        ("she's", "it's"),
        ("he's", "it's"),
        ("She's", "It's"),
        ("He's", "It's"),
        ("she dies", "it dies"),
        ("he dies", "it dies"),
        ("she attacks", "it attacks"),
        ("he attacks", "it attacks"),
        ("she deals", "it deals"),
        ("he deals", "it deals"),
    ] {
        if s.contains(from_word) {
            s = replace_word(&s, from_word, to_word);
        }
    }

    for noun in SELF_NOUNS {
        let lower_phrase = format!("this {noun}");
        let upper_phrase = format!("This {noun}");
        if s.contains(&lower_phrase) {
            s = replace_word(&s, &lower_phrase, "~");
        }
        if s.contains(&upper_phrase) {
            s = replace_word(&s, &upper_phrase, "~");
        }
    }
    s
}

/// Common English/Magic title words that must not be stripped as a legendary proper first name.
fn is_common_non_proper_prefix(word: &str) -> bool {
    matches!(
        word,
        "Ancient"
            | "Angel"
            | "Archmage"
            | "Avatar"
            | "Space"
            | "Baron"
            | "Beast"
            | "Black"
            | "Blade"
            | "Blood"
            | "Blue"
            | "Bone"
            | "Brother"
            | "Captain"
            | "Champion"
            | "Commander"
            | "Count"
            | "Crown"
            | "Dark"
            | "Dawn"
            | "Death"
            | "Demon"
            | "Doctor"
            | "Dragon"
            | "Duke"
            | "Dusk"
            | "Elder"
            | "Emperor"
            | "Empress"
            | "Eternal"
            | "Fallen"
            | "Father"
            | "Fire"
            | "First"
            | "Frost"
            | "General"
            | "Giant"
            | "Goblin"
            | "Gold"
            | "Golden"
            | "Grand"
            | "Great"
            | "Green"
            | "Guardian"
            | "Hand"
            | "Heart"
            | "High"
            | "Iron"
            | "Keeper"
            | "King"
            | "Knight"
            | "Lady"
            | "Last"
            | "Light"
            | "Living"
            | "Lord"
            | "Lost"
            | "Master"
            | "Mind"
            | "Moon"
            | "Mother"
            | "Night"
            | "Overlord"
            | "Primal"
            | "Prince"
            | "Princess"
            | "Queen"
            | "Saint"
            | "Savage"
            | "Shadow"
            | "Silver"
            | "Sister"
            | "Skull"
            | "Sliver"
            | "Soul"
            | "Sovereign"
            | "Spirit"
            | "Star"
            | "Steel"
            | "Stone"
            | "Storm"
            | "Sword"
            | "Throne"
            | "Time"
            | "Titan"
            | "Tower"
            | "True"
            | "Voice"
            | "Warden"
            | "Warlord"
            | "White"
            | "Wild"
            | "World"
    )
}

/// Replace whole-word occurrences of `needle` with `replacement`.
#[must_use]
pub fn replace_word(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || !haystack.contains(needle) {
        return haystack.to_string();
    }
    let bytes = haystack.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;
    while let Some(rel) = haystack[cursor..].find(needle) {
        let start = cursor + rel;
        let end = start + needle.len();
        let left_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let right_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok {
            out.push_str(&haystack[cursor..start]);
            out.push_str(replacement);
            cursor = end;
        } else {
            out.push_str(&haystack[cursor..end]);
            cursor = end;
        }
    }
    out.push_str(&haystack[cursor..]);
    out
}

/// Strip a CR 207.2c ability word (`<Label> U+2014 <Body>`) while preserving CR 702 em-dash keywords.
#[must_use]
pub fn strip_ability_word(norm: &str) -> (Option<String>, &str) {
    let sep = " \u{2014} ";
    let Some((head, tail)) = norm.split_once(sep) else {
        return (None, norm);
    };
    let head = head.trim();
    if head.is_empty()
        || head.len() > 64
        || head.split_whitespace().count() > 8
        || head.contains('"')
        || head.contains(';')
    {
        return (None, norm);
    }
    let first = head.chars().next().unwrap_or(' ');
    if !first.is_ascii_uppercase()
        && first != '~'
        && !first.is_ascii_digit()
        && first != '.'
        && first != '\u{221e}'
    {
        return (None, norm);
    }
    let hlow = head.to_ascii_lowercase();
    if first.is_ascii_digit()
        && (hlow.contains(" or ") || hlow.ends_with('+') || hlow.contains('-'))
    {
        return (None, norm);
    }
    if hlow.starts_with("choose ")
        || hlow.ends_with(" chooses one")
        || hlow.contains("villainous choice")
        || matches!(hlow.as_str(), "i" | "ii" | "iii" | "iv" | "v")
    {
        return (None, norm);
    }
    // If the body contains `:` (activated ability like "Renew — {4}{G}: ..." or "Exhaust — {2}: ..."),
    // always strip the em-dash label as an ability word so the cost parser receives the actual cost.
    if tail.contains(':') {
        return (Some(head.to_string()), tail.trim());
    }
    for dkw in DASH_COST_KEYWORDS {
        if hlow == *dkw || hlow.starts_with(&format!("{dkw} ")) {
            return (None, norm);
        }
    }
    (Some(head.to_string()), tail.trim())
}

/// True if `s` consists strictly of one or more `{...}` mana symbols.
#[must_use]
pub fn is_mana_sequence(s: &str) -> bool {
    if !s.starts_with('{') || !s.ends_with('}') {
        return false;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut count = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            return false;
        }
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i] != b'}' {
            let b = bytes[i];
            if !(b.is_ascii_digit()
                || matches!(
                    b,
                    b'W' | b'U'
                        | b'B'
                        | b'R'
                        | b'G'
                        | b'C'
                        | b'S'
                        | b'X'
                        | b'Y'
                        | b'Z'
                        | b'P'
                        | b'/'
                        | b'H'
                ))
            {
                return false;
            }
            i += 1;
        }
        if i >= bytes.len() || i == start {
            return false;
        }
        i += 1; // consume '}'
        count += 1;
    }
    count > 0
}

/// Split an ability or spell body into sentences on `.` outside double quotes.
#[must_use]
pub fn split_sentences(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (idx, ch) in body.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
            let ends_quoted_sentence = idx > start
                && (body.as_bytes()[idx - 1] == b'.'
                    || (idx >= start + 2
                        && body.as_bytes()[idx - 1] == b'\''
                        && body.as_bytes()[idx - 2] == b'.'));
            if !in_quotes
                && ends_quoted_sentence
                && body.get(idx + 1..).is_some_and(|rest| {
                    rest.starts_with(char::is_whitespace)
                        && rest
                            .trim_start()
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_ascii_uppercase() || c == '~')
                })
            {
                let part = body[start..=idx].trim();
                if !part.is_empty() && part != "\"" {
                    out.push(part);
                }
                start = idx + 1;
            }
        } else if ch == '.' && !in_quotes {
            let before_dot = &body[start..idx];
            let last_tok = before_dot
                .rsplit_once(char::is_whitespace)
                .map_or(before_dot, |(_, t)| t);
            if matches!(last_tok, "Dr" | "Mr" | "Mrs" | "Ms" | "St" | "Jr" | "Sr") {
                continue;
            }
            let part = before_dot.trim();
            if !part.is_empty() && part != "\"" {
                out.push(part);
            }
            start = idx + 1;
        }
    }
    let rem = body[start..].trim();
    if !rem.is_empty() && rem != "\"" {
        out.push(rem);
    }
    out
}

/// Split `text` on `sep` only outside double quotes.
#[must_use]
pub fn split_top_level<'a>(text: &'a str, sep: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    let bytes = text.as_bytes();
    let sep_bytes = sep.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
            i += 1;
        } else if !in_quotes
            && i + sep_bytes.len() <= bytes.len()
            && &bytes[i..i + sep_bytes.len()] == sep_bytes
        {
            let part = text[start..i].trim();
            if !part.is_empty() {
                out.push(part);
            }
            i += sep_bytes.len();
            start = i;
        } else {
            i += 1;
        }
    }
    let rem = text[start..].trim();
    if !rem.is_empty() {
        out.push(rem);
    }
    out
}

/// Split an activated ability `<cost>: <effect>` on the first `:` outside double quotes.
#[must_use]
pub fn split_cost_and_effect(line: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ':' && !in_quotes {
            let left = line[..idx].trim();
            let right = line[idx + 1..].trim();
            if left.is_empty() || right.is_empty() {
                return None;
            }
            let llow = left.to_ascii_lowercase();
            if llow.starts_with("choose ")
                || llow.starts_with("note")
                || llow.starts_with("flavor")
                || llow.starts_with("tier ")
                || llow.starts_with("level ")
            {
                return None;
            }
            return Some((left, right));
        }
    }
    None
}

/// Parse a Class level header (`{M}: Level N` -> `(cost_str, level)`).
#[must_use]
pub fn split_class_level_line(norm: &str) -> Option<(&str, u8)> {
    let (left, right) = norm.split_once(':')?;
    let cost = left.trim();
    let lvl_str = right.trim().strip_prefix("Level ")?;
    let level = lvl_str.trim().parse::<u8>().ok()?;
    if cost.is_empty() {
        return None;
    }
    Some((cost, level))
}

/// Parse a Leveler band header (`LEVEL N-M` or `LEVEL N+`).
#[must_use]
pub fn parse_leveler_band(norm: &str) -> Option<(u8, Option<u8>)> {
    let rest = norm.strip_prefix("LEVEL ")?.trim();
    if let Some(min_str) = rest.strip_suffix('+') {
        let min = min_str.trim().parse::<u8>().ok()?;
        return Some((min, None));
    }
    if let Some((a, b)) = rest.split_once('-') {
        let min = a.trim().parse::<u8>().ok()?;
        let max = b.trim().parse::<u8>().ok()?;
        return Some((min, Some(max)));
    }
    None
}

/// Parse an Unfinity Sticker option line (`{TK}{TK} U+2014 1/5` or `{TK}{TK} U+2014 Flying`).
#[must_use]
pub fn parse_sticker_option(norm: &str) -> Option<(u8, String)> {
    let (left, right) = norm.split_once(EM_DASH)?;
    let l = left.trim();
    if !l.starts_with("{TK}") {
        return None;
    }
    let rem = l.replace("{TK}", "");
    if !rem.trim().is_empty() {
        return None;
    }
    let count = l.matches("{TK}").count();
    let payload = right.trim();
    if payload.is_empty() {
        return None;
    }
    Some((u8::try_from(count).unwrap_or(u8::MAX), payload.to_string()))
}

/// Count occurrences of a repeated single symbol token like `{E}{E}{E}` or `{TK}{TK}`.
#[must_use]
pub fn count_symbol_sequence(s: &str, sym: &str) -> Option<u16> {
    let t = s.trim();
    if t.is_empty() || !t.starts_with(sym) {
        return None;
    }
    if !t.replace(sym, "").is_empty() {
        return None;
    }
    let n = t.matches(sym).count();
    u16::try_from(n).ok()
}

/// Parse a `LEVEL N-M`, `LEVEL N+`, `N/N`, `{M} - N/N`, or `{M}: Level N` exotic layout header.
#[must_use]
pub fn is_exotic_layout_header(norm: &str) -> bool {
    if norm.starts_with("Level ") && norm[6..].chars().all(|c| c.is_ascii_digit()) && norm.len() > 6
    {
        return true;
    }
    if let Some((left, right)) = norm.split_once(EM_DASH)
        && is_mana_sequence(left.trim())
        && is_bare_pt(right.trim())
    {
        return true;
    }
    false
}

/// True if `s` is a plain `N/N` stat line.
#[must_use]
pub fn is_bare_pt(s: &str) -> bool {
    let Some((p, t)) = s.split_once('/') else {
        return false;
    };
    !p.is_empty()
        && !t.is_empty()
        && p.bytes().all(|b| b.is_ascii_digit())
        && t.bytes().all(|b| b.is_ascii_digit())
}

/// Parse a planeswalker loyalty line `[+N / -N / 0 / -X]: <effect>`.
#[must_use]
pub fn split_loyalty_line(norm: &str) -> Option<(String, &str)> {
    let (left, right) = norm.split_once(':')?;
    let l = left.trim();
    if l.is_empty() || right.trim().is_empty() {
        return None;
    }
    let normalized = l.replace(MINUS, "-");
    let rest = normalized
        .strip_prefix('+')
        .or_else(|| normalized.strip_prefix('-'))
        .unwrap_or(&normalized);
    if rest == "X" || (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())) {
        return Some((normalized, right.trim()));
    }
    None
}

/// Parse a Saga chapter line `I, II <U+2014> <effect>`.
#[must_use]
pub fn split_saga_line(norm: &str) -> Option<(Vec<String>, &str)> {
    let (left, right) = norm.split_once(EM_DASH)?;
    let chapters: Vec<String> = left
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if chapters.is_empty()
        || !chapters
            .iter()
            .all(|c| c.bytes().all(|b| matches!(b, b'I' | b'V' | b'X')))
    {
        return None;
    }
    Some((chapters, right.trim()))
}

/// Parse a `+/-X/+/-Y` power/toughness modifier token like `+2/+2`, `-1/-1`, `+X/+0`.
#[must_use]
pub fn parse_pt_mod(tok: &str) -> Option<(String, String)> {
    let (dx, dy) = tok.split_once('/')?;
    let valid_part = |p: &str| {
        let Some(rest) = p.strip_prefix('+').or_else(|| p.strip_prefix('-')) else {
            return false;
        };
        rest == "X"
            || rest == "x"
            || rest == "Y"
            || rest == "y"
            || (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    };
    if valid_part(dx) && valid_part(dy) {
        Some((dx.to_ascii_uppercase(), dy.to_ascii_uppercase()))
    } else {
        None
    }
}

/// Parse a `P/T` base stat token like `3/3`, `X/X`, `*/*`.
#[must_use]
pub fn parse_base_pt(tok: &str) -> Option<(String, String)> {
    let (p, t) = tok.split_once('/')?;
    let valid = |s: &str| {
        s == "X" || s == "x" || s == "*" || (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    };
    if valid(p) && valid(t) {
        Some((p.to_ascii_uppercase(), t.to_ascii_uppercase()))
    } else {
        None
    }
}

/// True if `text` contains a physical, voice, attire, dexterity, or bystander prompt
/// suited for Spellwake's opt-in Tabletop Honor-System primitives (only enabled for Un-cards).
#[must_use]
pub fn is_physical_tabletop_clause(lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "gotcha",
        "audibly",
        "say \"",
        "says \"",
        "sing a ",
        "singing",
        "clap your",
        "high-five",
        "high five",
        "on your head",
        "from a height of",
        "feet away",
        "wearing ",
        "denim",
        "a shoe",
        "your shoe",
        "wearing a hat",
        "wearing glasses",
        "person outside",
        "bystander",
        "tear ~",
        "rip ~",
        "laughing",
        "smiles",
        "blinks",
        "stand up",
        "ten seconds",
        "real time",
        "rhyme",
        "accent",
        "whisper",
        "arm wrestle",
        "rock-paper-scissors",
        "staring contest",
        "balance ~",
        "jumping jack",
    ];
    // Guard physical hat attire/art prompts so eternal-legal Unfinity sticker clauses
    // (e.g. "... on that sticker") never trip the silver-border honor-system guard.
    let mentions_physical_hat = !lower.contains("sticker")
        && (lower == "a hat"
            || lower.starts_with("a hat ")
            || lower.starts_with("a hat.")
            || lower.starts_with("a hat,")
            || lower.ends_with(" a hat")
            || lower.contains(" a hat ")
            || lower.contains(" a hat.")
            || lower.contains(" a hat,"));
    mentions_physical_hat || MARKERS.iter().any(|m| lower.contains(m))
}
