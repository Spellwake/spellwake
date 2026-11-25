//! Deterministic MTG Oracle phrasing standardizer for Spellwake Custom Card Studio.
//!
//! Normalizes common legacy or informal card wording into modern Foundations/M25+ Oracle
//! syntax accepted by `spellwake-oracle-compiler` and `spellwake-card-programs` while
//! recording every rewrite so the author can review or undo changes in one click.

use serde::{Deserialize, Serialize};

use crate::preprocess::{BULLET, EM_DASH, MINUS};

/// Minimal face fields participating in phrasing standardization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardizeFaceInput {
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: String,
    pub oracle_text: String,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_indicator: Option<Vec<String>>,
}

/// One deterministic rewrite recorded during standardization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhrasingChange {
    pub face_index: usize,
    pub before: String,
    pub after: String,
    pub reason: String,
}

/// Result returned by [`standardize_faces`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardizeOutput {
    pub faces: Vec<StandardizeFaceInput>,
    pub changes: Vec<PhrasingChange>,
    pub changed: bool,
}

/// Deterministically normalize all faces of a custom card into modern MTG Oracle wording.
#[must_use]
pub fn standardize_faces(faces: &[StandardizeFaceInput]) -> StandardizeOutput {
    let mut out_faces = Vec::with_capacity(faces.len());
    let mut changes = Vec::new();

    for (face_index, face) in faces.iter().enumerate() {
        let mut next = face.clone();
        let trimmed_name = face.name.trim().to_owned();
        if trimmed_name != face.name {
            changes.push(PhrasingChange {
                face_index,
                before: face.name.clone(),
                after: trimmed_name.clone(),
                reason: "Trim leading/trailing spaces in card name".to_owned(),
            });
            next.name = trimmed_name;
        }

        // Normalize mana cost casing and braces (e.g. "{2}{u}{r}" -> "{2}{U}{R}").
        if let Some(ref raw_cost) = face.mana_cost {
            let norm_cost = normalize_mana_symbols(raw_cost.trim());
            let final_cost = if norm_cost.is_empty() {
                None
            } else {
                Some(norm_cost)
            };
            if final_cost != face.mana_cost {
                changes.push(PhrasingChange {
                    face_index,
                    before: raw_cost.clone(),
                    after: final_cost.clone().unwrap_or_default(),
                    reason: "Normalize mana cost symbol casing".to_owned(),
                });
                next.mana_cost = final_cost;
            }
        }

        // Normalize type line separator (" - " / " -- " / " – " -> " — ").
        let norm_type = normalize_type_line(&face.type_line);
        if norm_type != face.type_line {
            changes.push(PhrasingChange {
                face_index,
                before: face.type_line.clone(),
                after: norm_type.clone(),
                reason: "Use standard Magic type-line separator".to_owned(),
            });
            next.type_line = norm_type;
        }

        // Normalize oracle text line by line.
        let (norm_oracle, oracle_changes) =
            standardize_oracle_text(&face.oracle_text, &next.name, &next.type_line, face_index);
        if norm_oracle != face.oracle_text {
            next.oracle_text = norm_oracle;
            changes.extend(oracle_changes);
        }

        out_faces.push(next);
    }

    let changed = !changes.is_empty();
    StandardizeOutput {
        faces: out_faces,
        changes,
        changed,
    }
}

fn normalize_type_line(type_line: &str) -> String {
    let trimmed = type_line.trim();
    let dash_sep = format!(" {EM_DASH} ");
    let replaced = trimmed
        .replace(" -- ", &dash_sep)
        .replace(" - ", &dash_sep)
        .replace(" \u{2013} ", &dash_sep);
    collapse_inline_spaces(&replaced)
}

fn normalize_mana_symbols(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut token = String::new();
            let mut closed = false;
            for inner in chars.by_ref() {
                if inner == '}' {
                    closed = true;
                    break;
                }
                token.push(inner);
            }
            if closed {
                let upper = token.trim().to_ascii_uppercase();
                let canonical = match upper.as_str() {
                    "TAP" => "T".to_owned(),
                    "UNTAP" => "Q".to_owned(),
                    other => other.to_owned(),
                };
                out.push('{');
                out.push_str(&canonical);
                out.push('}');
            } else {
                out.push('{');
                out.push_str(&token);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn collapse_inline_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut prev_space = false;
    for ch in line.trim().chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

fn standardize_oracle_text(
    text: &str,
    face_name: &str,
    type_line: &str,
    face_index: usize,
) -> (String, Vec<PhrasingChange>) {
    let is_planeswalker = type_line.contains("Planeswalker");
    let normalized_newlines = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out_lines = Vec::new();
    let mut changes = Vec::new();
    let mut in_modal_bullets = false;

    for raw_line in normalized_newlines.split('\n') {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut line = normalize_mana_symbols(trimmed);

        // Replace placeholder card-name macros with exact face name.
        if !face_name.is_empty() {
            let before = line.clone();
            line = line
                .replace("[CARDNAME]", face_name)
                .replace("CARDNAME", face_name)
                .replace('~', face_name);
            if line != before {
                changes.push(PhrasingChange {
                    face_index,
                    before,
                    after: line.clone(),
                    reason: "Expand card-name placeholder to printed name".to_owned(),
                });
            }
        }

        // Normalize modal bullet prefix (* or - after a Choose ... line).
        if in_modal_bullets
            && let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* "))
        {
            let before = line.clone();
            line = format!("{BULLET} {}", rest.trim());
            changes.push(PhrasingChange {
                face_index,
                before,
                after: line.clone(),
                reason: "Use standard modal bullet symbol".to_owned(),
            });
        } else if !line.starts_with(BULLET) {
            in_modal_bullets = false;
        }

        // Normalize Planeswalker loyalty prefix ("-2:" -> "−2:", "+1 :" -> "+1:").
        if is_planeswalker
            && let Some(rewritten) = normalize_loyalty_prefix(&line)
            && rewritten != line
        {
            changes.push(PhrasingChange {
                face_index,
                before: line.clone(),
                after: rewritten.clone(),
                reason: "Use standard planeswalker loyalty cost format".to_owned(),
            });
            line = rewritten;
        }

        // Normalize "Choose one - " / "−2: Choose one - " -> em-dash modal header.
        if let Some(rewritten) = normalize_modal_header(&line) {
            if rewritten != line {
                changes.push(PhrasingChange {
                    face_index,
                    before: line.clone(),
                    after: rewritten.clone(),
                    reason: "Use standard modal header punctuation".to_owned(),
                });
                line = rewritten;
            }
            in_modal_bullets = true;
        } else if line.ends_with(EM_DASH) && contains_choose_clause(&line) {
            in_modal_bullets = true;
        }

        // Deterministic Foundations / M25+ phrasing rewrites.
        let rules: &[(&str, &str, &str)] = &[
            (
                "enters the battlefield",
                "enters",
                "Use Foundations 'enters' phrasing",
            ),
            (
                "Enters the battlefield",
                "Enters",
                "Use Foundations 'Enters' phrasing",
            ),
            (
                "enter the battlefield",
                "enter",
                "Use Foundations 'enter' phrasing",
            ),
            (
                "converted mana cost",
                "mana value",
                "Use modern 'mana value' terminology",
            ),
            (
                "Converted mana cost",
                "Mana value",
                "Use modern 'Mana value' terminology",
            ),
            (
                " to your mana pool.",
                ".",
                "Remove obsolete 'to your mana pool' wording",
            ),
            (
                " to your mana pool,",
                ",",
                "Remove obsolete 'to your mana pool' wording",
            ),
            (
                " to your mana pool",
                "",
                "Remove obsolete 'to your mana pool' wording",
            ),
            (
                "shuffle your library",
                "shuffle",
                "Use modern 'shuffle' wording",
            ),
            (
                "Shuffle your library",
                "Shuffle",
                "Use modern 'Shuffle' wording",
            ),
            (
                "shuffles their library",
                "shuffles",
                "Use modern 'shuffles' wording",
            ),
            ("his or her", "their", "Use modern singular 'their' pronoun"),
            ("he or she", "they", "Use modern singular 'they' pronoun"),
            ("him or her", "them", "Use modern singular 'them' pronoun"),
            ("cannot ", "can't ", "Use Oracle contraction 'can't'"),
            ("can not ", "can't ", "Use Oracle contraction 'can't'"),
            (
                "is put into a graveyard from the battlefield",
                "dies",
                "Use modern 'dies' wording",
            ),
        ];

        for &(needle, replacement, reason) in rules {
            if line.contains(needle) {
                let before = line.clone();
                line = line.replace(needle, replacement);
                changes.push(PhrasingChange {
                    face_index,
                    before,
                    after: line.clone(),
                    reason: reason.to_owned(),
                });
            }
        }

        // Normalize "deals X damage to target creature or player" -> "any target".
        if line.contains("to target creature or player") {
            let before = line.clone();
            line = line.replace("to target creature or player", "to any target");
            changes.push(PhrasingChange {
                face_index,
                before,
                after: line.clone(),
                reason: "Use modern 'any target' damage wording".to_owned(),
            });
        }

        // Normalize "put a ... token onto the battlefield" -> "create a ... token".
        if let Some(rewritten) = normalize_put_token(&line) {
            changes.push(PhrasingChange {
                face_index,
                before: line.clone(),
                after: rewritten.clone(),
                reason: "Use modern 'create' token phrasing".to_owned(),
            });
            line = rewritten;
        }

        let collapsed = collapse_inline_spaces(&line);
        out_lines.push(collapsed);
    }

    (out_lines.join("\n"), changes)
}

fn normalize_loyalty_prefix(line: &str) -> Option<String> {
    let (prefix, rest) = line.split_once(':')?;
    let p = prefix.trim();
    if let Some(num) = p.strip_prefix('-')
        && !num.is_empty()
        && num
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'X' || c == 'x')
    {
        return Some(format!(
            "{MINUS}{}: {}",
            num.to_ascii_uppercase(),
            rest.trim()
        ));
    }
    if let Some(num) = p.strip_prefix('+')
        && !num.is_empty()
        && num
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'X' || c == 'x')
    {
        return Some(format!("+{}: {}", num.to_ascii_uppercase(), rest.trim()));
    }
    if p == "0" {
        return Some(format!("0: {}", rest.trim()));
    }
    None
}

fn contains_choose_clause(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    low.starts_with("choose ") || low.contains(": choose ") || low.contains(", choose ")
}

fn normalize_modal_header(line: &str) -> Option<String> {
    if !contains_choose_clause(line) {
        return None;
    }
    for suffix in [" --", " -", ":"] {
        if let Some(stem) = line.strip_suffix(suffix) {
            return Some(format!("{} {EM_DASH}", stem.trim_end()));
        }
    }
    None
}

fn normalize_put_token(line: &str) -> Option<String> {
    let low = line.to_ascii_lowercase();
    let onto_idx = low.find(" onto the battlefield")?;
    for (prefix, repl) in [("Put ", "Create "), ("put ", "create ")] {
        if let Some(start_idx) = line.find(prefix)
            && start_idx < onto_idx
            && low[start_idx..onto_idx].contains(" token")
        {
            let before = &line[..start_idx];
            let middle = &line[start_idx + prefix.len()..onto_idx];
            let after = &line[onto_idx + " onto the battlefield".len()..];
            return Some(format!("{before}{repl}{middle}{after}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_legacy_oracle_phrases_deterministically() {
        let input = vec![StandardizeFaceInput {
            name: "Aetherblade Vanguard".to_owned(),
            mana_cost: Some("{2}{u}{r}".to_owned()),
            type_line: "Creature - Human Artificer".to_owned(),
            oracle_text: "When ~ enters the battlefield, add {r} to your mana pool.\n{t}: Put a 1/1 red Goblin creature token onto the battlefield, then shuffle your library.".to_owned(),
            power: Some("3".to_owned()),
            toughness: Some("2".to_owned()),
            loyalty: None,
            defense: None,
            color_indicator: None,
        }];
        let out = standardize_faces(&input);
        assert!(out.changed);
        assert_eq!(out.faces[0].mana_cost.as_deref(), Some("{2}{U}{R}"));
        assert_eq!(
            out.faces[0].type_line,
            format!("Creature {EM_DASH} Human Artificer")
        );
        assert_eq!(
            out.faces[0].oracle_text,
            "When Aetherblade Vanguard enters, add {R}.\n{T}: Create a 1/1 red Goblin creature token, then shuffle."
        );
        assert!(!out.changes.is_empty());
    }

    #[test]
    fn normalizes_planeswalker_minus_and_modal_bullets() {
        let input = vec![StandardizeFaceInput {
            name: "Kael, Rift Weaver".to_owned(),
            mana_cost: Some("{3}{U}".to_owned()),
            type_line: "Legendary Planeswalker - Kael".to_owned(),
            oracle_text: "+1: Draw a card.\n-2: Choose one -\n- Return target creature to its owner's hand.\n- Counter target spell.".to_owned(),
            power: None,
            toughness: None,
            loyalty: Some("4".to_owned()),
            defense: None,
            color_indicator: None,
        }];
        let out = standardize_faces(&input);
        assert!(out.changed);
        assert!(out.faces[0].oracle_text.contains(&format!("{MINUS}2:")));
        assert!(
            out.faces[0]
                .oracle_text
                .contains(&format!("{BULLET} Return"))
        );
    }
}
