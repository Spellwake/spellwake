//! One physical split card with two independently compiled spell bodies.
use super::*;
use spellwake_content::CatalogFace;
use spellwake_engine::program::SplitCard;

pub(super) fn compile(
    card: &InventoryCard,
    vocabulary: &CreatureTypeVocabulary,
    profile: Profile,
) -> Result<Program, &'static str> {
    let [left, right] = card.faces.as_slice() else {
        return Err("outside-split-face-count");
    };
    let compile_half = |face: &CatalogFace, unsupported: &'static str| -> Result<_, &'static str> {
        if !matches!(face.type_line.as_deref(), Some("Instant" | "Sorcery")) {
            return Err("outside-split-spell-type");
        }
        let mut standalone = card.clone();
        standalone.layout = "normal".to_owned();
        standalone.name = face.name.clone();
        standalone.faces = vec![face.clone()];
        let rules = without_reminder(face.oracle_text.as_deref().ok_or("missing-split-text")?)
            .ok_or("outside-split-text")?;
        let mut lines: Vec<&str> = rules.trim().lines().map(str::trim).collect();
        let aftermath = lines.first().copied() == Some("Aftermath");
        if aftermath {
            lines.remove(0);
        }
        let before_fuse_len = lines.len();
        lines.retain(|line| *line != "Fuse");
        let fuse = lines.len() != before_fuse_len;
        if aftermath || fuse {
            standalone.faces[0].oracle_text = Some(lines.join("\n"));
        }
        let hybrid = face.mana_cost.as_deref().and_then(split_hybrid_cost);
        if let Some((cost, _)) = &hybrid {
            standalone.faces[0].mana_cost = Some(cost.clone());
        }
        let mut program =
            super::compile(&standalone, vocabulary, profile).map_err(|_| unsupported)?;
        if let Some((_, symbols)) = hybrid {
            program.hybrid_symbols = symbols;
        }
        program.names = ObjectNames::single(
            EnglishCardName::new(face.name.clone()).map_err(|_| "outside-split-name")?,
        );
        Ok((program, aftermath, fuse))
    };
    let (left, left_aftermath, left_fuse) = compile_half(left, "outside-split-left-body")?;
    if left_aftermath {
        return Err("outside-split-aftermath-position");
    }
    let (right, aftermath, right_fuse) = compile_half(right, "outside-split-right-body")?;
    let fuse = left_fuse || right_fuse;
    if aftermath && fuse {
        return Err("outside-split-aftermath-fuse");
    }
    SplitCard {
        left,
        right,
        aftermath,
        fuse,
    }
    .into_program()
    .ok_or("outside-split-characteristics")
}
