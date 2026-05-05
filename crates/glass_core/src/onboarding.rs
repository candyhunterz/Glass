//! Onboarding coordinator: pure decision logic for welcome overlay and contextual hints.
//!
//! Receives trigger events from main.rs, returns display actions.
//! No rendering or I/O — fully testable.

use std::collections::HashSet;

use crate::state::GlassState;

/// Events the coordinator receives from main.rs.
#[derive(Debug, Clone)]
pub enum OnboardingEvent {
    SessionStart,
    CommandModifiedFiles,
    PipeDetected { stages: usize },
    SoiParsed,
    ProposalReady,
    CommandCount(u32),
    CodexNotLoggedIn,
}

/// Actions the coordinator returns to main.rs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnboardingAction {
    ShowWelcome,
    ShowToast(HintId),
}

/// Identifiers for each contextual hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HintId {
    Undo,
    PipeViz,
    HistorySearch,
    Soi,
    AgentProposals,
    CodexLogin,
}

impl HintId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::PipeViz => "pipe_viz",
            Self::HistorySearch => "history_search",
            Self::Soi => "soi",
            Self::AgentProposals => "agent_proposals",
            Self::CodexLogin => "codex_login",
        }
    }

    pub fn parse_id(s: &str) -> Option<Self> {
        match s {
            "undo" => Some(Self::Undo),
            "pipe_viz" => Some(Self::PipeViz),
            "history_search" => Some(Self::HistorySearch),
            "soi" => Some(Self::Soi),
            "agent_proposals" => Some(Self::AgentProposals),
            "codex_login" => Some(Self::CodexLogin),
            _ => None,
        }
    }
}

/// Result of LLM provider detection.
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    pub name: &'static str,
    pub available: bool,
    pub detail: String,
}

pub struct OnboardingCoordinator {
    welcome_completed: bool,
    hints_shown: HashSet<HintId>,
    pending_hints: Vec<HintId>,
    providers: Vec<ProviderStatus>,
    toast_active: bool,
    welcome_active: bool,
}

impl OnboardingCoordinator {
    /// Create coordinator from persisted state.
    pub fn from_state(state: &GlassState) -> Self {
        let hints_shown: HashSet<HintId> = state
            .hints_shown
            .iter()
            .filter_map(|s| HintId::parse_id(s))
            .collect();

        Self {
            welcome_completed: state.welcome_completed,
            hints_shown,
            pending_hints: Vec::new(),
            providers: Vec::new(),
            toast_active: false,
            welcome_active: false,
        }
    }

    /// Persist current state back to GlassState.
    pub fn save_to_state(&self, state: &mut GlassState) {
        state.welcome_completed = self.welcome_completed;
        state.hints_shown = self
            .hints_shown
            .iter()
            .map(|h| h.as_str().to_string())
            .collect();
    }

    /// Set detected provider results (called from main.rs after async detection).
    pub fn set_providers(&mut self, providers: Vec<ProviderStatus>) {
        self.providers = providers;
    }

    /// Get provider statuses for rendering.
    pub fn providers(&self) -> &[ProviderStatus] {
        &self.providers
    }

    /// Whether the welcome overlay has been completed.
    pub fn welcome_completed(&self) -> bool {
        self.welcome_completed
    }

    /// Mark the welcome overlay as completed.
    pub fn complete_welcome(&mut self) {
        self.welcome_completed = true;
        self.welcome_active = false;
    }

    /// Notify that the currently displayed toast has been dismissed.
    pub fn toast_dismissed(&mut self) {
        self.toast_active = false;
    }

    /// Process a trigger event, return zero or more display actions.
    pub fn process(
        &mut self,
        event: OnboardingEvent,
        proposal_toast_active: bool,
    ) -> Vec<OnboardingAction> {
        let mut actions = Vec::new();

        // Show welcome on first session start
        if let OnboardingEvent::SessionStart = &event {
            if !self.welcome_completed {
                self.welcome_active = true;
                actions.push(OnboardingAction::ShowWelcome);
                return actions;
            }
        }

        // Don't process hints while welcome is showing
        if self.welcome_active {
            return actions;
        }

        // Don't show new toasts while one is active or proposal toast is showing
        if self.toast_active || proposal_toast_active {
            // Still evaluate triggers to queue them
            self.evaluate_hint(&event);
            return actions;
        }

        // Check if this event triggers a new hint
        if let Some(hint) = self.evaluate_hint(&event) {
            self.hints_shown.insert(hint);
            self.toast_active = true;
            actions.push(OnboardingAction::ShowToast(hint));
            return actions;
        }

        // Check pending queue
        if let Some(hint) = self.pending_hints.pop() {
            if !self.hints_shown.contains(&hint) {
                self.hints_shown.insert(hint);
                self.toast_active = true;
                actions.push(OnboardingAction::ShowToast(hint));
            }
        }

        actions
    }

    /// Evaluate whether an event triggers a hint. Returns the hint if triggered.
    /// If the hint can't be shown now (toast active), queues it as pending.
    fn evaluate_hint(&mut self, event: &OnboardingEvent) -> Option<HintId> {
        let candidate = match event {
            OnboardingEvent::CommandModifiedFiles => Some(HintId::Undo),
            OnboardingEvent::PipeDetected { .. } => Some(HintId::PipeViz),
            OnboardingEvent::SoiParsed => Some(HintId::Soi),
            OnboardingEvent::ProposalReady => Some(HintId::AgentProposals),
            OnboardingEvent::CommandCount(n) if *n >= 10 => Some(HintId::HistorySearch),
            OnboardingEvent::CodexNotLoggedIn => Some(HintId::CodexLogin),
            _ => None,
        };

        if let Some(hint) = candidate {
            if !self.hints_shown.contains(&hint) {
                if self.toast_active {
                    // Queue for later
                    if !self.pending_hints.contains(&hint) {
                        self.pending_hints.push(hint);
                    }
                    return None;
                }
                return Some(hint);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_coordinator() -> OnboardingCoordinator {
        OnboardingCoordinator::from_state(&GlassState::default())
    }

    #[test]
    fn first_session_shows_welcome() {
        let mut coord = fresh_coordinator();
        let actions = coord.process(OnboardingEvent::SessionStart, false);
        assert_eq!(actions, vec![OnboardingAction::ShowWelcome]);
    }

    #[test]
    fn returning_user_no_welcome() {
        let state = GlassState {
            session_count: 5,
            welcome_completed: true,
            hints_shown: Vec::new(),
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        let actions = coord.process(OnboardingEvent::SessionStart, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn hints_suppressed_during_welcome() {
        let mut coord = fresh_coordinator();
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CommandModifiedFiles, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn undo_hint_fires_on_file_modify() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CommandModifiedFiles, false);
        assert_eq!(actions, vec![OnboardingAction::ShowToast(HintId::Undo)]);
    }

    #[test]
    fn hint_fires_only_once() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        coord.process(OnboardingEvent::CommandModifiedFiles, false);
        coord.toast_dismissed();
        let actions = coord.process(OnboardingEvent::CommandModifiedFiles, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn pipe_hint_includes_stage_count() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::PipeDetected { stages: 3 }, false);
        assert_eq!(actions, vec![OnboardingAction::ShowToast(HintId::PipeViz)]);
    }

    #[test]
    fn history_hint_at_10_commands() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CommandCount(9), false);
        assert!(actions.is_empty());
        let actions = coord.process(OnboardingEvent::CommandCount(10), false);
        assert_eq!(
            actions,
            vec![OnboardingAction::ShowToast(HintId::HistorySearch)]
        );
    }

    #[test]
    fn hint_suppressed_during_proposal_toast() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CommandModifiedFiles, true);
        assert!(actions.is_empty());
    }

    #[test]
    fn deferred_hint_fires_on_next_event() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        coord.process(OnboardingEvent::CommandModifiedFiles, false);
        coord.process(OnboardingEvent::PipeDetected { stages: 2 }, false);
        coord.toast_dismissed();
        let actions = coord.process(OnboardingEvent::CommandCount(1), false);
        assert_eq!(actions, vec![OnboardingAction::ShowToast(HintId::PipeViz)]);
    }

    #[test]
    fn state_roundtrip() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        coord.process(OnboardingEvent::CommandModifiedFiles, false);

        let mut new_state = GlassState::default();
        coord.save_to_state(&mut new_state);
        assert!(new_state.welcome_completed);
        assert!(new_state.hints_shown.contains(&"undo".to_string()));
    }

    #[test]
    fn persisted_hints_not_reshown() {
        let state = GlassState {
            welcome_completed: true,
            hints_shown: vec!["undo".to_string()],
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CommandModifiedFiles, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn codex_login_hint_id_roundtrips() {
        assert_eq!(HintId::CodexLogin.as_str(), "codex_login");
        assert_eq!(HintId::parse_id("codex_login"), Some(HintId::CodexLogin));
    }

    #[test]
    fn codex_not_logged_in_event_triggers_codex_login_hint() {
        let state = GlassState {
            welcome_completed: true,
            ..Default::default()
        };
        let mut coord = OnboardingCoordinator::from_state(&state);
        coord.process(OnboardingEvent::SessionStart, false);
        let actions = coord.process(OnboardingEvent::CodexNotLoggedIn, false);
        assert_eq!(
            actions,
            vec![OnboardingAction::ShowToast(HintId::CodexLogin)]
        );
    }
}
