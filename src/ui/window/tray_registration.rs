//! Retry only failed Shell registration; successful/idle trays have no timer.
#[derive(Default)]
pub(super) struct TrayRegistration {
    failures: u8,
    pub registered: bool,
}
impl TrayRegistration {
    pub fn can_retry(&self) -> bool {
        !self.registered && self.failures < 30
    }

    pub fn complete(&mut self, success: bool) -> bool {
        self.registered = success;
        if !success {
            self.failures = self.failures.saturating_add(1);
        }
        self.can_retry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_recovery_stops_retries_and_persistent_failure_is_bounded() {
        let mut state = TrayRegistration::default();
        assert!(state.complete(false));
        assert!(state.complete(false));
        assert!(!state.complete(true));
        assert!(state.registered);
        assert!(!state.can_retry());
        // Explorer restart begins a fresh registration cycle.
        state = TrayRegistration::default();
        for _ in 0..100 {
            if state.can_retry() {
                state.complete(false);
            }
        }
        assert!(!state.can_retry());
        assert!(!state.registered);
    }
}
