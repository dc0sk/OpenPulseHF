//! WL2K message header encode/decode.

use crate::B2fError;

/// Metadata for a single file attachment.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentInfo {
    pub name: String,
    pub size: u32,
}

/// Winlink message header.
#[derive(Debug, Clone, PartialEq)]
pub struct WlHeader {
    pub mid: String,
    pub date: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    /// Total message size in bytes (header + body + attachments).
    pub size: u32,
    /// Body size in bytes.
    pub body: u32,
    pub attachments: Vec<AttachmentInfo>,
}

/// Encode a header block to bytes (CRLF-terminated lines).
pub fn encode(h: &WlHeader) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(&format!("Mid: {}\r\n", h.mid));
    out.push_str(&format!("Date: {}\r\n", h.date));
    out.push_str(&format!("From: {}\r\n", h.from));
    for addr in &h.to {
        out.push_str(&format!("To: {addr}\r\n"));
    }
    out.push_str(&format!("Subject: {}\r\n", h.subject));
    out.push_str("Mbo: OPNPLS\r\n");
    out.push_str(&format!("Body: {}\r\n", h.body));
    out.push_str(&format!("File: {}\r\n", h.size));
    for att in &h.attachments {
        out.push_str(&format!("File: {} {}\r\n", att.size, att.name));
    }
    out.push_str("\r\n"); // blank line ends header
    out.into_bytes()
}

/// Decode a header block from bytes.
pub fn decode(data: &[u8]) -> Result<WlHeader, B2fError> {
    let text = std::str::from_utf8(data).map_err(|_| B2fError::InvalidHeader("non-UTF8".into()))?;
    let mut mid = String::new();
    let mut date = String::new();
    let mut from = String::new();
    let mut to = Vec::new();
    let mut subject = String::new();
    let mut size = 0u32;
    let mut body = 0u32;
    let mut attachments = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            break; // blank line marks end of header block
        }
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let val = val.trim();
            match key.as_str() {
                "mid" => mid = val.to_string(),
                "date" => date = val.to_string(),
                "from" => from = val.to_string(),
                "to" => to.push(val.to_string()),
                "subject" => subject = val.to_string(),
                "body" => {
                    body = val
                        .parse()
                        .map_err(|_| B2fError::InvalidHeader("bad body".into()))?;
                }
                "file" => {
                    let parts: Vec<&str> = val.splitn(2, ' ').collect();
                    let s: u32 = parts[0]
                        .parse()
                        .map_err(|_| B2fError::InvalidHeader("bad file size".into()))?;
                    if parts.len() == 2 {
                        attachments.push(AttachmentInfo {
                            name: parts[1].to_string(),
                            size: s,
                        });
                    } else {
                        size = s;
                    }
                }
                _ => {}
            }
        }
    }
    require("Mid", &mid)?;
    require("From", &from)?;
    if to.is_empty() {
        return Err(crate::B2fError::InvalidHeader("missing To".into()));
    }
    Ok(WlHeader {
        mid,
        date,
        from,
        to,
        subject,
        size,
        body,
        attachments,
    })
}

fn require(field: &str, val: &str) -> Result<(), crate::B2fError> {
    if val.is_empty() {
        Err(crate::B2fError::InvalidHeader(format!("missing {field}")))
    } else {
        Ok(())
    }
}
