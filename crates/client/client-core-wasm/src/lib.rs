//! wasm-bindgen surface for cli-pocket client.
//!
//! Build: `wasm-pack build crates/client/client-core-wasm --target web`

mod clock_perf;
mod kv_idb;
mod rng_crypto;
mod ws_transport;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn _start() {
    console_error_panic_hook::set_once();
    let _ = tracing_wasm::try_set_as_global_default();
}

#[wasm_bindgen]
pub struct CliPocketClient {
    // Holds session command + event handles. Real fields wired in Task F12.
}

#[wasm_bindgen]
impl CliPocketClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<CliPocketClient, JsValue> {
        Ok(Self {})
    }
}
