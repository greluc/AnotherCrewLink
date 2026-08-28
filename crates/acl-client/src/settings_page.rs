//! The settings screen, as the client holds it.
//!
//! [`acl_ui::settings_screen`] says what is on the screen and
//! [`acl_ui::views::settings`] draws it. Neither of them writes anything: a control that
//! is used comes back as a [`Change`], and this is what a change happens to.
//!
//! Three things live here because none of them is a drawing decision.
//!
//! **The confirmation.** A warning is a dialog, and a dialog outlives the frame that
//! raised it. The change is held until it is answered, and it is applied on yes and
//! dropped on no — never applied and then undone, which would have written it to disk in
//! between.
//!
//! **The capture.** A shortcut is read with `GetAsyncKeyState`, the same call that later
//! polls it, rather than from a keyboard event: egui reports modifiers as flags rather
//! than keys, so a window event cannot tell the right control key from the left, and the
//! shipped defaults are sided (`RControl`, `RAlt`). Scanning the key state can.
//!
//! **The file.** `config.json` is written after every applied change rather than on the
//! way out. A client that loses a preference because it crashed an hour later is a client
//! nobody trusts with preferences.

use acl_ui::config::Config;
use acl_ui::settings_screen::Scope;
use acl_ui::views::settings::{Change, Context, Entry, Values, as_json, draw, path};

/// What the settings page is currently doing.
pub(crate) struct Page {
    /// The file, as it was last read or written.
    config: Config,
    /// Where it goes.
    file: std::path::PathBuf,
    /// A change waiting on its warning being answered.
    pending: Option<Change>,
    /// The shortcut being captured, and what was already down when the capture began.
    capture: Option<Capture>,
}

/// A capture in progress.
struct Capture {
    /// Which setting it will be written to.
    key: &'static str,
    /// The keys that were already down when it started.
    ///
    /// Without this the mouse button that clicked the field, or the modifier still held
    /// from the shortcut that opened the settings, is captured immediately. A key has to
    /// go up and come down again to count.
    ignored: Vec<u16>,
}

/// What the caller has to act on, because the page cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Every setting goes back to its default.
    RestoreDefaults,
    /// The stored memory offsets are thrown away and fetched again.
    ResetOffsets,
    /// The language changed, so the catalogue has to be reloaded.
    LanguageChanged,
    /// Play a sound through the speaker that is selected.
    ///
    /// A `Probe` rather than an `Action`: it changes nothing, so it carries no warning and
    /// needs no confirmation. See `Kind::Probe`.
    TestSpeaker,
}

impl Page {
    /// Reads the settings file, or starts from an empty one.
    ///
    /// A file that will not parse reads as empty rather than as a refusal to start —
    /// [`Config::read`] says why.
    #[must_use]
    pub(crate) fn open(file: std::path::PathBuf) -> Self {
        let config = std::fs::read_to_string(&file)
            .map_or_else(|_| Config::new(), |text| Config::read(&text));
        Self {
            config,
            file,
            pending: None,
            capture: None,
        }
    }

    /// The settings, for anything else that needs to read one.
    #[must_use]
    pub(crate) const fn config(&self) -> &Config {
        &self.config
    }

    /// Which shortcut is being captured, if any.
    #[must_use]
    pub(crate) fn capturing(&self) -> Option<&'static str> {
        self.capture.as_ref().map(|capture| capture.key)
    }

    /// Draws the page and acts on what came back.
    ///
    /// Returns whatever the caller has to do that this cannot.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, context: &Context<'_>) -> Vec<Effect> {
        let mut effects = Vec::new();
        for change in draw(ui, &self.config, context) {
            self.act(change, &mut effects);
        }
        self.confirmation(ui, context, &mut effects);
        effects
    }

    /// Takes one change, either applying it or holding it for its warning.
    fn act(&mut self, change: Change, effects: &mut Vec<Effect>) {
        match change {
            Change::Capture(key) => self.begin_capture(key),
            Change::Run { key, warning } => {
                if warning.is_some() {
                    self.pending = Some(Change::Run { key, warning });
                } else {
                    Self::run(key, effects);
                }
            }
            set @ Change::Set { warning, .. } => {
                if warning.is_some() {
                    self.pending = Some(set);
                } else {
                    self.apply(&set, effects);
                }
            }
            // Applied as an ordinary `Set` of the schema's own value, so everything that
            // follows a change -- writing the file, and the effects a particular key has --
            // happens exactly as it would if somebody had typed the default in.
            Change::Reset { key, scope } => {
                let Some(default) = acl_ui::settings::default_for(key).map(as_json) else {
                    // A reset naming a setting the schema does not have. Refused rather
                    // than written as an empty value, and `a_reset_names_a_setting_that_
                    // exists` is what makes this unreachable.
                    return;
                };
                self.apply(
                    &Change::Set {
                        key,
                        scope,
                        value: default,
                        warning: None,
                    },
                    effects,
                );
            }
        }
    }

    /// Writes a change through to the file.
    fn apply(&mut self, change: &Change, effects: &mut Vec<Effect>) {
        let Change::Set {
            key, scope, value, ..
        } = change
        else {
            return;
        };
        self.config.set(&path(*scope, key), value.clone());
        self.save();
        if *key == "language" {
            effects.push(Effect::LanguageChanged);
        }
    }

    /// What an action button means to the caller.
    fn run(key: &str, effects: &mut Vec<Effect>) {
        match key {
            "restoreDefaults" => effects.push(Effect::RestoreDefaults),
            "resetOffsets" => effects.push(Effect::ResetOffsets),
            "testSpeaker" => effects.push(Effect::TestSpeaker),
            // Unreachable while `settings_screen` is the only source of these, and not a
            // panic: an action this build does not know is a screen that has moved ahead
            // of its handler, which is a missing feature rather than a broken client.
            _ => {}
        }
    }

    /// The dialog a warned change waits in.
    fn confirmation(
        &mut self,
        ui: &mut egui::Ui,
        context: &Context<'_>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let warning = match &pending {
            Change::Set { warning, .. } | Change::Run { warning, .. } => *warning,
            // Neither can be held: a capture is answered by a key press, and a reset puts
            // back a value the schema gives and so has nothing to confirm.
            Change::Capture(_) | Change::Reset { .. } => None,
        };
        let Some(warning) = warning else {
            self.pending = None;
            return;
        };

        let mut answered = None;
        egui::Window::new("")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label((context.t)("settings.warning"));
                ui.label((context.t)(warning));
                ui.horizontal(|ui| {
                    if ui.button((context.t)("buttons.confirm")).clicked() {
                        answered = Some(true);
                    }
                    if ui.button((context.t)("buttons.cancel")).clicked() {
                        answered = Some(false);
                    }
                });
            });

        match answered {
            None => {}
            Some(false) => self.pending = None,
            Some(true) => {
                self.pending = None;
                match pending {
                    Change::Run { key, .. } => Self::run(key, effects),
                    set @ Change::Set { .. } => self.apply(&set, effects),
                    // Unreachable: neither is ever held, for the reason above.
                    Change::Capture(_) | Change::Reset { .. } => {}
                }
            }
        }
    }

    /// Starts capturing a shortcut.
    fn begin_capture(&mut self, key: &'static str) {
        self.capture = Some(Capture {
            key,
            ignored: down_now(),
        });
    }

    /// Advances a capture, if one is running.
    ///
    /// Called every frame rather than from an event: see the module documentation for why
    /// the key state and not the window's events.
    pub(crate) fn poll_capture(&mut self) {
        let Some(capture) = self.capture.as_mut() else {
            return;
        };
        let down = down_now();
        // Anything held when the capture began stops being ignored once it is released,
        // so the click that started it does not end it.
        capture.ignored.retain(|key| down.contains(key));
        let Some(pressed) = down
            .iter()
            .find(|key| !capture.ignored.contains(key))
            .copied()
        else {
            return;
        };
        let Some(name) = acl_core::shortcuts::name_for(pressed) else {
            return;
        };
        let key = capture.key;
        self.capture = None;
        self.config
            .set(&path(Scope::Client, key), serde_json::json!(name));
        self.save();
    }

    /// Abandons a capture without changing anything.
    pub(crate) fn cancel_capture(&mut self) {
        self.capture = None;
    }

    /// Writes the file.
    ///
    /// After every applied change rather than on the way out: a client that loses a
    /// preference because it crashed an hour later is a client nobody trusts with
    /// preferences. A failed write is not reported here — there is nowhere to report it
    /// from a paint function, and the setting is still in force for this session.
    fn save(&self) {
        let _ = std::fs::write(&self.file, self.config.write());
    }

    /// Puts every setting back to its default.
    ///
    /// By emptying the document rather than by writing the defaults into it: that is what
    /// a fresh installation is, and what `electron-store` does — it writes only what
    /// differs from the default.
    pub(crate) fn restore_defaults(&mut self) {
        self.config = Config::new();
        self.save();
    }
}

impl Values for Page {
    fn bool_at(&self, scope: Scope, key: &str) -> bool {
        Values::bool_at(&self.config, scope, key)
    }

    fn number_at(&self, scope: Scope, key: &str) -> f64 {
        Values::number_at(&self.config, scope, key)
    }

    fn text_at(&self, scope: Scope, key: &str) -> String {
        Values::text_at(&self.config, scope, key)
    }
}

/// Every capturable key that is down right now.
#[cfg(windows)]
fn down_now() -> Vec<u16> {
    use acl_core::keys::KeyState as _;

    let state = acl_core::keys::AsyncKeyState;
    acl_core::shortcuts::CAPTURABLE
        .iter()
        .copied()
        .filter(|key| state.is_down(*key))
        .collect()
}

/// Nothing is ever down off Windows, because there is no key state to ask.
#[cfg(not(windows))]
fn down_now() -> Vec<u16> {
    Vec::new()
}

/// The locales this build ships, as the language picker wants them.
#[must_use]
pub(crate) fn locales() -> Vec<Entry<'static>> {
    acl_i18n::NAMES
        .iter()
        .map(|(tag, name)| Entry {
            id: tag,
            label: name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Effect, Page};
    use acl_ui::settings_screen::Scope;
    use acl_ui::views::settings::{Change, Values};
    use serde_json::json;

    fn page() -> Page {
        Page::open(std::env::temp_dir().join("acl-settings-page-test.json"))
    }

    fn set(key: &'static str, warning: Option<&'static str>) -> Change {
        Change::Set {
            key,
            scope: Scope::Client,
            value: json!(true),
            warning,
        }
    }

    /// A change with no warning takes effect at once.
    #[test]
    fn an_unwarned_change_applies_immediately() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(set("alwaysOnTop", None), &mut effects);
        assert!(Values::bool_at(&page, Scope::Client, "alwaysOnTop"));
        assert!(effects.is_empty());
    }

    /// A warned one does not, and this is the half that matters: applying it and undoing
    /// it on "no" would have written it to the file in between.
    #[test]
    fn a_warned_change_waits_and_changes_nothing_yet() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(
            set("natFix", Some("settings.advanced.nat_fix_warning")),
            &mut effects,
        );
        assert!(
            !Values::bool_at(&page, Scope::Client, "natFix"),
            "the change was applied before it was confirmed"
        );
        assert!(page.pending.is_some());
    }

    /// An action with a warning waits too, rather than running and being regretted.
    #[test]
    fn a_warned_action_waits() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(
            Change::Run {
                key: "resetOffsets",
                warning: Some("settings.troubleshooting.reset_offsets_warning"),
            },
            &mut effects,
        );
        assert!(effects.is_empty(), "the action ran before it was confirmed");
        assert!(page.pending.is_some());
    }

    /// Both actions reach the caller, because neither is something a paint function can do.
    #[test]
    fn the_two_actions_are_handed_back() {
        let mut effects = Vec::new();
        Page::run("restoreDefaults", &mut effects);
        Page::run("resetOffsets", &mut effects);
        assert_eq!(effects, [Effect::RestoreDefaults, Effect::ResetOffsets]);
    }

    /// An action this build does not know is ignored rather than fatal: a screen that has
    /// moved ahead of its handler is a missing feature, not a broken client.
    #[test]
    fn an_unknown_action_does_nothing() {
        let mut effects = Vec::new();
        Page::run("somethingNewer", &mut effects);
        assert!(effects.is_empty());
    }

    /// Changing the language tells the caller, because the catalogue is loaded once and
    /// would otherwise go on answering in the old one.
    #[test]
    fn changing_the_language_asks_for_a_reload() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(
            Change::Set {
                key: "language",
                scope: Scope::Client,
                value: json!("de"),
                warning: None,
            },
            &mut effects,
        );
        assert_eq!(effects, [Effect::LanguageChanged]);
    }

    /// A lobby rule is written under `localLobbySettings`, where 1.x looks for it.
    #[test]
    fn a_lobby_rule_lands_in_the_other_clients_place() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(
            Change::Set {
                key: "haunting",
                scope: Scope::Lobby,
                value: json!(true),
                warning: None,
            },
            &mut effects,
        );
        assert_eq!(
            page.config().get("localLobbySettings.haunting"),
            Some(&json!(true))
        );
    }

    /// Restoring empties the document rather than writing the defaults into it. That is
    /// what a fresh installation is, and it is what `electron-store` does — it stores only
    /// what differs from the default.
    #[test]
    fn restoring_empties_rather_than_filling() {
        let mut page = page();
        let mut effects = Vec::new();
        page.act(set("alwaysOnTop", None), &mut effects);
        page.restore_defaults();
        assert!(page.config().get("alwaysOnTop").is_none());
        assert!(!Values::bool_at(&page, Scope::Client, "alwaysOnTop"));
    }

    /// Every locale the picker offers has a catalogue behind it, and there are as many
    /// entries as there are translations.
    #[test]
    fn the_picker_offers_every_shipped_locale() {
        let offered = super::locales();
        assert_eq!(offered.len(), acl_i18n::NAMES.len());
        assert!(offered.iter().any(|entry| entry.id == "en"));
        assert!(
            offered.iter().all(|entry| !entry.label.is_empty()),
            "a locale would show as a blank row"
        );
    }
}
