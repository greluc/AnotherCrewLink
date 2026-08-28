//! The editor for custom launch platforms.
//!
//! Not a [`crate::settings_screen::Control`], and it could not be: that model is a
//! compile-time constant, and this is a list whose length is whatever somebody has added.
//! So it is a view of its own, drawn under the settings page, and it returns [`Edit`]s the
//! same way the settings view returns changes — nothing here writes.
//!
//! The shapes it edits, and why they are not what the file holds, are in
//! [`crate::platforms`].

use crate::platforms::Entry;
use egui::Ui;

/// What somebody did to the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Add an entry under this name.
    Add(String),
    /// Remove the entry with this name.
    Remove(String),
    /// Store this entry as it now stands.
    Update(Entry),
    /// Make this platform the one the launch button uses.
    Use(String),
}

/// Everything the editor needs that is not the list.
pub struct Context<'a> {
    /// Looks up an i18n key.
    pub t: &'a dyn Fn(&str) -> String,
    /// Which platform `launchPlatform` currently names.
    pub chosen: &'a str,
    /// The name being typed into the "add" field, which the caller keeps.
    pub adding: &'a mut String,
}

/// Draws the editor and reports what was done to it.
///
/// The entries are drawn as the caller passes them, so a field being typed into is the
/// caller's copy — the alternative is a screen that rewrites the file on every keystroke,
/// which for a path means writing every prefix of it.
pub fn draw(ui: &mut Ui, entries: &[Entry], context: &mut Context<'_>) -> Vec<Edit> {
    let mut edits = Vec::new();
    let say = |key: &str| (context.t)(key);

    ui.heading(say("settings.customplatforms.title"));

    for entry in entries {
        let mut changed = entry.clone();
        ui.separator();
        ui.horizontal(|ui| {
            // Choosing one is the point of having it, and a list you can edit but not
            // select from is a list of things that do nothing.
            let mut chosen = context.chosen == entry.name;
            if ui.radio_value(&mut chosen, true, &entry.name).clicked() {
                edits.push(Edit::Use(entry.name.clone()));
            }
            if ui.button(say("buttons.delete")).clicked() {
                edits.push(Edit::Remove(entry.name.clone()));
            }
        });

        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut changed.is_uri,
                false,
                say("settings.customplatforms.path"),
            );
            ui.selectable_value(
                &mut changed.is_uri,
                true,
                say("settings.customplatforms.uri"),
            );
        });
        // One field, because a person has one path. It is split into a directory and a
        // program on the way to the file; see `platforms::to_stored`.
        ui.text_edit_singleline(&mut changed.path);
        if !changed.is_uri {
            // Behind a disclosure, as `CustomPlatformSettings.tsx` puts it: most people
            // need a path and nothing else, and a field they will not use is one more thing
            // to read past before the one they will.
            egui::CollapsingHeader::new(say("settings.customplatforms.advanced"))
                .id_salt((&entry.name, "advanced"))
                .show(ui, |ui| {
                    ui.label(say("settings.customplatforms.arguments"));
                    ui.text_edit_singleline(&mut changed.arguments);
                });
        }
        if changed != *entry {
            edits.push(Edit::Update(changed));
        }
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(say("settings.customplatforms.platform_title"));
        ui.text_edit_singleline(context.adding);
        // Refused rather than accepted and then ignored: a name that cannot be a key is one
        // whose entry could never be chosen or deleted, and the button saying so is better
        // than a row that appears and does nothing.
        let usable = crate::platforms::is_a_usable_name(context.adding);
        if ui
            .add_enabled(usable, egui::Button::new(say("buttons.default")))
            .clicked()
        {
            edits.push(Edit::Add(context.adding.trim().to_owned()));
            context.adding.clear();
        }
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::{Context, Edit, draw};
    use crate::platforms::Entry;

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            is_uri: false,
            path: r"C:\Games\Among Us\Among Us.exe".to_owned(),
            arguments: String::new(),
        }
    }

    /// Drawing it reports nothing, which is what a screen nobody has touched should do.
    ///
    /// The editor returns an `Update` whenever its copy differs from what it was given, so
    /// a pass with no interaction must produce none — otherwise every frame would rewrite
    /// the file.
    #[test]
    fn an_untouched_editor_changes_nothing() {
        let context = egui::Context::default();
        let entries = [entry("Mine"), entry("Other")];
        let mut adding = String::new();
        let mut seen = Vec::new();
        // Twice, because egui's first pass has no layout yet and the second is the one that
        // reflects a real frame -- the same reason `tests/views_draw.rs` runs two.
        for _ in 0..2 {
            seen.clear();
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                seen = draw(
                    ui,
                    &entries,
                    &mut Context {
                        t: &|key| key.to_owned(),
                        chosen: "Mine",
                        adding: &mut adding,
                    },
                );
            });
            // The font atlas comes back as a texture a renderer is expected to upload, and
            // epaint panics on dropping one nobody took. There is no renderer here.
            output.textures_delta.clear();
        }
        assert!(
            seen.is_empty(),
            "an untouched editor asked for {seen:?}, which would rewrite the file every frame"
        );
    }

    /// An `Edit` says what to do without doing it.
    #[test]
    fn the_edits_are_data() {
        assert_eq!(
            Edit::Remove("Mine".to_owned()),
            Edit::Remove("Mine".to_owned())
        );
        assert_ne!(
            Edit::Use("Mine".to_owned()),
            Edit::Remove("Mine".to_owned())
        );
    }
}
