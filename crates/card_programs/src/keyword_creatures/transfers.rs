//! Strict printed numeric forms of graft, modular and soulshift.
use spellwake_engine::program::TransferKeyword;
pub(super) fn parse(label: &str) -> Option<TransferKeyword> {
    let (kind, raw) = label.split_once(' ')?;
    if raw.is_empty() || !raw.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let amount = raw.parse().ok()?;
    Some(match kind {
        "modular" => TransferKeyword::Modular(amount),
        "graft" => TransferKeyword::Graft(amount),
        "soulshift" => TransferKeyword::Soulshift(amount),
        _ => return None,
    })
}
