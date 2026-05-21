//! Client state machine. Real types land in Plan F.
//!
//! Must remain wasm-friendly: no tokio multi-thread, no std::net direct,
//! no direct std::time::Instant outside trait impls. Plan F enforces this.

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_compiles() {}
}
