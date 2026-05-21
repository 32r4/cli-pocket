//! Wire protocol contracts. Real types land in Plan B.

/// Placeholder version used until Plan B lands the real protocol.
pub const SCAFFOLD_VERSION: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_version_is_zero() {
        assert_eq!(SCAFFOLD_VERSION, 0);
    }
}
