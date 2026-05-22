//! Relay server facade. Real implementation lands in Task E7.

#[derive(Debug, Clone)]
pub struct RelayServer {
    config: crate::RelayConfig,
}

impl RelayServer {
    #[must_use]
    pub fn new(config: crate::RelayConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn config(&self) -> &crate::RelayConfig {
        &self.config
    }
}
