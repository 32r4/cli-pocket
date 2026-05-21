//! Daemon orchestration. Real types land in Plan D.

#[must_use]
pub fn version_banner() -> String {
    format!(
        "cli-pocket-daemon (scaffold proto v{})",
        cli_pocket_proto::SCAFFOLD_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_mentions_proto_version() {
        assert!(version_banner().contains("proto v0"));
    }
}
