//! Exact printed hand-to-exile actions and complete spell bodies for the current profile.
use super::*;
use spellwake_engine::program::ExileAction;

pub(super) fn parse(label: &str) -> Option<ExileAction> {
    if let Some(price) = label.strip_prefix("foretell ") {
        return Some(ExileAction::Foretell(fixed_cost(
            &price.to_ascii_uppercase(),
        )?));
    }
    let (n, price) = label.strip_prefix("suspend ")?.split_once('—')?;
    let counters = n.parse::<u16>().ok().filter(|&n| n > 0)?;
    Some(ExileAction::Suspend {
        counters,
        price: fixed_cost(&price.to_ascii_uppercase())?,
    })
}
