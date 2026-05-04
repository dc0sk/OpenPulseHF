//! AX.25 UI frame encode/decode (APRS subset).
//!
//! Only UI frames are supported: Control=0x03, PID=0xF0, no repeater digipeating.

#[derive(Debug, Clone, PartialEq)]
pub struct Ax25Addr {
    /// 6 ASCII bytes, right-padded with spaces.
    pub callsign: [u8; 6],
    /// SSID 0–15.
    pub ssid: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ax25UiFrame {
    pub dest: Ax25Addr,
    pub src: Ax25Addr,
    pub info: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum Ax25Error {
    #[error("frame too short")]
    TooShort,
    #[error("invalid control or PID byte")]
    BadFrame,
    #[error("invalid callsign")]
    InvalidCallsign,
}

impl Ax25Addr {
    /// Parse a callsign string such as `"APRS"` or `"W1AW-9"`.
    pub fn parse(s: &str) -> Result<Self, Ax25Error> {
        let (call_str, ssid) = if let Some(pos) = s.find('-') {
            let ssid: u8 = s[pos + 1..]
                .parse()
                .map_err(|_| Ax25Error::InvalidCallsign)?;
            (&s[..pos], ssid)
        } else {
            (s, 0u8)
        };
        if call_str.len() > 6 || ssid > 15 {
            return Err(Ax25Error::InvalidCallsign);
        }
        let mut callsign = [b' '; 6];
        for (i, c) in call_str.bytes().enumerate() {
            callsign[i] = c.to_ascii_uppercase();
        }
        Ok(Self { callsign, ssid })
    }

    /// Encode to 7 wire bytes.  `last` sets the end-of-address bit.
    fn to_wire(&self, last: bool) -> [u8; 7] {
        let mut out = [0u8; 7];
        for (i, &b) in self.callsign.iter().enumerate() {
            out[i] = b << 1;
        }
        // Bits 7-6 reserved (set to 1), bits 4-1 SSID, bit 0 end-of-address.
        out[6] = 0x60 | ((self.ssid & 0x0F) << 1) | u8::from(last);
        out
    }

    fn from_wire(bytes: &[u8]) -> Self {
        let mut callsign = [0u8; 6];
        for (i, &b) in bytes[..6].iter().enumerate() {
            callsign[i] = b >> 1;
        }
        let ssid = (bytes[6] >> 1) & 0x0F;
        Self { callsign, ssid }
    }

    /// Return the callsign as a trimmed ASCII string.
    pub fn callsign_str(&self) -> String {
        std::str::from_utf8(&self.callsign)
            .unwrap_or("")
            .trim_end()
            .to_string()
    }
}

impl Ax25UiFrame {
    /// Encode to wire bytes: dest(7) + src(7) + Control(0x03) + PID(0xF0) + info.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.info.len());
        out.extend_from_slice(&self.dest.to_wire(false));
        out.extend_from_slice(&self.src.to_wire(true));
        out.push(0x03); // Control: UI
        out.push(0xF0); // PID: no layer 3
        out.extend_from_slice(&self.info);
        out
    }

    /// Decode from wire bytes.
    pub fn decode(data: &[u8]) -> Result<Self, Ax25Error> {
        if data.len() < 16 {
            return Err(Ax25Error::TooShort);
        }
        let dest = Ax25Addr::from_wire(&data[0..7]);
        let src = Ax25Addr::from_wire(&data[7..14]);
        if data[14] != 0x03 {
            return Err(Ax25Error::BadFrame);
        }
        if data[15] != 0xF0 {
            return Err(Ax25Error::BadFrame);
        }
        Ok(Self {
            dest,
            src,
            info: data[16..].to_vec(),
        })
    }
}
