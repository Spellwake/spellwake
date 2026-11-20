//! Test-build access to actual damage allocators on an explicitly cloned source.
use super::DamageState;

impl DamageState {
    pub(crate) fn qualification_set_next_shield(&mut self, next: u64) {
        self.next_shield = next;
    }

    pub(crate) fn qualification_set_next_multiplier(&mut self, next: u64) {
        self.next_multiplier = next;
    }

    pub(crate) fn qualification_set_next_batch(&mut self, next: u64) {
        self.next_batch = next;
    }

    pub(crate) fn qualification_set_next_attempt(&mut self, next: u64) {
        self.next_attempt = next;
    }
}
