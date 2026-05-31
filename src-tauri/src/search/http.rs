use std::time::Duration;

/// The single place anything in the app touches the network for search.
/// Short timeouts so a dead source never hangs the grid. Returns the body
/// string on 2xx, or an Err message on any failure (caller degrades to no rows).
pub(crate) fn get(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(4))
        .timeout_read(Duration::from_secs(6))
        .user_agent("napm")
        .build();
    match agent.get(url).call() {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Percent-encode a query value (encode everything except RFC 3986 unreserved).
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encode_escapes_spaces_and_slashes() {
        assert_eq!(encode("fuzzy finder"), "fuzzy%20finder");
        assert_eq!(encode("@scope/pkg"), "%40scope%2Fpkg");
        assert_eq!(encode("ripgrep"), "ripgrep");
    }
}
