//! Drawing the settings screen.
//!
//! [`crate::settings_screen`] says what is on it; this puts the widgets up. The split is
//! the one [`crate::views`] asks for, and this screen is the reason it is worth having:
//! forty controls, several gated, several behind a confirmation, two that are not
//! settings. Written as one paint function that is forty branches nobody can reach.
//!
//! **Nothing here writes a setting.** Every control that is used returns a [`Change`], and
//! the caller applies it. Two reasons, and the second is the load-bearing one. A warning
//! is a dialog, and a dialog outlives the frame that raised it — a paint function cannot
//! wait for an answer. And the lobby rules are not always this client's to write: when
//! somebody else is host they are read from what the host sent and are not writable at
//! all, which is a decision about who is in the game rather than about drawing.

use egui::{ComboBox, Slider, Ui};
use serde_json::{Value, json};

use crate::settings_screen::{
    Control, Kind, SECTIONS, Scope, availability, gate_is_its_own_control, shown, stored,
};

/// What the player did.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A setting was moved. Not yet stored.
    Set {
        /// Which setting.
        key: &'static str,
        /// Whose it is, which is what says where it is written.
        scope: Scope,
        /// The new value, in the shape `config.json` holds it.
        value: Value,
        /// A warning to confirm before applying it, if the control has one.
        warning: Option<&'static str>,
    },
    /// A button was pressed. Always carries its warning; see
    /// `an_action_is_not_a_setting`.
    Run {
        /// Which action.
        key: &'static str,
        /// What to ask first.
        warning: Option<&'static str>,
    },
    /// A shortcut field was focused, and the next key or button pressed is the shortcut.
    Capture(&'static str),
}

/// One entry of a list the player chooses from.
#[derive(Clone, Copy, Debug)]
pub struct Entry<'a> {
    /// What is stored when it is picked.
    pub id: &'a str,
    /// What is shown.
    pub label: &'a str,
}

/// Where the values come from, and how they are named.
///
/// A trait rather than a `&Config` because [`Scope::Lobby`] values are not always this
/// client's: when another player is host they come from what the host sent. Which one is
/// in force is a question about the game, so it is answered by the caller.
pub trait Values {
    /// A boolean setting.
    fn bool_at(&self, scope: Scope, key: &str) -> bool;
    /// A numeric setting.
    fn number_at(&self, scope: Scope, key: &str) -> f64;
    /// A text setting.
    fn text_at(&self, scope: Scope, key: &str) -> String;
}

/// Reads this client's own file for both scopes.
///
/// What a host sees, and what anybody sees before joining a lobby.
impl Values for crate::config::Config {
    fn bool_at(&self, scope: Scope, key: &str) -> bool {
        Self::bool_at(self, &path(scope, key))
    }

    fn number_at(&self, scope: Scope, key: &str) -> f64 {
        Self::number_at(self, &path(scope, key))
    }

    fn text_at(&self, scope: Scope, key: &str) -> String {
        Self::text_at(self, &path(scope, key))
    }
}

/// Where a setting lives in `config.json`.
///
/// The lobby rules are one object under `localLobbySettings`, which is how
/// `SettingsStore.tsx` writes them. Flattening them would put `haunting` beside
/// `masterVolume` in a file 1.x also reads, and 1.x would not find either of them.
#[must_use]
pub fn path(scope: Scope, key: &str) -> String {
    match scope {
        Scope::Client => key.to_owned(),
        Scope::Lobby => format!("localLobbySettings.{key}"),
    }
}

/// Everything the screen needs that is not a setting.
pub struct Context<'a> {
    /// Looks up an i18n key.
    pub t: &'a dyn Fn(&str) -> String,
    /// The microphones this machine currently has.
    pub microphones: &'a [Entry<'a>],
    /// The speakers.
    pub speakers: &'a [Entry<'a>],
    /// The locales under `static/locales`, with their names.
    pub locales: &'a [Entry<'a>],
    /// What the microphone is hearing, from nought to one, for [`Kind::Meter`].
    ///
    /// `None` when there is no microphone open, which draws an empty bar rather than a
    /// full one: a meter that reads maximum when nothing is listening is worse than one
    /// that reads nothing.
    pub input_level: Option<f32>,
    /// Whether this player may change the lobby rules: host, and in a lobby.
    pub host_may_change: bool,
    /// Whether the player is somewhere a lobby exists, which picks which of the two
    /// explanations a disabled lobby rule gives.
    pub in_menu_or_lobby: bool,
    /// The shortcut currently being captured, if any, so its field can say so.
    pub capturing: Option<&'a str>,
}

/// Draws the whole screen.
pub fn draw(ui: &mut Ui, values: &dyn Values, context: &Context<'_>) -> Vec<Change> {
    let mut changes = Vec::new();
    for section in SECTIONS {
        if let Some(title) = section.title {
            ui.add_space(8.0);
            ui.heading((context.t)(title));
        }
        for control in section.controls {
            let gate_is_on = control
                .gate
                .is_some_and(|gate| values.bool_at(section.scope, gate));
            let state = availability(
                control,
                section.scope,
                gate_is_on,
                context.host_may_change,
                context.in_menu_or_lobby,
            );
            // The gating checkbox, *before* the control and carrying its label -- so it
            // reads "[x] Microphone volume" over the slider it enables.
            //
            // Two things about where it is drawn. It is outside the `add_enabled_ui` below,
            // because a gate inside the thing it gates is one you can switch off and never
            // switch back on. And it is skipped when the gate is a control in its own
            // right: `enableOverlay` has its own labelled row, and drawing it again for
            // each of the three controls it gates is what put three unlabelled checkboxes
            // in the overlay section.
            let own_gate = control
                .gate
                .filter(|gate| !gate_is_its_own_control(gate))
                .map(|gate| {
                    gate_checkbox(
                        ui,
                        gate,
                        section.scope,
                        gate_is_on,
                        &control.label.map(context.t).unwrap_or_default(),
                        &mut changes,
                    );
                });
            let response = ui
                .scope(|ui| {
                    ui.add_enabled_ui(state.enabled, |ui| {
                        one(
                            ui,
                            control,
                            section.scope,
                            values,
                            context,
                            &mut changes,
                            own_gate.is_some(),
                        );
                    });
                })
                .response;
            if let Some(reason) = state.reason {
                response.on_hover_text((context.t)(reason));
            }
        }
        ui.separator();
    }
    changes
}

/// The checkbox that enables a gated control, labelled with what it enables.
fn gate_checkbox(
    ui: &mut Ui,
    gate: &'static str,
    scope: Scope,
    is_on: bool,
    label: &str,
    changes: &mut Vec<Change>,
) {
    let mut on = is_on;
    if ui.checkbox(&mut on, label).changed() {
        changes.push(Change::Set {
            key: gate,
            scope,
            value: json!(on),
            warning: None,
        });
    }
}

/// One control.
fn one(
    ui: &mut Ui,
    control: &'static Control,
    scope: Scope,
    values: &dyn Values,
    context: &Context<'_>,
    changes: &mut Vec<Change>,
    labelled_by_its_gate: bool,
) {
    // Empty when the gating checkbox above already carries it, which is the whole reason
    // that checkbox is worth drawing: the label says what the switch is for.
    let label = if labelled_by_its_gate {
        String::new()
    } else {
        control.label.map(context.t).unwrap_or_default()
    };
    let set = |value: Value| Change::Set {
        key: control.key,
        scope,
        value,
        warning: control.warning,
    };
    match control.kind {
        Kind::Toggle => {
            let mut on = values.bool_at(scope, control.key);
            if ui.checkbox(&mut on, label).changed() {
                changes.push(set(json!(on)));
            }
        }
        Kind::Slider { min, max, step, .. } => {
            let mut value = shown(control.kind, values.number_at(scope, control.key));
            ui.label(label);
            if ui
                .add(Slider::new(&mut value, min..=max).step_by(step))
                .changed()
            {
                changes.push(set(json!(stored(control.kind, value))));
            }
        }
        Kind::Choice(options) => {
            let current = options
                .iter()
                .find(|choice| matches(choice.value, values, scope, control.key));
            let shown_label = current.map_or_else(String::new, |choice| (context.t)(choice.label));
            ComboBox::from_id_salt(control.key)
                .selected_text(shown_label)
                .show_ui(ui, |ui| {
                    for choice in options {
                        if ui
                            .selectable_label(
                                current.is_some_and(|now| now.value == choice.value),
                                (context.t)(choice.label),
                            )
                            .clicked()
                        {
                            changes.push(set(as_json(choice.value)));
                        }
                    }
                });
        }
        Kind::Device { capture } => {
            let devices = if capture {
                context.microphones
            } else {
                context.speakers
            };
            list(ui, control, scope, &label, devices, values, changes);
        }
        Kind::Language => list(ui, control, scope, &label, context.locales, values, changes),
        Kind::Shortcut => {
            let held = values.text_at(scope, control.key);
            let capturing = context.capturing == Some(control.key);
            // Ellipsis rather than an empty field while capturing: a field that has gone
            // blank looks like the shortcut was cleared.
            let text = if capturing { "…" } else { held.as_str() };
            if ui.button(format!("{label}: {text}")).clicked() {
                changes.push(Change::Capture(control.key));
            }
        }
        Kind::Meter => {
            let level = context.input_level.unwrap_or(0.0).clamp(0.0, 1.0);
            // A bar rather than a number. The question it answers is "is it hearing me",
            // and the answer is whether the thing moves when you speak.
            ui.add(egui::ProgressBar::new(level).desired_width(200.0).fill(
                if context.input_level.is_some() {
                    egui::Color32::from_rgb(0x2e, 0xcc, 0x71)
                } else {
                    ui.visuals().weak_text_color()
                },
            ));
        }
        Kind::Text => {
            let mut text = values.text_at(scope, control.key);
            ui.label(label);
            // On losing focus rather than on every keystroke. `serverURL` is validated and
            // reconnected to when it changes, and half a URL is a server that does not
            // exist.
            if ui.text_edit_singleline(&mut text).lost_focus() {
                changes.push(set(json!(text)));
            }
        }
        Kind::Action => {
            if ui.button(label).clicked() {
                changes.push(Change::Run {
                    key: control.key,
                    warning: control.warning,
                });
            }
        }
        Kind::Probe => {
            // The same `Run`, and never a warning: a `Probe` has none by construction, and
            // `nothing_that_writes_nothing_shadows_a_setting` is what keeps it that way.
            if ui.button(label).clicked() {
                changes.push(Change::Run {
                    key: control.key,
                    warning: None,
                });
            }
        }
    }
}

/// A combo box over a list that is only known at run time.
fn list(
    ui: &mut Ui,
    control: &'static Control,
    scope: Scope,
    label: &str,
    entries: &[Entry<'_>],
    values: &dyn Values,
    changes: &mut Vec<Change>,
) {
    let current = values.text_at(scope, control.key);
    let shown_label = entries
        .iter()
        .find(|entry| entry.id == current)
        .map_or(current.as_str(), |entry| entry.label);
    ui.label(label);
    ComboBox::from_id_salt(control.key)
        .selected_text(shown_label)
        .show_ui(ui, |ui| {
            for entry in entries {
                if ui
                    .selectable_label(entry.id == current, entry.label)
                    .clicked()
                {
                    changes.push(Change::Set {
                        key: control.key,
                        scope,
                        value: json!(entry.id),
                        warning: control.warning,
                    });
                }
            }
        });
}

/// Whether a choice is the one currently stored.
fn matches(
    choice: crate::settings::Default_,
    values: &dyn Values,
    scope: Scope,
    key: &str,
) -> bool {
    use crate::settings::Default_::{Bool, Number, Text};
    match choice {
        Bool(wanted) => values.bool_at(scope, key) == wanted,
        // Compared as an exact `f64`, which is right here and would not be in general:
        // these are the small integers an enum is stored as, and `electron-store` wrote
        // them from the same set.
        Number(wanted) => (values.number_at(scope, key) - wanted).abs() < f64::EPSILON,
        Text(wanted) => values.text_at(scope, key) == wanted,
    }
}

/// A choice's value, as `config.json` holds it.
fn as_json(value: crate::settings::Default_) -> Value {
    use crate::settings::Default_::{Bool, Number, Text};
    match value {
        Bool(value) => json!(value),
        Number(value) => json!(value),
        Text(value) => json!(value),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::{Values, path};
    use crate::config::Config;
    use crate::settings_screen::Scope;
    use serde_json::json;

    /// The lobby rules go under `localLobbySettings`, where `SettingsStore.tsx` puts them.
    /// Flattened, 1.x would not find a single one of them — and this client would not find
    /// what 1.x had written.
    #[test]
    fn a_lobby_rule_is_written_where_the_other_client_looks_for_it() {
        assert_eq!(path(Scope::Client, "masterVolume"), "masterVolume");
        assert_eq!(
            path(Scope::Lobby, "haunting"),
            "localLobbySettings.haunting"
        );
    }

    /// And reading goes to the same place, which is what makes the round trip work against
    /// a file the Electron client wrote.
    #[test]
    fn the_two_scopes_read_from_the_two_places() {
        let mut config = Config::new();
        config.set("localLobbySettings.maxDistance", json!(7.5));
        config.set("masterVolume", json!(42));
        assert!((Values::number_at(&config, Scope::Lobby, "maxDistance") - 7.5).abs() < 1e-9);
        assert!((Values::number_at(&config, Scope::Client, "masterVolume") - 42.0).abs() < 1e-9);
        // And not from each other's: `maxDistance` at the top level is a different key
        // from `maxDistance` under the lobby rules, and reading the wrong one would give
        // every player the host's voice distance or none of them it.
        assert!((Values::number_at(&config, Scope::Client, "maxDistance") - 7.5).abs() > 1e-9);
    }

    /// A setting that is not in the file still reads as its documented default through the
    /// same path, which is what a fresh installation depends on.
    #[test]
    fn a_missing_value_still_comes_back_as_its_default() {
        let config = Config::new();
        assert!(!Values::bool_at(&config, Scope::Client, "alwaysOnTop"));
        assert!(!Values::text_at(&config, Scope::Client, "serverURL").is_empty());
    }
}
