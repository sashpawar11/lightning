//! Power shortcuts (shut down, restart, sleep...): a static table matched by
//! keyword prefix. Nothing is allocated or scored unless a keyword matches.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Shutdown,
    Restart,
    Sleep,
    SignOut,
    Lock,
    Hibernate,
}

pub struct PowerAction {
    pub action: Action,
    /// Stable id used in `Hit::target`.
    pub id: &'static str,
    pub label: &'static str,
    /// Lowercase phrases; the query must be a prefix of one.
    pub keywords: &'static [&'static str],
    /// Closes other apps or ends the session, so it needs a second activation.
    pub confirm: bool,
}

const fn a(
    action: Action,
    id: &'static str,
    label: &'static str,
    keywords: &'static [&'static str],
    confirm: bool,
) -> PowerAction {
    PowerAction { action, id, label, keywords, confirm }
}

/// Table order is the display order. Every action also answers to "power".
pub static ACTIONS: &[PowerAction] = &[
    a(Action::Shutdown, "shutdown", "Shut down", &["power", "shutdown", "shut down", "power off", "poweroff", "turn off"], true),
    a(Action::Restart, "restart", "Restart", &["power", "restart", "reboot"], true),
    a(Action::Sleep, "sleep", "Sleep", &["power", "sleep", "suspend", "standby"], false),
    a(Action::SignOut, "signout", "Sign out", &["power", "sign out", "signout", "log off", "logoff", "log out", "logout"], true),
    a(Action::Lock, "lock", "Lock", &["power", "lock"], false),
    a(Action::Hibernate, "hibernate", "Hibernate", &["power", "hibernate"], false),
];

/// Shorter queries are too ambiguous to show buttons for.
const MIN_QUERY: usize = 3;

impl Action {
    pub fn info(self) -> &'static PowerAction {
        ACTIONS.iter().find(|p| p.action == self).expect("every action is in the table")
    }

    /// The `Hit::target` for this action.
    pub fn target(self) -> String {
        format!("power:{}", self.info().id)
    }

    pub fn from_target(t: &str) -> Option<Self> {
        let key = t.strip_prefix("power:")?;
        ACTIONS.iter().find(|p| p.id == key).map(|p| p.action)
    }
}

/// Actions whose keywords the query starts, best match first. Allocates only
/// when something matches; rejection is a few byte compares.
pub fn matches(query: &str) -> Vec<&'static PowerAction> {
    let q = query.trim().as_bytes();
    if q.len() < MIN_QUERY {
        return Vec::new();
    }
    // 2 = whole keyword typed, 1 = prefix of one.
    let rank = |p: &PowerAction| {
        p.keywords
            .iter()
            .filter(|k| k.len() >= q.len() && k.as_bytes()[..q.len()].eq_ignore_ascii_case(q))
            .map(|k| if k.len() == q.len() { 2 } else { 1 })
            .max()
    };
    let mut found: Vec<(u8, &PowerAction)> = ACTIONS.iter().filter_map(|p| Some((rank(p)?, p))).collect();
    // Stable, so table order breaks ties.
    found.sort_by_key(|(r, _)| std::cmp::Reverse(*r));
    found.into_iter().map(|(_, p)| p).collect()
}

/// Whether `query` is a whole keyword, as opposed to the start of one. Only
/// then is the intent clear enough for the buttons to lead and be pre-selected.
pub fn is_exact(query: &str) -> bool {
    let q = query.trim();
    ACTIONS.iter().any(|p| p.keywords.iter().any(|k| k.eq_ignore_ascii_case(q)))
}

/// A press on `action`'s button. Returns whether to run it now: actions
/// needing confirmation arm on the first press and run on the second.
/// Anything else (another key, Esc, a click elsewhere) should clear `armed`.
pub fn activate(armed: &mut Option<Action>, action: Action) -> bool {
    if !action.info().confirm || *armed == Some(action) {
        *armed = None;
        return true;
    }
    *armed = Some(action);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(q: &str) -> Option<Action> {
        matches(q).first().map(|p| p.action)
    }

    #[test]
    fn keywords_find_actions() {
        assert_eq!(first("shut"), Some(Action::Shutdown));
        assert_eq!(first("Shutdown"), Some(Action::Shutdown));
        assert_eq!(first("power off"), Some(Action::Shutdown));
        assert_eq!(first("sign"), Some(Action::SignOut));
        assert_eq!(first("log off"), Some(Action::SignOut));
        assert_eq!(first("reboot"), Some(Action::Restart));
        assert_eq!(first("rest"), Some(Action::Restart));
        assert_eq!(first(" sleep "), Some(Action::Sleep));
        assert_eq!(first("lock"), Some(Action::Lock));
        assert_eq!(first("hiber"), Some(Action::Hibernate));
    }

    #[test]
    fn power_lists_everything() {
        assert_eq!(matches("power").len(), ACTIONS.len());
        assert_eq!(first("power"), Some(Action::Shutdown));
    }

    #[test]
    fn ordinary_queries_match_nothing() {
        for q in ["", "s", "sh", "re", "chrome", "calc", "sound", "lo", "locker", "power x", "shutdownn"] {
            assert!(matches(q).is_empty(), "{q}");
        }
    }

    #[test]
    fn only_whole_keywords_are_exact() {
        for q in ["power", "Lock", " sleep ", "shut down", "reboot"] {
            assert!(is_exact(q), "{q}");
        }
        for q in ["pow", "sta", "shut", "loc", "sign", "chrome", ""] {
            assert!(!is_exact(q), "{q}");
        }
    }

    #[test]
    fn exact_keyword_beats_prefix() {
        // "lock" is whole for Lock; "logoff"-style prefixes never outrank it.
        assert_eq!(first("lock"), Some(Action::Lock));
        assert_eq!(first("sleep"), Some(Action::Sleep));
    }

    #[test]
    fn targets_round_trip() {
        for p in ACTIONS {
            assert_eq!(Action::from_target(&p.action.target()), Some(p.action));
        }
        assert_eq!(Action::from_target("ms-settings:display"), None);
    }

    #[test]
    fn destructive_actions_need_two_presses() {
        let mut armed = None;
        assert!(!activate(&mut armed, Action::Shutdown));
        assert_eq!(armed, Some(Action::Shutdown));
        assert!(activate(&mut armed, Action::Shutdown));
        assert_eq!(armed, None);
        // Arming one button doesn't confirm another.
        assert!(!activate(&mut armed, Action::Restart));
        assert!(!activate(&mut armed, Action::SignOut));
        assert_eq!(armed, Some(Action::SignOut));
        // Disarmed (any other key) starts over.
        armed = None;
        assert!(!activate(&mut armed, Action::SignOut));
    }

    #[test]
    fn lock_and_sleep_run_at_once() {
        let mut armed = None;
        for a in [Action::Lock, Action::Sleep, Action::Hibernate] {
            assert!(activate(&mut armed, a));
            assert_eq!(armed, None);
        }
    }
}
