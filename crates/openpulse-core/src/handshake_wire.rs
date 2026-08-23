//! Byte-level codec for the v2 binary handshake frames (#1147).
//!
//! **What gets signed is the transmitted prefix** — `magic || version || length || body` — so the
//! signed message is exactly the bytes on the air minus the trailing signature. There is no second
//! representation, so verification is "hash what you received" and cannot drift from the encoder.
//! v1 signed `serde_json::to_vec` of the body in serde declaration order with no key sorting, while
//! the wire spec claimed keys were "sorted recursively": canonical was a label, not a property.
//!
//! No domain-separation tags. The obvious ones (`OPHF-CONREQ-v2`) begin with `OPHF`, the magic of
//! `WireEnvelope`, which the SAME station key already signs — separation by whichever byte differs
//! first, which is the property tags exist to remove. Signing the transmitted prefix separates by
//! the magic itself (already unique per frame type) and binds the VERSION, which v1 did not do.

use crate::error::ModemError;

/// Frame magics. Distinct per type, and part of the signed prefix.
pub const MAGIC_CONREQ: &[u8; 4] = b"HSCQ";
/// See [`MAGIC_CONREQ`].
pub const MAGIC_CONACK: &[u8; 4] = b"HSAK";
/// Wire version.
///
/// DORMANT(#1147): part of the format's public contract — the wire spec and the known-answer vector
/// cite it — but no production caller names it, because `split_frame` enforces it internally.
/// `0x01` (the JSON format) is rejected outright — there is no dual decode, because
/// there is no compatibility mode for the data plane and inventing one here would be the
/// "legacy keystream" mistake in a different file.
pub const WIRE_VERSION: u8 = 0x02;

/// `magic(4) + version(1) + length(2)`.
pub const HEADER_LEN: usize = 7;
/// Ed25519 signature, trailing and unsigned.
///
/// DORMANT(#1147): as [`WIRE_VERSION`] — contract surface, enforced internally.
pub const SIG_LEN: usize = 64;
/// One SAR fragment's data capacity — 255 B frame payload less the 4 B SAR header.
///
/// DORMANT(#1147): the budget every gate asserts against, and the number the whole change exists to
/// stay under. Consumed by tests rather than by production, because nothing in the encode path
/// needs to ask: the caps make the bound structural.
pub const FRAGMENT_CAPACITY: usize = 251;

/// Decoder-enforced caps. These are what make "one fragment" a property of the MAXIMAL LEGAL frame
/// rather than of one example: `u8`-prefixed strings would otherwise admit a legal 300 B CONREQ.
pub mod caps {
    /// `station_id` and `dst_station`. `DC0SK/P` is 7.
    pub const STATION_ID: usize = 12;
    /// `session_id`. `"{callsign}-{now_ms}"` is ~20.
    pub const SESSION_ID: usize = 24;
    /// `station_grid`. Six-character Maidenhead is the longest in use.
    pub const GRID: usize = 8;
    /// `profile_name`. `hpx_pilot_fast_rrc` is 18.
    pub const PROFILE_NAME: usize = 24;
    /// `signing_modes` entries.
    pub const SIGNING_MODES: usize = 4;
}

/// Fixed-size fields. A length prefix on a constant-size field hands a parser decision to the
/// sender, so these are positional and a wrong length is a decode error, not a late verify failure.
pub const PUBKEY_LEN: usize = 32;
/// See [`PUBKEY_LEN`].
pub const KEX_PUBKEY_LEN: usize = 32;
/// SHA-256 over the complete transmitted CONREQ frame, including its signature.
pub const CONREQ_HASH_LEN: usize = 32;

/// Append-only writer for the body region.
#[derive(Default)]
pub struct BodyWriter {
    buf: Vec<u8>,
}

impl BodyWriter {
    /// A writer over an empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Write a `u8`-length-prefixed string, refusing anything over `cap`.
    ///
    /// The cap is enforced on the ENCODE side too, so a station cannot emit a frame its peer is
    /// required to reject — the failure surfaces at the sender, where it is actionable.
    pub fn str_capped(&mut self, field: &str, value: &str, cap: usize) -> Result<(), ModemError> {
        let bytes = value.as_bytes();
        if bytes.len() > cap {
            return Err(ModemError::Frame(format!(
                "handshake field `{field}` is {} bytes, over its {cap}-byte cap",
                bytes.len()
            )));
        }
        self.buf.push(bytes.len() as u8);
        self.buf.extend_from_slice(bytes);
        Ok(())
    }

    /// Write a fixed-size field, refusing a wrong length at the sender.
    pub fn fixed(&mut self, field: &str, value: &[u8], len: usize) -> Result<(), ModemError> {
        if value.len() != len {
            return Err(ModemError::Frame(format!(
                "handshake field `{field}` is {} bytes, must be exactly {len}",
                value.len()
            )));
        }
        self.buf.extend_from_slice(value);
        Ok(())
    }

    /// Write one byte.
    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Write a big-endian `u64`.
    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_be_bytes());
    }

    /// The body bytes.
    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Reader over the body region; every accessor is bounds-checked.
pub struct BodyReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> BodyReader<'a> {
    /// A reader over `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize, field: &str) -> Result<&'a [u8], ModemError> {
        let end = self.pos.checked_add(n).ok_or_else(|| {
            ModemError::Frame(format!("handshake field `{field}` length overflows"))
        })?;
        if end > self.buf.len() {
            return Err(ModemError::Frame(format!(
                "handshake truncated reading `{field}`: need {n} bytes, {} remain",
                self.buf.len().saturating_sub(self.pos)
            )));
        }
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    /// Read a `u8`-length-prefixed string, refusing anything over `cap`.
    pub fn str_capped(&mut self, field: &str, cap: usize) -> Result<String, ModemError> {
        let len = self.take(1, field)?[0] as usize;
        if len > cap {
            return Err(ModemError::Frame(format!(
                "handshake field `{field}` declares {len} bytes, over its {cap}-byte cap"
            )));
        }
        let bytes = self.take(len, field)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| ModemError::Frame(format!("handshake field `{field}` is not UTF-8")))
    }

    /// Read a fixed-size field.
    pub fn fixed(&mut self, field: &str, len: usize) -> Result<Vec<u8>, ModemError> {
        Ok(self.take(len, field)?.to_vec())
    }

    /// Read one byte.
    pub fn u8(&mut self, field: &str) -> Result<u8, ModemError> {
        Ok(self.take(1, field)?[0])
    }

    /// Read a big-endian `u64`.
    pub fn u64(&mut self, field: &str) -> Result<u64, ModemError> {
        let b = self.take(8, field)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }

    /// Refuse trailing bytes — a body longer than its fields is a different frame, not this one.
    pub fn finish(self, what: &str) -> Result<(), ModemError> {
        if self.pos != self.buf.len() {
            return Err(ModemError::Frame(format!(
                "{what} body has {} trailing bytes",
                self.buf.len() - self.pos
            )));
        }
        Ok(())
    }
}

/// Assemble `magic || version || length || body`, the exact bytes that get signed.
pub fn signed_prefix(magic: &[u8; 4], body: &[u8]) -> Result<Vec<u8>, ModemError> {
    let len = u16::try_from(body.len())
        .map_err(|_| ModemError::Frame("handshake body exceeds u16 length".into()))?;
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(magic);
    out.push(WIRE_VERSION);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(body);
    Ok(out)
}

/// The three spans of a received frame.
///
/// DORMANT(#1147): production consumes its FIELDS (`split_frame(..)?.signed_prefix`), never the type
/// by name, so the reachability scan does not see it referenced.
///: the signed prefix, the body inside it, and the signature.
///
/// Named rather than returned as a bare tuple so a caller cannot silently transpose two `&[u8]`
/// of identical type — the prefix and the body differ only by seven leading bytes, and verifying
/// the wrong one would still "work" on well-formed input and fail only on the attack it exists to
/// stop.
#[derive(Debug)]
pub struct FrameSpans<'a> {
    /// `magic || version || length || body` — exactly what the signature covers.
    pub signed_prefix: &'a [u8],
    /// The body region alone.
    pub body: &'a [u8],
    /// The trailing, unsigned Ed25519 signature.
    pub signature: &'a [u8],
}

/// Split a received frame into its signed prefix, body and signature.
///
/// Validates magic, version and declared length before anything downstream looks at the body. A v1
/// (`0x01`) frame is rejected here by version, which is why no caller needs a dual-decode path.
pub fn split_frame<'a>(
    bytes: &'a [u8],
    magic: &[u8; 4],
    what: &str,
) -> Result<FrameSpans<'a>, ModemError> {
    if bytes.len() < HEADER_LEN + SIG_LEN {
        return Err(ModemError::Frame(format!(
            "{what} too short: {} bytes, minimum {}",
            bytes.len(),
            HEADER_LEN + SIG_LEN
        )));
    }
    if &bytes[..4] != magic {
        return Err(ModemError::Frame(format!("invalid {what} magic")));
    }
    let version = bytes[4];
    if version != WIRE_VERSION {
        return Err(ModemError::Frame(format!(
            "{what} wire version {version:#04x} is not supported (this build speaks \
             {WIRE_VERSION:#04x} only; there is no dual decode)"
        )));
    }
    let declared = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;
    let expected_total = HEADER_LEN + declared + SIG_LEN;
    if bytes.len() != expected_total {
        return Err(ModemError::Frame(format!(
            "{what} declares a {declared}-byte body, so the frame must be {expected_total} bytes, \
             but it is {}",
            bytes.len()
        )));
    }
    Ok(FrameSpans {
        signed_prefix: &bytes[..HEADER_LEN + declared],
        body: &bytes[HEADER_LEN..HEADER_LEN + declared],
        signature: &bytes[HEADER_LEN + declared..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CONREQ body at every cap: the layout the 251 B budget is actually spent on.
    fn maximal_conreq_body() -> Vec<u8> {
        let mut w = BodyWriter::new();
        w.str_capped(
            "station_id",
            &"A".repeat(caps::STATION_ID),
            caps::STATION_ID,
        )
        .unwrap();
        w.str_capped(
            "dst_station",
            &"B".repeat(caps::STATION_ID),
            caps::STATION_ID,
        )
        .unwrap();
        w.fixed("pubkey", &[7u8; PUBKEY_LEN], PUBKEY_LEN).unwrap();
        w.fixed("kex_pubkey", &[9u8; KEX_PUBKEY_LEN], KEX_PUBKEY_LEN)
            .unwrap();
        w.u8(caps::SIGNING_MODES as u8);
        for _ in 0..caps::SIGNING_MODES {
            w.u8(1);
        }
        w.str_capped(
            "session_id",
            &"C".repeat(caps::SESSION_ID),
            caps::SESSION_ID,
        )
        .unwrap();
        w.str_capped("station_grid", &"D".repeat(caps::GRID), caps::GRID)
            .unwrap();
        w.str_capped(
            "profile_name",
            &"E".repeat(caps::PROFILE_NAME),
            caps::PROFILE_NAME,
        )
        .unwrap();
        w.u64(u64::MAX);
        w.u64(u64::MAX);
        w.finish()
    }

    /// THE BUDGET GATE: the MAXIMAL legal CONREQ fits one SAR fragment.
    ///
    /// Asserted on the worst case, not on an example frame — the whole point of the decoder caps is
    /// that "it fits" is a property of every legal frame. A CONREQ that spans fragments decodes with
    /// probability p^3 on a fading channel instead of p, which is the defect #1147 exists to fix.
    #[test]
    fn the_maximal_legal_conreq_fits_one_sar_fragment() {
        let frame = HEADER_LEN + maximal_conreq_body().len() + SIG_LEN;
        assert_eq!(
            frame, 241,
            "the maximal CONREQ is {frame} B, not the 241 B the design budgeted — the layout \
             changed and the fragment arithmetic must be redone, not the assertion adjusted"
        );
        assert!(
            frame <= FRAGMENT_CAPACITY,
            "the maximal legal CONREQ is {frame} B, over the {FRAGMENT_CAPACITY} B one fragment \
             holds, so a legal frame would need three acquisitions instead of one"
        );
    }

    /// The CONACK budget, which is what forced `session_id` OUT of the frame: echoing it costs 25 B
    /// and pushes the worst case to 269 B. `conreq_hash` subsumes the echo and binds harder.
    #[test]
    fn the_maximal_legal_conack_fits_one_sar_fragment() {
        let mut w = BodyWriter::new();
        w.str_capped(
            "station_id",
            &"A".repeat(caps::STATION_ID),
            caps::STATION_ID,
        )
        .unwrap();
        w.str_capped(
            "dst_station",
            &"B".repeat(caps::STATION_ID),
            caps::STATION_ID,
        )
        .unwrap();
        w.fixed("pubkey", &[7u8; PUBKEY_LEN], PUBKEY_LEN).unwrap();
        w.fixed("kex_pubkey", &[9u8; KEX_PUBKEY_LEN], KEX_PUBKEY_LEN)
            .unwrap();
        w.u8(1);
        w.fixed("conreq_hash", &[3u8; CONREQ_HASH_LEN], CONREQ_HASH_LEN)
            .unwrap();
        w.str_capped("station_grid", &"D".repeat(caps::GRID), caps::GRID)
            .unwrap();
        w.str_capped(
            "profile_name",
            &"E".repeat(caps::PROFILE_NAME),
            caps::PROFILE_NAME,
        )
        .unwrap();
        w.u64(u64::MAX);
        w.u64(u64::MAX);
        let frame = HEADER_LEN + w.finish().len() + SIG_LEN;
        assert_eq!(
            frame, 244,
            "the maximal CONACK is {frame} B, not the budgeted 244"
        );
        assert!(frame <= FRAGMENT_CAPACITY);
    }

    /// A v1 frame is refused by VERSION, which is why nothing downstream needs a dual-decode path.
    #[test]
    fn a_v1_frame_is_rejected_by_version_not_by_parsing() {
        let mut f = signed_prefix(MAGIC_CONREQ, &[0u8; 8]).unwrap();
        f.extend_from_slice(&[0u8; SIG_LEN]);
        f[4] = 0x01;
        let e = split_frame(&f, MAGIC_CONREQ, "CONREQ")
            .unwrap_err()
            .to_string();
        assert!(e.contains("0x01"), "expected a version refusal, got: {e}");
    }

    /// A frame cannot verify as another TYPE: the magic is inside the signed prefix.
    #[test]
    fn a_conack_frame_is_not_accepted_as_a_conreq() {
        let mut f = signed_prefix(MAGIC_CONACK, &[0u8; 8]).unwrap();
        f.extend_from_slice(&[0u8; SIG_LEN]);
        assert!(split_frame(&f, MAGIC_CONREQ, "CONREQ").is_err());
    }

    /// A declared length that disagrees with the frame is refused before the body is read, so a
    /// truncated or padded frame cannot be reinterpreted as a shorter well-formed one.
    #[test]
    fn a_length_that_disagrees_with_the_frame_is_rejected() {
        let mut f = signed_prefix(MAGIC_CONREQ, &[1u8; 16]).unwrap();
        f.extend_from_slice(&[0u8; SIG_LEN]);
        assert!(split_frame(&f, MAGIC_CONREQ, "CONREQ").is_ok());
        f[6] = 15;
        assert!(split_frame(&f, MAGIC_CONREQ, "CONREQ").is_err());
    }

    /// Caps are enforced on BOTH sides: a sender cannot emit a frame its peer must reject.
    #[test]
    fn an_over_cap_field_is_refused_at_the_sender_and_at_the_decoder() {
        let mut w = BodyWriter::new();
        assert!(w
            .str_capped(
                "station_id",
                &"A".repeat(caps::STATION_ID + 1),
                caps::STATION_ID
            )
            .is_err());

        let mut body = vec![(caps::STATION_ID + 1) as u8];
        body.extend(std::iter::repeat_n(b'A', caps::STATION_ID + 1));
        let mut r = BodyReader::new(&body);
        assert!(r.str_capped("station_id", caps::STATION_ID).is_err());
    }

    /// Trailing bytes are refused — a longer body is a different frame, not this one.
    #[test]
    fn trailing_body_bytes_are_rejected() {
        let body = [0u8; 9];
        let mut r = BodyReader::new(&body);
        assert_eq!(r.u64("timestamp_ms").unwrap(), 0);
        assert!(r.finish("CONREQ").is_err());
    }
}
