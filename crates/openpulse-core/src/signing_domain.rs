//! Registry of every context the station identity key signs.
//!
//! Ed25519 signatures bind a key to a byte string, not to a purpose. Two contexts that sign
//! overlapping byte strings under one key let a signature made for one be presented as the other.
//! This module makes the separation structural: every signed message begins with a registered
//! four-byte domain tag, and the tags are pairwise distinct by test.
//!
//! Scope is **public-key signature domains only**. The ACK path's ECDH-derived MAC
//! (`session_key`/`ack`) and the linksec Noise channel authenticate by shared secret, not by
//! station identity, and are deliberately outside this registry — do not fold a MAC in here.

use core::fmt;

/// Width of a domain tag, and of every wire magic in the workspace.
pub const TAG_LEN: usize = 4;

/// A context the station identity key signs.
///
/// Two families, one invariant. **In-band** domains already transmit a unique magic as the first
/// bytes they sign, so the magic *is* the tag and nothing is prepended. **Prepended** domains sign
/// a serialization with no fixed leading bytes, so `tag || version` is prefixed to the signed
/// message only — it never reaches the wire and costs no airtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SigningDomain {
    /// Handshake CONREQ. In-band: `HSCQ`.
    ConReq,
    /// Handshake CONACK. In-band: `HSAK`.
    ConAck,
    /// Post-quantum CONREQ. In-band: `HPCQ`.
    PqConReq,
    /// Post-quantum CONACK. In-band: `HPAK`.
    PqConAck,
    /// Peer-query/relay envelope. In-band: `OPHF`.
    WireEnvelope,
    /// Transfer manifest over canonical JSON.
    Manifest,
    /// Self-authenticating peer descriptor over canonical JSON.
    PeerDescriptor,
    /// Route-discovery response over its packed canonical form.
    RouteResponse,
    /// Relay route update over its packed canonical form.
    RouteUpdate,
    /// File-transfer offer over its packed signed fields.
    FileOffer,
    /// Remote rig-control command over canonical JSON.
    RigCtrlCmd,
    /// QSY frequency-agility line over its ASCII form.
    QsyLine,
    /// FreeDV authenticated-voice beacon over canonical JSON.
    AuthBeacon,
}

/// How a domain's tag reaches the signed message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagPlacement {
    /// The tag is the frame's transmitted magic; the signed message already starts with it.
    InBand,
    /// The tag is prefixed to the signed message only and never transmitted.
    Prepended,
}

impl SigningDomain {
    /// Every domain. Exhaustively matched below, so a new variant fails the build until registered.
    pub const ALL: &'static [SigningDomain] = &[
        SigningDomain::ConReq,
        SigningDomain::ConAck,
        SigningDomain::PqConReq,
        SigningDomain::PqConAck,
        SigningDomain::WireEnvelope,
        SigningDomain::Manifest,
        SigningDomain::PeerDescriptor,
        SigningDomain::RouteResponse,
        SigningDomain::RouteUpdate,
        SigningDomain::FileOffer,
        SigningDomain::RigCtrlCmd,
        SigningDomain::QsyLine,
        SigningDomain::AuthBeacon,
    ];

    /// The four-byte tag that begins this domain's signed message.
    pub const fn tag(self) -> &'static [u8; TAG_LEN] {
        match self {
            SigningDomain::ConReq => b"HSCQ",
            SigningDomain::ConAck => b"HSAK",
            SigningDomain::PqConReq => b"HPCQ",
            SigningDomain::PqConAck => b"HPAK",
            SigningDomain::WireEnvelope => b"OPHF",
            SigningDomain::Manifest => b"OPMF",
            SigningDomain::PeerDescriptor => b"OPPD",
            SigningDomain::RouteResponse => b"OPRR",
            SigningDomain::RouteUpdate => b"OPRU",
            SigningDomain::FileOffer => b"OPFO",
            SigningDomain::RigCtrlCmd => b"OPRC",
            SigningDomain::QsyLine => b"OPQS",
            SigningDomain::AuthBeacon => b"OPAB",
        }
    }

    /// Where the tag comes from for this domain.
    pub const fn placement(self) -> TagPlacement {
        match self {
            SigningDomain::ConReq
            | SigningDomain::ConAck
            | SigningDomain::PqConReq
            | SigningDomain::PqConAck
            | SigningDomain::WireEnvelope => TagPlacement::InBand,
            SigningDomain::Manifest
            | SigningDomain::PeerDescriptor
            | SigningDomain::RouteResponse
            | SigningDomain::RouteUpdate
            | SigningDomain::FileOffer
            | SigningDomain::RigCtrlCmd
            | SigningDomain::QsyLine
            | SigningDomain::AuthBeacon => TagPlacement::Prepended,
        }
    }

    /// Version of this domain's signed-message layout.
    ///
    /// Prepended domains carry it in the prefix, so a later serialization change (JSON to binary,
    /// say) does not leave old and new messages sharing one undifferentiated domain. In-band
    /// domains carry their own version byte in the frame and this is that byte's current value.
    pub const fn version(self) -> u8 {
        match self {
            SigningDomain::ConReq
            | SigningDomain::ConAck
            | SigningDomain::PqConReq
            | SigningDomain::PqConAck => 0x02,
            SigningDomain::WireEnvelope => 0x02,
            SigningDomain::Manifest
            | SigningDomain::PeerDescriptor
            | SigningDomain::RouteResponse
            | SigningDomain::RouteUpdate
            | SigningDomain::FileOffer
            | SigningDomain::RigCtrlCmd
            | SigningDomain::QsyLine
            | SigningDomain::AuthBeacon => 0x01,
        }
    }
}

impl fmt::Display for SigningDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Naming the domain keeps a cross-version signature failure diagnosable: the tag never
        // reaches the wire, so without this an epoch mismatch reads as a bare "invalid signature".
        write!(f, "{}", String::from_utf8_lossy(self.tag()))
    }
}

/// Four-byte wire magics that are **not** signing domains.
///
/// Held here so the distinctness test runs over the union: a future context can neither adopt a
/// reserved magic as a signing tag nor mint a transmitted magic equal to an existing tag.
pub const RESERVED_MAGICS: &[(&[u8; TAG_LEN], &str)] = &[
    (b"OPLS", "openpulse-core frame envelope"),
    (b"OPSE", "openpulse-core SignedEnvelope container"),
    (b"OPSP", "openpulse-daemon control protocol"),
    (b"OPFX", "openpulse-filexfer wire"),
    (b"OPKS", "openpulse-keystore at-rest container"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The invariant the registry exists for, over the **union** of signing tags and reserved
    /// wire magics — so a future context can neither adopt a reserved magic as a signing tag nor
    /// mint a transmitted magic equal to an existing tag.
    #[test]
    fn every_tag_and_reserved_magic_is_pairwise_distinct() {
        // VERIFIES: REQ-SEC-12
        let mut seen: HashMap<&[u8; TAG_LEN], String> = HashMap::new();
        for d in SigningDomain::ALL {
            if let Some(prev) = seen.insert(d.tag(), format!("signing domain {d:?}")) {
                panic!("tag {} is claimed by both {prev} and {d:?}", d);
            }
        }
        for (magic, owner) in RESERVED_MAGICS {
            if let Some(prev) = seen.insert(magic, format!("reserved magic ({owner})")) {
                panic!(
                    "magic {} is claimed by both {prev} and {owner}",
                    String::from_utf8_lossy(*magic)
                );
            }
        }
        assert_eq!(seen.len(), SigningDomain::ALL.len() + RESERVED_MAGICS.len());
    }

    /// `ALL` drives the distinctness test, so a variant missing from it would be unchecked.
    ///
    /// The match below has no wildcard arm: adding a variant fails to compile here, and the count
    /// assertion then fails until `ALL` is updated too. Both are needed — the match alone cannot
    /// see that `ALL` is short, and the count alone cannot see *which* variant is missing.
    #[test]
    fn all_lists_every_variant() {
        for d in SigningDomain::ALL {
            match d {
                SigningDomain::ConReq
                | SigningDomain::ConAck
                | SigningDomain::PqConReq
                | SigningDomain::PqConAck
                | SigningDomain::WireEnvelope
                | SigningDomain::Manifest
                | SigningDomain::PeerDescriptor
                | SigningDomain::RouteResponse
                | SigningDomain::RouteUpdate
                | SigningDomain::FileOffer
                | SigningDomain::RigCtrlCmd
                | SigningDomain::QsyLine
                | SigningDomain::AuthBeacon => {}
            }
        }
        assert_eq!(
            SigningDomain::ALL.len(),
            13,
            "a SigningDomain variant was added without extending ALL"
        );
    }

    /// The in-band tags are not free choices — they must equal the magic the frame already
    /// transmits, or the signed message would not begin with them.
    #[test]
    fn in_band_tags_match_the_transmitted_magics() {
        assert_eq!(
            SigningDomain::ConReq.tag(),
            crate::handshake_wire::MAGIC_CONREQ
        );
        assert_eq!(
            SigningDomain::ConAck.tag(),
            crate::handshake_wire::MAGIC_CONACK
        );
        assert_eq!(
            SigningDomain::PqConReq.tag(),
            crate::handshake_wire::MAGIC_PQ_CONREQ
        );
        assert_eq!(
            SigningDomain::PqConAck.tag(),
            crate::handshake_wire::MAGIC_PQ_CONACK
        );
    }

    #[test]
    fn tags_are_printable_ascii_so_a_capture_is_readable() {
        for d in SigningDomain::ALL {
            assert!(
                d.tag().iter().all(|b| b.is_ascii_graphic()),
                "{d:?} tag is not printable ASCII"
            );
        }
    }
}
