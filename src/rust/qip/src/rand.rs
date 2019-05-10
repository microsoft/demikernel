use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;

pub type Rng = ChaChaRng;
pub type Seed = <ChaChaRng as SeedableRng>::Seed;
