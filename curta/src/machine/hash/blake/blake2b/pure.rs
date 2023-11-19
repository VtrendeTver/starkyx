use super::{COMPRESS_IV, HASH_ARRAY_SIZE, WORK_VECTOR_SIZE};
use crate::machine::hash::blake::blake2b::{NUM_MIX_ROUNDS, SIGMA_PERMUTATIONS};

pub struct BLAKE2BPure;

impl BLAKE2BPure {
    fn permute_msgs<T: Clone>(&self, arr: &[T], mix_round_num: usize) -> Vec<T> {
        assert!(mix_round_num <= NUM_MIX_ROUNDS);

        let permutation = SIGMA_PERMUTATIONS[mix_round_num % 10];
        let mut result = vec![arr[0].clone(); arr.len()];

        for (to_index, &from_index) in permutation.iter().enumerate() {
            result[to_index] = arr[from_index as usize].clone();
        }

        result
    }

    pub fn compress(
        msg_chunk: &[u8],
        state: &mut [u64; HASH_ARRAY_SIZE],
        bytes_compressed: u64,
        last_chunk: bool,
    ) -> [u64; HASH_ARRAY_SIZE] {
        // Set up the work vector V
        let mut v: [u64; WORK_VECTOR_SIZE] = [0; WORK_VECTOR_SIZE];

        v[..8].copy_from_slice(&state[..HASH_ARRAY_SIZE]);
        v[8..16].copy_from_slice(&COMPRESS_IV);

        v[12] ^= bytes_compressed;
        if last_chunk {
            v[14] ^= 0xFFFFFFFFFFFFFFFF;
        }

        let msg_u64_chunks = msg_chunk
            .chunks_exact(8)
            .map(|x| u64::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<_>>();

        for i in 0..NUM_MIX_ROUNDS {
            let s = SIGMA_PERMUTATIONS[i];

            Self::mix(
                &mut v,
                0,
                4,
                8,
                12,
                msg_u64_chunks[s[0] as usize],
                msg_u64_chunks[s[1] as usize],
            );
            Self::mix(
                &mut v,
                1,
                5,
                9,
                13,
                msg_u64_chunks[s[2] as usize],
                msg_u64_chunks[s[3] as usize],
            );
            Self::mix(
                &mut v,
                2,
                6,
                10,
                14,
                msg_u64_chunks[s[4] as usize],
                msg_u64_chunks[s[5] as usize],
            );
            Self::mix(
                &mut v,
                3,
                7,
                11,
                15,
                msg_u64_chunks[s[6] as usize],
                msg_u64_chunks[s[7] as usize],
            );

            Self::mix(
                &mut v,
                0,
                5,
                10,
                15,
                msg_u64_chunks[s[8] as usize],
                msg_u64_chunks[s[9] as usize],
            );
            Self::mix(
                &mut v,
                1,
                6,
                11,
                12,
                msg_u64_chunks[s[10] as usize],
                msg_u64_chunks[s[11] as usize],
            );
            Self::mix(
                &mut v,
                2,
                7,
                8,
                13,
                msg_u64_chunks[s[12] as usize],
                msg_u64_chunks[s[13] as usize],
            );
            Self::mix(
                &mut v,
                3,
                4,
                9,
                14,
                msg_u64_chunks[s[14] as usize],
                msg_u64_chunks[s[15] as usize],
            );
        }

        for i in 0..HASH_ARRAY_SIZE {
            state[i] ^= v[i];
        }

        for i in 0..HASH_ARRAY_SIZE {
            state[i] ^= v[i + 8];
        }

        *state
    }

    fn mix(
        v: &mut [u64; WORK_VECTOR_SIZE],
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        x: u64,
        y: u64,
    ) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(32);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(24);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(63);
    }
}
