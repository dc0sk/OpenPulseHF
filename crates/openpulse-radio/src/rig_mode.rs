use crate::RadioError;

/// Transceiver operating mode as reported/set via rigctld.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigMode {
    Usb,
    Lsb,
    Fm,
    Am,
    Cw,
    CwR,
    Rtty,
    RttyR,
    PktUsb,
    PktLsb,
    /// Any mode string not matched by the known variants.
    Other(String),
}

impl RigMode {
    /// Return the rigctld mode string (e.g. `"USB"`, `"FM"`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Usb => "USB",
            Self::Lsb => "LSB",
            Self::Fm => "FM",
            Self::Am => "AM",
            Self::Cw => "CW",
            Self::CwR => "CWR",
            Self::Rtty => "RTTY",
            Self::RttyR => "RTTYR",
            Self::PktUsb => "PKTUSB",
            Self::PktLsb => "PKTLSB",
            Self::Other(s) => s.as_str(),
        }
    }

    pub(crate) fn from_str(s: &str) -> Result<Self, RadioError> {
        Ok(match s {
            "USB" => Self::Usb,
            "LSB" => Self::Lsb,
            "FM" => Self::Fm,
            "AM" => Self::Am,
            "CW" => Self::Cw,
            "CWR" => Self::CwR,
            "RTTY" => Self::Rtty,
            "RTTYR" => Self::RttyR,
            "PKTUSB" => Self::PktUsb,
            "PKTLSB" => Self::PktLsb,
            other => Self::Other(other.to_string()),
        })
    }
}
