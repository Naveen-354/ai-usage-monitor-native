//! Pure state machine for two-step confirmation of destructive actions.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfirmState {
    #[default]
    Idle,
    Armed,
}

impl ConfirmState {
    /// Arms the confirmation dialog/button.
    pub fn arm(&mut self) {
        *self = ConfirmState::Armed;
    }

    /// Cancels confirmation and returns to idle state.
    pub fn cancel(&mut self) {
        *self = ConfirmState::Idle;
    }

    /// Whether the action is currently waiting for confirmation.
    pub fn is_armed(&self) -> bool {
        matches!(self, ConfirmState::Armed)
    }

    /// Executes confirmation: if armed, transitions back to idle and returns true.
    /// Otherwise returns false.
    pub fn confirm(&mut self) -> bool {
        if *self == ConfirmState::Armed {
            *self = ConfirmState::Idle;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_idle() {
        let state = ConfirmState::default();
        assert_eq!(state, ConfirmState::Idle);
        assert!(!state.is_armed());
    }

    #[test]
    fn arm_and_cancel_transitions() {
        let mut state = ConfirmState::default();
        state.arm();
        assert!(state.is_armed());
        state.cancel();
        assert!(!state.is_armed());
        assert_eq!(state, ConfirmState::Idle);
    }

    #[test]
    fn confirm_when_armed_succeeds_and_resets() {
        let mut state = ConfirmState::default();
        state.arm();
        assert!(state.confirm());
        assert_eq!(state, ConfirmState::Idle);
        assert!(!state.is_armed());
    }

    #[test]
    fn confirm_when_idle_fails() {
        let mut state = ConfirmState::default();
        assert!(!state.confirm());
        assert_eq!(state, ConfirmState::Idle);
    }
}
