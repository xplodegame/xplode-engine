#![allow(dead_code)]
use std::collections::HashSet;

use rand::{rngs::StdRng, RngCore, SeedableRng};
use sha3::{Digest, Sha3_256};

struct DistributedSeedGen {
    pub seed_hash: [u8; 32],
}

impl DistributedSeedGen {
    fn new(genesis_contrib: u64) -> Self {
        let mut hasher = sha3::Sha3_256::new();

        hasher.update(genesis_contrib.to_be_bytes());

        let seed_hash: [u8; 32] = hasher.finalize().into();

        DistributedSeedGen { seed_hash }
    }

    fn update_seed_hash(&mut self, new_contrib: u64) {
        let mut hasher = Sha3_256::new();
        hasher.update(self.seed_hash);
        hasher.update(new_contrib.to_be_bytes());

        self.seed_hash = hasher.finalize().into();
    }

    fn seed(&self) -> u64 {
        // take first 8 bytes from hash and parse it to u64

        let seed = u64::from_be_bytes(self.seed_hash[..8].try_into().unwrap());
        seed
    }
}

pub fn get_bomb_coords(bombs_needed: usize, dimension: u64) -> Vec<u64> {
    let seed = rand::random();
    let mut rng = StdRng::seed_from_u64(seed);

    let mut coords = HashSet::new();
    while coords.len() < bombs_needed {
        coords.insert(rng.next_u64() % (dimension * dimension));
    }

    coords.into_iter().collect()
}
