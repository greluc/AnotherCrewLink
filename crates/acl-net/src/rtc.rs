//! The layer between this crate's decisions and the `webrtc` crate.
//!
//! Translation, and as little of it as possible. Everything that decides anything is in
//! [`crate::ice`], [`crate::peer`], [`crate::mesh`] and [`crate::reconnect`], where it can
//! be tested without a network; this turns those answers into calls, and turns the
//! crate's events back into questions for them.
//!
//! The one thing worth watching here is what the conversion drops. A relay entry that
//! loses its credentials on the way through gathers no candidate at all, and the failure
//! looks like a relay that is down rather than a client that forgot to send a password —
//! which is why the conversions have tests of their own even though they are eight lines.

use webrtc::peer_connection::{
    RTCBundlePolicy, RTCConfiguration, RTCConfigurationBuilder, RTCIceServer, RTCIceTransportPolicy,
};

use crate::ice::{BundlePolicy, IceServer, IceTransportPolicy, RtcConfig};

/// Turns one advertised server into the crate's shape.
///
/// `username` and `credential` are `Option<String>` here and `String` there, so an absent
/// credential becomes an empty one. That is the same thing to a STUN server, which has no
/// authentication, and it is the only sound mapping: a relay without credentials is
/// refused by the relay rather than by this function, and refusing it here would hide a
/// server's misconfiguration behind a client error.
#[must_use]
pub fn to_ice_server(server: &IceServer) -> RTCIceServer {
    RTCIceServer {
        urls: server.urls.clone(),
        username: server.username.clone().unwrap_or_default(),
        credential: server.credential.clone().unwrap_or_default(),
    }
}

/// Turns the whole configuration into the crate's shape.
#[must_use]
pub fn to_configuration(config: &RtcConfig) -> RTCConfiguration {
    RTCConfigurationBuilder::default()
        .with_ice_servers(config.ice_servers.iter().map(to_ice_server).collect())
        .with_ice_transport_policy(match config.ice_transport_policy {
            IceTransportPolicy::All => RTCIceTransportPolicy::All,
            IceTransportPolicy::Relay => RTCIceTransportPolicy::Relay,
        })
        .with_bundle_policy(match config.bundle_policy {
            BundlePolicy::MaxBundle => RTCBundlePolicy::MaxBundle,
        })
        .build()
}

#[cfg(test)]
mod tests {
    // A test that cannot unwrap has to invent error handling for cases that cannot
    // happen, which is noise around the thing being checked.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn relay() -> IceServer {
        IceServer {
            urls: vec!["turn:relay.example:3478".to_owned()],
            username: Some("u".to_owned()),
            credential: Some("p".to_owned()),
        }
    }

    #[test]
    fn a_relay_keeps_its_credentials() {
        // Losing them produces a candidate that never gathers, and a failure that looks
        // like the relay is down rather than like this client forgot the password.
        let converted = to_ice_server(&relay());
        assert_eq!(converted.username, "u");
        assert_eq!(converted.credential, "p");
        assert_eq!(converted.urls, ["turn:relay.example:3478"]);
    }

    #[test]
    fn a_stun_server_without_credentials_converts_to_empty_ones() {
        // The crate has no way to say "absent". Empty is what a STUN server expects, and
        // a relay that needs credentials refuses an empty pair itself — which is the
        // right place for that to be noticed.
        let converted = to_ice_server(&IceServer::new("stun:stun.example:3478"));
        assert!(converted.username.is_empty());
        assert!(converted.credential.is_empty());
    }

    #[test]
    fn every_server_survives_the_conversion_in_order() {
        // ICE tries candidates in the order it is given them, and `with_tcp_relays` puts
        // UDP first on purpose. A conversion that reordered would undo that quietly.
        let config = RtcConfig::new(&[IceServer::new("stun:stun.example:3478"), relay()], false);
        let converted = to_configuration(&config);
        let urls: Vec<&str> = converted
            .ice_servers()
            .iter()
            .flat_map(|server| server.urls.iter().map(String::as_str))
            .collect();
        assert_eq!(
            urls,
            [
                "stun:stun.example:3478",
                "turn:relay.example:3478",
                "turn:relay.example:3478?transport=tcp",
            ]
        );
    }

    #[test]
    fn the_transport_policy_carries_across() {
        assert_eq!(
            to_configuration(&RtcConfig::new(&[relay()], true)).ice_transport_policy(),
            RTCIceTransportPolicy::Relay
        );
        assert_eq!(
            to_configuration(&RtcConfig::new(&[relay()], false)).ice_transport_policy(),
            RTCIceTransportPolicy::All
        );
    }

    #[test]
    fn the_bundle_policy_is_never_left_unspecified() {
        // The crate's default is `Unspecified`, which is not the same as `MaxBundle` and
        // would cost a second relay allocation per peer. This is the conversion that must
        // not be forgotten, and the one that would be invisible if it were.
        assert_eq!(
            to_configuration(&RtcConfig::new(&[], false)).bundle_policy(),
            RTCBundlePolicy::MaxBundle
        );
        assert_ne!(RTCBundlePolicy::default(), RTCBundlePolicy::MaxBundle);
    }
}
