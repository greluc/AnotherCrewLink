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

use egui::{ComboBox, Ui};
use serde_json::{Value, json};

use crate::settings_screen::{
    Control, Kind, SECTIONS, Scope, availability, gate_is_its_own_control, shown, stored,
};
use crate::views::theme;

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
    /// One setting goes back to the value the schema gives it.
    ///
    /// A `Change` of its own rather than a `Set` with the default in it, because the view
    /// does not know what any setting defaults to and should not have to: the schema is the
    /// one place that says, and it is the caller's.
    Reset {
        /// Which setting.
        key: &'static str,
        /// Whose it is, which is what says where it is written.
        scope: Scope,
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
    /// Whether a test tone is playing, which decides what the button says.
    pub testing_speaker: bool,
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
    if theme::checkbox(ui, &mut on, label).changed() {
        changes.push(Change::Set {
            key: gate,
            scope,
            value: json!(on),
            warning: None,
        });
    }
}

/// A fixed set of values, as a combo box.
///
/// Lifted out of `one` so that function stays under the line limit the workspace sets. It
/// is also the arm with the most in it: a current value to find, a label to translate for
/// the closed box and one for every entry in the open one.
fn choice(
    ui: &mut Ui,
    control: &'static Control,
    scope: Scope,
    options: &'static [crate::settings_screen::Choice],
    values: &dyn Values,
    context: &Context<'_>,
    changes: &mut Vec<Change>,
) {
    let current = options
        .iter()
        .find(|choice| matches(choice.value, values, scope, control.key));
    // A stored value none of the options offers shows as *something*. The launch platform
    // is the case that made this visible: choosing a custom one leaves the picker blank,
    // because the three built-ins are the only entries and none of them matches. A blank
    // box reads as "nothing is chosen" when something is.
    let shown_label = current.map_or_else(
        || (context.t)("platform.custom"),
        |choice| (context.t)(choice.label),
    );
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
                    let value = as_json(choice.value);
                    changes.push(Change::Set {
                        key: control.key,
                        scope,
                        warning: control.warning.filter(|_| {
                            control
                                .warn_when
                                .applies(&held_value(values, scope, control), &value)
                        }),
                        value,
                    });
                }
            }
        });
}

/// One free-text setting.
///
/// A function of its own because the buffer has to outlive the frame, and `one` was
/// already at the line limit.
fn text_field(
    ui: &mut Ui,
    control: &'static Control,
    scope: Scope,
    label: &str,
    values: &dyn Values,
    changes: &mut Vec<Change>,
) {
    ui.label(label);
    // The buffer lives in `egui`'s own per-widget memory rather than being rebuilt
    // from the file every frame, and that is the whole of a fix from 2026-08-29.
    //
    // `let mut text = values.text_at(...)` read the *stored* value on every pass.
    // The field is drawn five times a second at least, so a keystroke was written
    // into a `String` that was thrown away before the next frame recreated it from
    // the file -- and the file only changes on `lost_focus`, which never came,
    // because there was never anything in the field to lose. Neither of the two
    // text settings could be typed into at all: the voice server address and the
    // public lobby's title.
    //
    // Held only while the field has focus. Outside that it follows the file, so a
    // value changed elsewhere -- restore defaults, a hand-edited settings file --
    // shows up rather than being masked by a stale draft.
    let id = ui.make_persistent_id(("settings-text", path(scope, control.key)));
    let stored = values.text_at(scope, control.key);
    let mut text = ui
        .data(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| stored.clone());
    let field = ui.text_edit_singleline(&mut text);
    // On losing focus rather than on every keystroke. `serverURL` is validated and
    // reconnected to when it changes, and half a URL is a server that does not
    // exist.
    if field.lost_focus() {
        let value = json!(text);
        changes.push(Change::Set {
            key: control.key,
            scope,
            // A text field has no direction, so its warning -- the voice server's --
            // applies to any change. See `WarnWhen`.
            warning: control
                .warning
                .filter(|_| control.warn_when.applies(&json!(stored), &value)),
            value,
        });
        ui.data_mut(|data| data.remove::<String>(id));
    } else if field.has_focus() {
        ui.data_mut(|data| data.insert_temp(id, text));
    } else {
        ui.data_mut(|data| data.remove::<String>(id));
    }
}

/// What is stored for a control right now, as JSON.
///
/// `WarnWhen` compares the value being asked for against the one it replaces, and the two
/// have to be the same shape. `Values` answers by type, so this asks for the one the
/// control's kind implies.
fn held_value(values: &dyn Values, scope: Scope, control: &Control) -> Value {
    match control.kind {
        Kind::Toggle { inverted } => json!(values.bool_at(scope, control.key) != inverted),
        Kind::Slider { .. } => json!(values.number_at(scope, control.key)),
        Kind::Text | Kind::Shortcut | Kind::Device { .. } | Kind::Language => {
            json!(values.text_at(scope, control.key))
        }
        // An action, a meter, a reset or a probe: nothing is stored, so there is nothing
        // for a condition to compare against and `WarnWhen::Always` is what they carry.
        _ => Value::Null,
    }
}

/// What the microphone is hearing, as a bar.
///
/// A bar rather than a number. The question it answers is "is it hearing me", and the
/// answer is whether the thing moves when somebody speaks — a figure would have to be read
/// and compared against another figure to say the same thing.
fn meter(ui: &mut Ui, level: Option<f32>) {
    // Green when it is live and grey when nothing has reported: an empty grey bar reads as
    // "not listening", and an empty green one as "listening, and hearing silence".
    //
    // **A deviation from the design system, decided by the maintainer on 2026-08-28.**
    // `reference.json` gives the microphone bar `--acl-red-500`, one colour whatever the
    // state. That loses the distinction above, which is the answer to "is the microphone
    // even open" -- the first thing anybody checks when nobody can hear them. Put to the
    // maintainer with the trade-off stated and kept as it is.
    let fill = if level.is_some() {
        theme::TALKING
    } else {
        ui.visuals().weak_text_color()
    };
    // `--mic-bar-w` by `--mic-bar-h`. The height was never set, so egui took the row --
    // 24px -- and `--radius-lg` turned a 200x24 bar into a capsule with a dot in it.
    ui.add(
        egui::ProgressBar::new(level.unwrap_or(0.0).clamp(0.0, 1.0))
            .desired_width(theme::METER_W)
            .desired_height(theme::METER_H)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_SM))
            .fill(fill),
    );
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
        // The alternative first, if its setting is on: one slider says a different thing
        // about the same number depending on a neighbour. See `settings_screen::Instead`.
        control
            .instead
            .filter(|instead| values.bool_at(scope, instead.when))
            .map_or_else(
                || control.label.map(context.t).unwrap_or_default(),
                |instead| (context.t)(instead.label),
            )
    };
    // The warning only travels with a change it applies to. `Settings.tsx` passes a
    // `showDialog` condition at every one of its eight warned call sites and the port
    // ignored all of them, so turning the public lobby *off* raised the dialog about
    // turning it on, switching the voice detector back *on* raised the one about switching
    // it off, and the sensitivity slider raised its dialog on every tick of a drag.
    let set = |value: Value| Change::Set {
        key: control.key,
        scope,
        warning: control.warning.filter(|_| {
            control
                .warn_when
                .applies(&held_value(values, scope, control), &value)
        }),
        value,
    };
    match control.kind {
        Kind::Toggle { inverted } => {
            // What the box shows, which is not always what is stored. See `Kind::Toggle`.
            let stored = values.bool_at(scope, control.key);
            let mut shown = stored != inverted;
            if theme::checkbox(ui, &mut shown, label).changed() {
                changes.push(set(json!(shown != inverted)));
            }
        }
        Kind::Slider { min, max, step, .. } => {
            let mut value = shown(control.kind, values.number_at(scope, control.key));
            // The number belongs in the label -- "Voice Distance: 5.3" -- which is where
            // `forms/Slider.jsx` puts it, and it is one decimal when a step can land
            // between whole numbers and none when it cannot. A volume that steps by two
            // has no business reading 100.00.
            //
            // Empty when the gating checkbox above already carries the words, so the row
            // is the number alone rather than a colon with nothing in front of it.
            let decimals = usize::from(step.fract() != 0.0);
            ui.label(if label.is_empty() {
                format!("{value:.decimals$}")
            } else {
                format!("{label}: {value:.decimals$}")
            });
            if theme::slider(ui, &mut value, min..=max, step).changed() {
                changes.push(set(json!(stored(control.kind, value))));
            }
        }
        Kind::Choice(options) => choice(ui, control, scope, options, values, context, changes),
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
        Kind::Meter => meter(ui, context.input_level),
        Kind::Text => text_field(ui, control, scope, &label, values, changes),
        Kind::Action => {
            if ui.button(label).clicked() {
                changes.push(Change::Run {
                    key: control.key,
                    warning: control.warning,
                });
            }
        }
        Kind::Reset { setting } => {
            // The schema's own value for that key, which the caller looks up: this view
            // knows what a control is, not what any setting defaults to.
            if ui.button(label).clicked() {
                changes.push(Change::Reset {
                    key: setting,
                    scope,
                });
            }
        }
        Kind::Note => {
            // Weak and wrapped: it is a rule to be read once, not a label competing with
            // the fields above it.
            ui.label(egui::RichText::new(label).weak().small());
        }
        Kind::Probe => {
            // One button, two sentences. The shipped catalogue has both and only one was
            // ever shown, because the tone could start and not stop.
            let label = if control.key == "testSpeaker" && context.testing_speaker {
                (context.t)("settings.audio.test_speaker_stop")
            } else {
                label
            };
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
#[must_use]
pub fn as_json(value: crate::settings::Default_) -> Value {
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
