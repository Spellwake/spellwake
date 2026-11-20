//! Shared contact and account-maturity rules; callers supply trusted committed evidence.

const DAY_MILLISECONDS: u64 = 86_400_000;
pub const RECENT_GAME_MILLISECONDS: u64 = 14 * DAY_MILLISECONDS;
pub const MATURE_AGE_MILLISECONDS: u64 = 30 * DAY_MILLISECONDS;

/// Only the participant's own committed turns establish qualification, never table turns.
pub fn is_real_game(own_turns: u64) -> bool {
    own_turns >= 3
}

/// Both elapsed age and lifetime real-game count are necessary, irrespective of play mode.
pub fn is_restricted(created_at_ms: u64, now_ms: u64, lifetime_real_games: u64) -> bool {
    let age_qualifies = now_ms
        .checked_sub(created_at_ms)
        .is_some_and(|elapsed| elapsed >= MATURE_AGE_MILLISECONDS);
    !(age_qualifies && lifetime_real_games >= 20)
}

/// Reused by DM sends and new direct offers; an existing offer has its separate acceptance rule.
pub fn can_contact(
    blocked: bool,
    current_friend: bool,
    shared_real_game_at_ms: Option<u64>,
    now_ms: u64,
) -> bool {
    let recent = shared_real_game_at_ms
        .and_then(|shared| now_ms.checked_sub(shared))
        .is_some_and(|elapsed| elapsed <= RECENT_GAME_MILLISECONDS);
    !blocked && (current_friend || recent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_uses_elapsed_time_and_both_maturity_conditions() {
        let created = 1_000_000;
        assert!(is_restricted(
            created,
            created + MATURE_AGE_MILLISECONDS - 1,
            20
        ));
        assert!(is_restricted(
            created,
            created + MATURE_AGE_MILLISECONDS,
            19
        ));
        assert!(!is_restricted(
            created,
            created + MATURE_AGE_MILLISECONDS,
            20
        ));
        assert!(is_restricted(created, created - 1, 100));
        assert!(!is_real_game(2));
        assert!(is_real_game(3));
    }

    #[test]
    fn block_overrides_friendship_and_recent_game_with_exact_cutoff() {
        let now = RECENT_GAME_MILLISECONDS + 1000;
        assert!(can_contact(false, true, None, now));
        assert!(!can_contact(true, true, Some(now), now));
        assert!(can_contact(false, false, Some(1000), now));
        assert!(!can_contact(false, false, Some(999), now));
        assert!(!can_contact(false, false, Some(now + 1), now));
        assert!(!can_contact(false, false, None, now));
    }
}
