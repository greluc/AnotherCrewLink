//! The peer configuration a server sends on connect, and what the client does with it.
//!
//! A straight port of `src/renderer/validateClientPeerConfig.ts` and its tests, plus the
//! composition step that lives inline in `Voice.tsx`'s `clientPeerConfig` handler.
//!
//! This runs against whatever the connected server sends, and the user can point the
//! client at any server they like, so the interesting cases are the malformed ones. The
//! TypeScript replaced an Ajv schema with a hand-written check because Ajv compiles
//! schemas by evaluating generated JavaScript, which a Content-Security-Policy without
//! `unsafe-eval` forbids. Here the reason is different and the shape is the same: the
//! error paths are part of the contract, because they are what a server operator reads
//! when their deployment is refused.
//!
//! The validator returns the parsed configuration rather than a boolean beside a mutable
//! list of errors. That was an artefact of mirroring what Ajv exposed; a `Result` says
//! the same thing and cannot be read stale.

use serde_json::Value;

use crate::ice::{IceServer, RtcConfig, has_relay};

/// A peer configuration that has been checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerConfig {
    /// Whether the server asks that only relayed candidates be used.
    pub force_relay_only: bool,
    /// The servers to gather candidates from, exactly as advertised.
    pub ice_servers: Vec<IceServer>,
}

/// Why a configuration was not used, in the two ways that can happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// It did not have the shape of a configuration. Carries one message per fault, each
    /// naming the path it was found at.
    Malformed(Vec<String>),
    /// It asked for relay-only and advertised no relay.
    ///
    /// Relay rule three of §4.6: forcing relay mode with nothing to relay through leaves
    /// the connection unable to gather any candidate at all, so a peer that sometimes
    /// connected directly stops connecting ever. The default configuration is kept
    /// instead, and this is not silent — from the player's side a misconfigured server
    /// and a broken client look identical.
    RelayForcedWithoutRelay,
}

/// Whether a value is a URI, matching what the TypeScript accepted.
///
/// The TypeScript tried a `stun`/`turn` scheme first and fell back to the WHATWG URL
/// parser, which accepts any valid scheme followed by a colon — `foo:bar` and even `foo:`
/// parse. So the effective rule is "a valid RFC 3986 scheme and a colon", and that is
/// what this implements rather than pulling in a URL parser for a check this narrow.
///
/// Measured against the original for sixteen inputs rather than derived from the
/// specification, which is how the leading-whitespace case was found: WHATWG strips
/// leading and trailing C0 controls and spaces before it looks at anything, so
/// `"  stun:a:1"` is accepted.
fn is_uri(value: &str) -> bool {
    let trimmed = value.trim_matches(|c: char| c <= ' ');
    let Some((scheme, _)) = trimmed.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Reads the optional `username` or `credential`, which must be strings when present.
fn optional_string(
    server: &serde_json::Map<String, Value>,
    key: &str,
    where_: &str,
    errors: &mut Vec<String>,
) -> Result<Option<String>, ()> {
    match server.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => {
            errors.push(format!("{where_}/{key} must be a string"));
            Err(())
        }
    }
}

/// Checks one entry of `iceServers`, reporting at most one fault for it.
fn check_ice_server(server: &Value, index: usize, errors: &mut Vec<String>) -> Option<IceServer> {
    let where_ = format!("/iceServers/{index}");
    let Some(candidate) = server.as_object() else {
        errors.push(format!("{where_} must be an object"));
        return None;
    };

    let urls = match candidate.get("urls") {
        None | Some(Value::Null) => {
            errors.push(format!("{where_}/urls is required"));
            return None;
        }
        Some(Value::Array(entries)) => {
            let mut urls = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry.as_str() {
                    Some(url) if is_uri(url) => urls.push(url.to_owned()),
                    _ => {
                        errors.push(format!("{where_}/urls must contain only URIs"));
                        return None;
                    }
                }
            }
            urls
        }
        Some(Value::String(url)) if is_uri(url) => vec![url.clone()],
        Some(_) => {
            errors.push(format!("{where_}/urls must be a URI"));
            return None;
        }
    };

    let username = optional_string(candidate, "username", &where_, errors).ok()?;
    let credential = optional_string(candidate, "credential", &where_, errors).ok()?;

    Some(IceServer {
        urls,
        username,
        credential,
    })
}

/// Validates the peer configuration a server sent.
///
/// On failure every fault found is reported, each naming its path, because a server
/// operator fixing a deployment wants the whole list rather than the first line of it.
///
/// # Errors
///
/// Returns one message per fault when the value is not a well-formed configuration.
pub fn validate_peer_config(value: &Value) -> Result<PeerConfig, Vec<String>> {
    let mut errors = Vec::new();

    let Some(config) = value.as_object() else {
        return Err(vec![" must be an object".to_owned()]);
    };

    // Not an early return: a configuration can be wrong in both ways at once, and saying
    // so in one pass is the difference between one round trip to the operator and two.
    let force_relay_only = if let Some(Value::Bool(value)) = config.get("forceRelayOnly") {
        *value
    } else {
        errors.push("/forceRelayOnly must be a boolean".to_owned());
        false
    };

    let Some(entries) = config.get("iceServers").and_then(Value::as_array) else {
        errors.push("/iceServers must be an array".to_owned());
        return Err(errors);
    };

    let mut ice_servers = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        if let Some(server) = check_ice_server(entry, index, &mut errors) {
            ice_servers.push(server);
        }
    }

    if errors.is_empty() {
        Ok(PeerConfig {
            force_relay_only,
            ice_servers,
        })
    } else {
        Err(errors)
    }
}

/// Turns what a server sent into the configuration connections are opened with.
///
/// Two refusals, and they are kept apart because they mean different things to whoever
/// has to fix them: a malformed configuration is a bug in the server, and a relay-only
/// configuration with no relay is a deployment that is merely incomplete. In both cases
/// the caller keeps the default configuration rather than using a broken one.
///
/// # Errors
///
/// Returns [`Rejection`] when the configuration is malformed, or when it forces relay
/// mode without advertising a relay.
pub fn apply_client_peer_config(value: &Value) -> Result<RtcConfig, Rejection> {
    let config = validate_peer_config(value).map_err(Rejection::Malformed)?;

    if config.force_relay_only && !has_relay(&config.ice_servers) {
        return Err(Rejection::RelayForcedWithoutRelay);
    }

    Ok(RtcConfig::new(&config.ice_servers, config.force_relay_only))
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::ice::IceTransportPolicy;
    use serde_json::json;

    fn valid() -> Value {
        json!({
            "forceRelayOnly": false,
            "iceServers": [
                { "urls": "stun:stun.l.google.com:19302" },
                { "urls": "turn:turn.example.com:3478", "username": "u", "credential": "c" },
            ],
        })
    }

    fn errors(value: &Value) -> String {
        validate_peer_config(value)
            .expect_err("the configuration should have been refused")
            .join(",")
    }

    #[test]
    fn accepts_what_the_server_actually_sends() {
        let config = validate_peer_config(&valid()).expect("this is the live shape");
        assert!(!config.force_relay_only);
        assert_eq!(config.ice_servers.len(), 2);
        assert_eq!(config.ice_servers[1].username.as_deref(), Some("u"));
    }

    #[test]
    fn accepts_an_array_of_urls() {
        let value = json!({ "forceRelayOnly": false, "iceServers": [{ "urls": ["stun:a:1", "turn:b:2"] }] });
        let config = validate_peer_config(&value).expect("a list of URLs is allowed on the wire");
        assert_eq!(config.ice_servers[0].urls, ["stun:a:1", "turn:b:2"]);
    }

    #[test]
    fn rejects_a_missing_force_relay_only() {
        let value = json!({ "iceServers": [] });
        assert!(errors(&value).contains("forceRelayOnly"));
    }

    #[test]
    fn rejects_force_relay_only_of_the_wrong_type() {
        let value = json!({ "forceRelayOnly": "yes", "iceServers": [] });
        assert!(validate_peer_config(&value).is_err());
    }

    #[test]
    fn rejects_ice_servers_that_is_not_an_array() {
        let value = json!({ "forceRelayOnly": false, "iceServers": {} });
        assert!(errors(&value).contains("iceServers"));
    }

    #[test]
    fn rejects_an_ice_server_without_urls() {
        let value = json!({ "forceRelayOnly": false, "iceServers": [{ "username": "u" }] });
        assert!(errors(&value).contains("urls is required"));
    }

    #[test]
    fn rejects_a_non_uri_url() {
        let value = json!({ "forceRelayOnly": false, "iceServers": [{ "urls": "not a uri" }] });
        assert!(validate_peer_config(&value).is_err());
    }

    #[test]
    fn rejects_credentials_of_the_wrong_type() {
        let value = json!({ "forceRelayOnly": false, "iceServers": [{ "urls": "stun:a:1", "credential": 42 }] });
        assert!(validate_peer_config(&value).is_err());
    }

    #[test]
    fn rejects_null_and_non_objects_outright() {
        assert!(validate_peer_config(&Value::Null).is_err());
        assert!(validate_peer_config(&json!("nope")).is_err());
    }

    #[test]
    fn reports_the_position_of_the_offending_server() {
        let value = json!({ "forceRelayOnly": false, "iceServers": [{ "urls": "stun:a:1" }, { "urls": 5 }] });
        assert!(errors(&value).contains("/iceServers/1/urls"));
    }

    #[test]
    fn reports_every_fault_rather_than_the_first() {
        // The TypeScript accumulated too, and a server operator fixing a deployment wants
        // the whole list rather than one round trip per line.
        let value = json!({ "forceRelayOnly": 1, "iceServers": [{ "urls": 5 }, { "urls": 6 }] });
        let reported = validate_peer_config(&value).expect_err("three faults");
        assert_eq!(reported.len(), 3);
    }

    #[test]
    fn a_second_call_does_not_see_the_first_call_s_errors() {
        // The TypeScript kept one module-level array and cleared it on entry, which is
        // the bug this shape cannot have. Kept as a test because the behaviour is what
        // callers relied on.
        assert!(validate_peer_config(&Value::Null).is_err());
        assert!(validate_peer_config(&valid()).is_ok());
    }

    #[test]
    fn is_uri_matches_the_typescript_it_replaces() {
        // Every one of these was run through the original before it was written down.
        for accepted in [
            "stun:a:1",
            "TURN:a:1",
            "turns:a:1",
            "https://example.com",
            "http://x",
            "foo:bar",
            "foo:",
            "mailto:a@b",
            "turn:relay.example:3478?transport=tcp",
            "  stun:a:1",
        ] {
            assert!(is_uri(accepted), "{accepted} should be accepted");
        }
        for refused in ["not a uri", "", "a", "://x", ":x", "1foo:bar"] {
            assert!(!is_uri(refused), "{refused} should be refused");
        }
    }

    #[test]
    fn a_good_configuration_becomes_one_connections_can_use() {
        let config = apply_client_peer_config(&valid()).expect("the live shape is usable");
        assert_eq!(config.ice_transport_policy, IceTransportPolicy::All);
        // The bare relay gained its TCP form on the way through.
        assert_eq!(
            config.urls(),
            [
                "stun:stun.l.google.com:19302",
                "turn:turn.example.com:3478",
                "turn:turn.example.com:3478?transport=tcp",
            ]
        );
    }

    #[test]
    fn a_malformed_configuration_is_refused_with_its_faults() {
        let outcome = apply_client_peer_config(&json!({ "iceServers": [] }));
        assert!(matches!(outcome, Err(Rejection::Malformed(faults)) if !faults.is_empty()));
    }

    /// Relay rule three of §4.6, and the one a counter-based rule reaches for: forcing
    /// relay-only with nothing to relay through leaves the connection with no candidates
    /// at all, so a peer that sometimes connected directly stops connecting ever.
    #[test]
    fn relay_only_without_a_relay_is_refused_rather_than_obeyed() {
        let value = json!({
            "forceRelayOnly": true,
            "iceServers": [{ "urls": "stun:stun.l.google.com:19302" }],
        });
        assert_eq!(
            apply_client_peer_config(&value),
            Err(Rejection::RelayForcedWithoutRelay)
        );
    }

    #[test]
    fn relay_only_with_a_relay_is_obeyed() {
        let value = json!({
            "forceRelayOnly": true,
            "iceServers": [{ "urls": "turn:relay.example:3478", "username": "u", "credential": "p" }],
        });
        let config = apply_client_peer_config(&value).expect("there is a relay to force onto");
        assert_eq!(config.ice_transport_policy, IceTransportPolicy::Relay);
    }
}
