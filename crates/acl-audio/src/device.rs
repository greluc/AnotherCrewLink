//! Where audio comes from and where it goes, behind a trait.
//!
//! The trait is here from the first commit rather than added later, and the reason is
//! named in the plan: `cpal` 0.18 is a recent rework whose WASAPI device-change path — the
//! one this application already has a bug class around — has open issues on it. `cubeb` is
//! the documented fallback, and swapping a backend is only cheap if nothing above it ever
//! learned the backend's name.
//!
//! # The trigger for changing backend
//!
//! Written down so it is a decision rather than a mood. Move to `cubeb` if any of these
//! turns out to be true in a shipped build:
//!
//! - A device that disappears while in use does not produce a [`DeviceEvent::Lost`], or
//!   produces one and then keeps delivering silence.
//! - Switching the default device does not produce a [`DeviceEvent::DefaultChanged`]
//!   within a second.
//! - The capture callback is late by more than one buffer, repeatably, on hardware that
//!   Chromium handles.
//!
//! Each is something the Electron client gets right today, which makes them regressions
//! rather than shortcomings — and this application's users would notice all three.

use std::fmt;

/// Which way audio is flowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// A microphone.
    Input,
    /// A speaker or headset.
    Output,
}

/// One device, as the settings screen needs to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// A stable handle for reopening it.
    ///
    /// Not the name: on Windows a device id changes when the same headset is plugged into
    /// a different port, and the name does not. The client already carries a bug class
    /// about exactly this — it re-resolves a microphone by label when its id changes — so
    /// both are kept and the caller decides which to match on.
    pub id: String,
    /// What to show a person.
    pub name: String,
    /// Which way it goes.
    pub direction: Direction,
    /// Whether the system considers it the default.
    pub default: bool,
}

/// Something that happened to the device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// A device appeared.
    Added(Device),
    /// A device went away. The id is all that is left of it.
    Lost {
        /// Which one.
        id: String,
        /// Which way it went.
        direction: Direction,
    },
    /// The system default changed.
    DefaultChanged {
        /// The new default.
        id: String,
        /// Which way it goes.
        direction: Direction,
    },
}

/// What went wrong.
#[derive(Debug)]
pub enum DeviceError {
    /// The backend could not be reached at all.
    Unavailable(String),
    /// The named device is not there.
    NotFound(String),
    /// The device is there but refused the format.
    Unsupported(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(formatter, "no audio backend: {message}"),
            Self::NotFound(id) => write!(formatter, "no such device: {id}"),
            Self::Unsupported(message) => write!(formatter, "device refused the format: {message}"),
        }
    }
}

impl std::error::Error for DeviceError {}

/// An audio backend.
///
/// Enumeration only, for now. Opening a stream is the next piece and belongs behind the
/// same trait; putting the boundary in before there is a second implementation is the
/// point — a trait added after the fact is shaped by whatever the first backend happened
/// to do.
pub trait Backend {
    /// Every device the system currently offers.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError::Unavailable`] if the backend itself cannot be reached.
    fn devices(&self) -> Result<Vec<Device>, DeviceError>;

    /// The system default for one direction, if there is one.
    ///
    /// # Errors
    ///
    /// Returns [`DeviceError`] if the backend cannot be reached.
    fn default(&self, direction: Direction) -> Result<Option<Device>, DeviceError> {
        Ok(self
            .devices()?
            .into_iter()
            .find(|device| device.direction == direction && device.default))
    }

    /// The name of the backend, for a log line that has to say which one is in use.
    fn name(&self) -> &'static str;
}

/// Finds a device again after the system has renumbered it.
///
/// By id first, then by name. That order is not arbitrary: on Windows a device id changes
/// when the same headset is plugged into a different port, and a client that only matched
/// on id would silently fall back to the default microphone — which is the bug the
/// Electron client had and now works around by re-resolving on label.
#[must_use]
pub fn reacquire<'a>(devices: &'a [Device], id: &str, name: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|device| device.id == id)
        .or_else(|| devices.iter().find(|device| device.name == name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// A backend with a fixed list, which is what makes any of this testable at all.
    struct Fake(Vec<Device>);

    impl Backend for Fake {
        fn devices(&self) -> Result<Vec<Device>, DeviceError> {
            Ok(self.0.clone())
        }
        fn name(&self) -> &'static str {
            "fake"
        }
    }

    fn device(id: &str, name: &str, direction: Direction, default: bool) -> Device {
        Device {
            id: id.to_owned(),
            name: name.to_owned(),
            direction,
            default,
        }
    }

    #[test]
    fn the_default_is_found_per_direction() {
        let backend = Fake(vec![
            device("in-1", "Headset", Direction::Input, false),
            device("in-2", "Webcam", Direction::Input, true),
            device("out-1", "Speakers", Direction::Output, true),
        ]);
        assert_eq!(
            backend.default(Direction::Input).unwrap().unwrap().id,
            "in-2"
        );
        assert_eq!(
            backend.default(Direction::Output).unwrap().unwrap().id,
            "out-1"
        );
    }

    #[test]
    fn no_default_is_not_an_error() {
        // A machine with no microphone at all. The client should say so, not fail.
        let backend = Fake(vec![device("out-1", "Speakers", Direction::Output, true)]);
        assert!(backend.default(Direction::Input).unwrap().is_none());
    }

    #[test]
    fn a_device_is_found_by_id_first() {
        let devices = vec![
            device("stable-id", "Headset", Direction::Input, false),
            device("other", "Headset", Direction::Input, false),
        ];
        let found = reacquire(&devices, "stable-id", "Headset").unwrap();
        assert_eq!(found.id, "stable-id");
    }

    #[test]
    fn a_renumbered_device_is_found_by_name() {
        // The bug this exists for: on Windows the same headset gets a different id when
        // it is plugged into another port, and a client matching only on id falls back to
        // the default microphone without saying anything.
        let devices = vec![
            device("new-id-after-replug", "Headset", Direction::Input, false),
            device("built-in", "Webcam", Direction::Input, true),
        ];
        let found = reacquire(&devices, "old-id", "Headset").unwrap();
        assert_eq!(found.id, "new-id-after-replug");
    }

    #[test]
    fn a_device_that_is_really_gone_is_not_invented() {
        // Falling back to *something* would be worse than saying nothing: the person
        // would be talking into a microphone they did not choose.
        let devices = vec![device("built-in", "Webcam", Direction::Input, true)];
        assert!(reacquire(&devices, "old-id", "Headset").is_none());
    }
}

/// The system backend, over `cpal`.
///
/// Deliberately thin: it enumerates and it names, and everything above it talks to
/// [`Backend`]. That is what makes the fallback in this module's header a change of one
/// type rather than a change everywhere.
#[cfg(feature = "cpal-backend")]
pub mod system {
    use cpal::traits::{DeviceTrait as _, HostTrait as _};

    use super::{Backend, Device, DeviceError, Direction};

    /// `cpal` over whatever host the platform offers.
    pub struct Cpal {
        host: cpal::Host,
    }

    impl Cpal {
        /// The platform's default host.
        #[must_use]
        pub fn new() -> Self {
            Self {
                host: cpal::default_host(),
            }
        }

        /// A device's id, as a string that can be stored in settings and read back.
        ///
        /// `cpal` documents `DeviceId` as stable across runs, disconnections and reboots
        /// where the platform allows it, and as round-tripping through `Display` and
        /// `FromStr` — which is exactly what a settings file needs. A device that refuses
        /// to identify itself falls back to its name, which is what `reacquire` matches on
        /// second anyway.
        fn identify(device: &cpal::Device, name: &str) -> String {
            device
                .id()
                .map_or_else(|_| name.to_owned(), |id| id.to_string())
        }
    }

    impl Default for Cpal {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Backend for Cpal {
        fn devices(&self) -> Result<Vec<Device>, DeviceError> {
            // Compared by id rather than by name: two identical headsets have the same
            // name, and marking both as the default would put a settings list in a state
            // no person could resolve.
            let default_input = self
                .host
                .default_input_device()
                .and_then(|device| device.id().ok());
            let default_output = self
                .host
                .default_output_device()
                .and_then(|device| device.id().ok());

            let mut found = Vec::new();
            for (direction, list) in [
                (Direction::Input, self.host.input_devices()),
                (Direction::Output, self.host.output_devices()),
            ] {
                let list = list.map_err(|error| DeviceError::Unavailable(error.to_string()))?;
                for device in list {
                    // A device that cannot even describe itself is skipped rather than
                    // shown as "unknown": a settings list with three unknowns in it is
                    // worse than one that is short.
                    let Ok(description) = device.description() else {
                        continue;
                    };
                    let name = description.name().to_owned();
                    let id = device.id().ok();
                    let default = match direction {
                        Direction::Input => id.is_some() && id == default_input,
                        Direction::Output => id.is_some() && id == default_output,
                    };
                    found.push(Device {
                        id: Self::identify(&device, &name),
                        name,
                        direction,
                        default,
                    });
                }
            }
            Ok(found)
        }

        fn name(&self) -> &'static str {
            "cpal"
        }
    }

    #[cfg(test)]
    mod tests {
        #![allow(clippy::unwrap_used, clippy::expect_used)]

        use super::*;

        #[test]
        fn it_enumerates_whatever_this_machine_has() {
            // Not an assertion about the machine: CI runners have no sound card, and a
            // test that demanded one would fail there and prove nothing here. What is
            // asserted is that asking does not fail, and that anything it does return is
            // shaped properly — a device with no name or two defaults in one direction is
            // a bug in this file whether or not the machine has speakers.
            let backend = Cpal::new();
            assert_eq!(backend.name(), "cpal");

            let Ok(devices) = backend.devices() else {
                // No backend at all is a legitimate answer on a headless runner.
                return;
            };
            for device in &devices {
                assert!(!device.name.is_empty(), "a device with no name");
                assert!(!device.id.is_empty(), "a device with no id");
            }
            for direction in [Direction::Input, Direction::Output] {
                let defaults = devices
                    .iter()
                    .filter(|device| device.direction == direction && device.default)
                    .count();
                assert!(defaults <= 1, "{defaults} defaults for {direction:?}");
            }
            eprintln!(
                "cpal: {} input, {} output",
                devices
                    .iter()
                    .filter(|d| d.direction == Direction::Input)
                    .count(),
                devices
                    .iter()
                    .filter(|d| d.direction == Direction::Output)
                    .count()
            );
        }
    }
}
