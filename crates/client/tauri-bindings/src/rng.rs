use cli_pocket_client_core::Rng;

#[derive(Clone, Copy, Debug, Default)]
pub struct OsRandom;

impl Rng for OsRandom {
    fn fill(&self, dest: &mut [u8]) {
        if let Err(error) = getrandom::getrandom(dest) {
            panic!("OS random source failed: {error}");
        }
    }
}
