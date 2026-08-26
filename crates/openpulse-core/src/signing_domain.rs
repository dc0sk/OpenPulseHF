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
const TAG_LEN: usize = 4;

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
            // DERIVED, not written: these four carry their version in the frame, so the value is
            // whatever the wire format says it is. It was hard-coded as 0x02 and went stale the
            // moment #1191 reset WIRE_VERSION — caught by `in_band_versions_match_the_wire_version`,
            // which exists for exactly that. Sharing the constant makes the test unfailable by
            // construction, which is better than a test that catches the drift after the fact.
            SigningDomain::ConReq
            | SigningDomain::ConAck
            | SigningDomain::PqConReq
            | SigningDomain::PqConAck => crate::handshake_wire::WIRE_VERSION,
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
/// Held here so the distinctness test runs over the union, which stops a signing tag adopting a
/// magic that is already in use.
///
/// **What this does NOT guarantee.** It cannot stop a *future* magic being minted equal to an
/// existing tag, because this is a hand-maintained list and a magic that never registers here is
/// invisible to it. That is the exact rot this module's own history documents — the list was wrong
/// on its first commit (`OPZ1` was missing). `every_magic_in_the_tree_is_registered` closes that
/// by scanning the source, so the list is checked against reality rather than trusted.
///
/// Test-scoped because those tests are its only consumer — the constraint is static, so there is
/// nothing for production to check at runtime. Keeping it `pub` to satisfy the linter would be
/// public API with no caller, which the reachability ratchet exists to refuse.
#[cfg(test)]
const RESERVED_MAGICS: &[(&[u8; TAG_LEN], &str)] = &[
    (b"OPLS", "openpulse-core frame envelope"),
    (b"OPSE", "openpulse-core SignedEnvelope container"),
    (b"OPSP", "openpulse-daemon control protocol"),
    (b"OPFX", "openpulse-filexfer wire"),
    (b"OPKS", "openpulse-keystore at-rest container"),
    (b"OPZ1", "openpulse-core compression container"),
];

/// Magics owned by external file formats, deliberately excluded from [`RESERVED_MAGICS`].
///
/// They are not OpenPulse wire magics and cannot collide with a signing tag in any context the
/// station key signs; they are listed so the source scan below can tell "excluded on purpose"
/// from "not yet registered".
#[cfg(test)]
const FOREIGN_MAGICS: &[&[u8; TAG_LEN]] = &[b"RIFF", b"WAVE"];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The invariant the registry exists for, over the **union** of signing tags and reserved
    /// wire magics — so a future context can neither adopt a reserved magic as a signing tag nor
    /// mint a transmitted magic equal to an existing tag.
    #[test]
    fn every_tag_and_reserved_magic_is_pairwise_distinct() {
        // VERIFIES: REQ-SEC-13
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

    /// The in-band `version()` values are not free choices — they must equal the version byte the
    /// frame actually transmits, or the doc claim that they track the frame is false. Without this
    /// they rot silently at the next wire-version bump.
    #[test]
    fn in_band_versions_match_the_wire_version() {
        for d in [
            SigningDomain::ConReq,
            SigningDomain::ConAck,
            SigningDomain::PqConReq,
            SigningDomain::PqConAck,
        ] {
            assert_eq!(
                d.version(),
                crate::handshake_wire::WIRE_VERSION,
                "{d:?} version drifted from the handshake wire version"
            );
        }
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

#[cfg(test)]
mod source_scan {
    use super::*;

    /// Every four-byte magic literal in the workspace is registered — as a signing tag, a reserved
    /// magic, or an explicitly-foreign one.
    ///
    /// This exists because the hand list is not trustworthy and its own history proves it: the
    /// inventory was wrong four times over (the issue that prompted it listed 7 contexts of 13, a
    /// careful re-derivation missed `OPSE`, review's reserved list missed `OPSP`, and the list as
    /// first committed missed `OPZ1`). A list nobody checks against the source rots; this checks it.
    ///
    /// **Limit, stated so it is not over-trusted:** it matches four-byte byte-string literals
    /// only. A magic
    /// assembled arithmetically, built from a `const` expression, or written as a byte array
    /// evades it. It narrows the gap; it does not close it.
    #[test]
    fn every_magic_in_the_tree_is_registered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");

        let mut found: Vec<(String, String)> = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                let name = e.file_name();
                let name = name.to_string_lossy();
                if path.is_dir() {
                    if !matches!(name.as_ref(), "target" | ".git" | "docs" | "node_modules") {
                        stack.push(path);
                    }
                } else if path.extension().is_some_and(|x| x == "rs") {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let bytes = text.as_bytes();
                    for i in 0..bytes.len().saturating_sub(7) {
                        // Skip doc/line comments: prose naming a magic is not a declaration.
                        // Indexed on BYTES, not chars — the docs contain multi-byte punctuation and
                        // slicing the &str by byte offset panics on a char boundary.
                        let line_start = bytes[..i]
                            .iter()
                            .rposition(|&c| c == b'\n')
                            .map_or(0, |n| n + 1);
                        let head: Vec<u8> = bytes[line_start..i]
                            .iter()
                            .copied()
                            .skip_while(|c| c.is_ascii_whitespace())
                            .take(2)
                            .collect();
                        if head == b"//" {
                            continue;
                        }
                        if &bytes[i..i + 2] == b"b\""
                            && bytes[i + 6] == b'"'
                            && bytes[i + 2..i + 6]
                                .iter()
                                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                        {
                            let magic = String::from_utf8_lossy(&bytes[i + 2..i + 6]).to_string();
                            found.push((magic, path.display().to_string()));
                        }
                    }
                }
            }
        }

        // The scan must actually find things; an empty sweep would pass vacuously.
        assert!(
            found.len() >= 10,
            "the magic scan found only {} literals — the scan itself is broken, not the tree",
            found.len()
        );

        let registered: Vec<String> = SigningDomain::ALL
            .iter()
            .map(|d| String::from_utf8_lossy(d.tag()).to_string())
            .chain(
                RESERVED_MAGICS
                    .iter()
                    .map(|(m, _)| String::from_utf8_lossy(*m).to_string()),
            )
            .chain(
                FOREIGN_MAGICS
                    .iter()
                    .map(|m| String::from_utf8_lossy(*m).to_string()),
            )
            .collect();

        let mut unregistered: Vec<(String, String)> = found
            .into_iter()
            .filter(|(m, _)| !registered.contains(m))
            .collect();
        unregistered.sort();
        unregistered.dedup_by(|a, b| a.0 == b.0);

        assert!(
            unregistered.is_empty(),
            "four-byte magic literals in the tree are registered nowhere — add each to \
             RESERVED_MAGICS (an OpenPulse wire magic), to SigningDomain (a signed context), or to \
             FOREIGN_MAGICS (an external format): {unregistered:?}"
        );
    }
}
