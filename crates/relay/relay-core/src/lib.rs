//! Relay forwarding logic. Real types land in Plan E.

#[must_use]
pub fn version_banner() -> &'static str {
    "cli-pocket-relay (scaffold)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_is_relay() {
        assert!(version_banner().contains("relay"));
    }
}
