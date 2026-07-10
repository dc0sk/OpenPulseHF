//! Filename sanitization for received offers — never trust the sender's `name` field on disk.

/// Reduce an offer's suggested filename to a safe **basename**: no path separators, no `..`, no
/// control/reserved characters, non-empty, length-bounded. Used before any write under the download
/// directory to prevent path traversal.
pub fn sanitize_filename(name: &str) -> String {
    // Keep only the last path component the sender might have embedded (defeats `../` and `/etc/...`).
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('.'); // no leading/trailing dots → kills "." / ".." / hidden-by-accident

    let mut cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();

    // Bound the length on a char boundary (filesystem limits; leave room for a ".partial" suffix).
    const MAX_LEN: usize = 96;
    if cleaned.len() > MAX_LEN {
        let mut end = MAX_LEN;
        while end > 0 && !cleaned.is_char_boundary(end) {
            end -= 1;
        }
        cleaned.truncate(end);
    }

    if cleaned.is_empty() {
        "received.bin".to_string()
    } else {
        cleaned
    }
}
