use env_logger::init;

use super::data::{BLAKE2BConstNums, BLAKE2BConsts, BLAKE2BData};
use super::{BLAKE2BAir, IV};
use crate::chip::memory::pointer::slice::Slice;
use crate::chip::memory::time::Time;
use crate::chip::register::array::ArrayRegister;
use crate::chip::register::bit::BitRegister;
use crate::chip::register::element::ElementRegister;
use crate::chip::register::{Register, RegisterSerializable};
use crate::chip::uint::operations::instruction::UintInstructions;
use crate::chip::uint::register::U64Register;
use crate::chip::uint::util::u64_to_le_field_bytes;
use crate::chip::AirParameters;
use crate::machine::builder::Builder;
use crate::machine::bytes::builder::BytesBuilder;
use crate::machine::hash::blake::blake2b::data::{
    BLAKE2BMemory, BLAKE2BPublicData, BLAKE2BTraceData, MemoryArray,
};
use crate::machine::hash::blake::blake2b::{
    COMPRESS_INITIALIZE_INDICES, COMPRESS_IV, SIGMA_PERMUTATIONS, V_INDICES, V_LAST_WRITE_AGES,
};
use crate::math::prelude::*;

const DUMMY_INDEX: u64 = i32::MAX as u64;
const DUMMY_TS: u64 = i32::MAX as u64;

impl<L: AirParameters> BLAKE2BAir<L>
where
    L::Instruction: UintInstructions,
{
    fn cycles_end_bits(
        builder: &mut BytesBuilder<L>,
    ) -> (
        BitRegister,
        BitRegister,
        BitRegister,
        BitRegister,
        BitRegister,
    ) {
        let cycle_4 = builder.cycle(2);
        let cycle_8 = builder.cycle(3);
        let loop_3 = builder.api.loop_instr(3);
        let cycle_96_end_bit = {
            let cycle_32 = builder.cycle(5);
            builder.mul(loop_3.get_iteration_reg(2), cycle_32.end_bit)
        };
        let cycle_12_end_bit = builder.mul(loop_3.get_iteration_reg(2), cycle_4.end_bit);

        (
            loop_3.get_iteration_reg(2),
            cycle_4.end_bit,
            cycle_8.end_bit,
            cycle_12_end_bit,
            cycle_96_end_bit,
        )
    }

    pub fn blake2b(
        builder: &mut BytesBuilder<L>,
        padded_chunks: &[ArrayRegister<U64Register>],
        t_values: &ArrayRegister<U64Register>,
        end_bits: &ArrayRegister<BitRegister>,
        digest_bits: &ArrayRegister<BitRegister>,
        digest_indices: &ArrayRegister<ElementRegister>,
        num_messages: &ElementRegister,
    ) -> Vec<ArrayRegister<U64Register>> {
        let data = Self::blake2b_data(
            builder,
            padded_chunks,
            t_values,
            end_bits,
            digest_bits,
            digest_indices,
            num_messages,
        );

        let (v_indices, v_values, previous_compress_id) =
            Self::blake2b_compress_initialize(builder, &data);

        Self::blake2b_compress(builder, &v_indices, &v_values, previous_compress_id, &data)
    }

    pub fn blake2b_const_nums(builder: &mut BytesBuilder<L>) -> BLAKE2BConstNums {
        BLAKE2BConstNums {
            const_0: builder.constant(&L::Field::from_canonical_u8(0)),
            const_0_u64: builder.constant(&u64_to_le_field_bytes(0u64)),
            const_1: builder.constant(&L::Field::from_canonical_u8(1)),
            const_2: builder.constant(&L::Field::from_canonical_u8(2)),
            const_3: builder.constant(&L::Field::from_canonical_u8(3)),
            const_4: builder.constant(&L::Field::from_canonical_u8(4)),
            const_9: builder.constant(&L::Field::from_canonical_u8(9)),
            const_96: builder.constant(&L::Field::from_canonical_u8(96)),
            const_ffffffffffffffff: builder.constant::<U64Register>(&u64_to_le_field_bytes::<
                L::Field,
            >(
                0xFFFFFFFFFFFFFFFF
            )),
        }
    }

    pub fn blake2b_const(
        builder: &mut BytesBuilder<L>,
        num_compress_element: &ElementRegister,
        num_mix_iterations: &ElementRegister,
        const_nums: &BLAKE2BConstNums,
    ) -> BLAKE2BConsts<L> {
        assert!(DUMMY_INDEX < L::Field::order());
        let dummy_index: ElementRegister =
            builder.constant(&L::Field::from_canonical_u64(DUMMY_INDEX));

        assert!(DUMMY_TS < L::Field::order());
        let dummy_ts: ElementRegister = builder.constant(&L::Field::from_canonical_u64(DUMMY_TS));

        let iv_values = builder.constant_array::<U64Register>(&IV.map(u64_to_le_field_bytes));

        let compress_iv_values =
            builder.constant_array::<U64Register>(&COMPRESS_IV.map(u64_to_le_field_bytes));
        let compress_iv = builder.uninit_slice();
        for (i, value) in compress_iv_values.iter().enumerate() {
            builder.store(
                &compress_iv.get(i),
                value,
                &Time::zero(),
                Some(*num_compress_element),
            );
        }
        let num_dummy_reads = 2 * (96 - 4) * 2;
        let num_dummy_reads_element =
            builder.constant(&L::Field::from_canonical_usize(num_dummy_reads));
        builder.store(
            &compress_iv.get_at(dummy_index),
            const_nums.const_0_u64,
            &Time::zero(),
            Some(num_dummy_reads_element),
        );

        let mut compress_initial_indices = MemoryArray::<L, 4, 2>::new(builder);
        for (i, indices) in COMPRESS_INITIALIZE_INDICES.iter().enumerate() {
            compress_initial_indices.store_row(builder, i, indices, *num_compress_element);
        }
        // Add in the stores for the dummy index.  There will be two reads for all compress rows
        // other than the first four.
        let num_dummy_reads = 2 * (96 - 4);
        let num_dummy_reads_element =
            builder.constant(&L::Field::from_canonical_usize(num_dummy_reads));
        builder.store(
            &compress_initial_indices
                .flattened_memory
                .get_at(dummy_index),
            dummy_index,
            &Time::zero(),
            Some(num_dummy_reads_element),
        );

        // Each element is loaded once per compress cycle.
        let mut v_indices = MemoryArray::<L, 8, 4>::new(builder);
        for (i, indices) in V_INDICES.iter().enumerate() {
            v_indices.store_row(builder, i, indices, *num_mix_iterations);
        }

        let mut v_last_write_ages = MemoryArray::<L, 8, 4>::new(builder);
        for (i, ages) in V_LAST_WRITE_AGES.iter().enumerate() {
            v_last_write_ages.store_row(builder, i, ages, *num_mix_iterations);
        }

        let mut permutations = MemoryArray::<L, 12, 16>::new(builder);
        for (i, permutation) in SIGMA_PERMUTATIONS.iter().enumerate() {
            permutations.store_row(builder, i, permutation, *num_mix_iterations);
        }

        BLAKE2BConsts {
            iv_values,
            compress_iv,
            compress_initial_indices,
            v_indices,
            v_last_write_ages,
            permutations,
            dummy_index,
            dummy_ts,
        }
    }

    // This function will create all the registers/memory slots that will be used for control flow
    // related functions.
    pub fn blake2b_trace_data(
        builder: &mut BytesBuilder<L>,
        const_nums: &BLAKE2BConstNums,
        num_rounds_element: &ElementRegister,
        end_bits: &ArrayRegister<BitRegister>,
        digest_bits: &ArrayRegister<BitRegister>,
    ) -> BLAKE2BTraceData {
        let (cycle_3_end_bit, cycle_4_end_bit, cycle_8_end_bit, cycle_12_end_bit, cycle_96_end_bit) =
            Self::cycles_end_bits(builder);

        // Allocate end_bits from public input.
        let end_bit = builder.uninit_slice();
        for (i, end_bit_val) in end_bits.iter().enumerate() {
            builder.store(
                &end_bit.get(i),
                end_bit_val,
                &Time::zero(),
                Some(const_nums.const_96),
            );
        }
        let digest_bit = builder.uninit_slice();
        for (i, digest_bit_val) in digest_bits.iter().enumerate() {
            builder.store(
                &digest_bit.get(i),
                digest_bit_val,
                &Time::zero(),
                Some(const_nums.const_96),
            );
        }

        // `compress_id` is a register is computed by counting the number of cycles. We do this by
        // setting `process_id` to be the cumulative sum of the `end_bit` of each cycle.
        let compress_id: ElementRegister = builder.alloc::<ElementRegister>();
        builder.set_to_expression_first_row(&compress_id, L::Field::ZERO.into());
        builder.set_to_expression_transition(
            &compress_id.next(),
            compress_id.expr() + cycle_96_end_bit.expr(),
        );

        let mix_index = builder.alloc::<ElementRegister>();
        builder.set_to_expression_first_row(&mix_index, L::Field::ZERO.into());
        builder.set_to_expression_transition(
            &mix_index.next(),
            cycle_8_end_bit.not_expr() * (mix_index.expr() + const_nums.const_1.expr())
                + cycle_8_end_bit.expr() * const_nums.const_0.expr(),
        );

        // The array index register can be computed as `clock - process_id * CYCLE_LENGTH`.
        let clk = builder.clk;
        let compress_index =
            builder.expression(clk.expr() - compress_id.expr() * const_nums.const_96.expr());

        let compress_iteration = builder.alloc::<ElementRegister>();
        builder.set_to_expression_first_row(&compress_iteration, L::Field::ZERO.into());
        builder.set_to_expression_transition(
            &compress_iteration.next(),
            cycle_12_end_bit.not_expr() * (compress_iteration.expr() + const_nums.const_1.expr())
                + cycle_12_end_bit.expr() * const_nums.const_0.expr(),
        );

        let at_last_hash_compress = builder.load(&end_bit.get_at(compress_id), &Time::zero());
        let at_first_compress = builder.alloc::<BitRegister>();
        builder.set_to_expression_first_row(&at_first_compress, L::Field::ONE.into());
        builder.set_to_expression_transition(
            &at_first_compress.next(),
            (cycle_96_end_bit.not_expr() * at_first_compress.expr())
                + (cycle_96_end_bit.expr() * at_last_hash_compress.expr()),
        );

        // Flag if we are within the first four rows of a hash invocation.  In these rows, we will
        // need to use the IV values.
        let is_hash_initialize = builder.alloc::<BitRegister>();
        builder.set_to_expression_first_row(&is_hash_initialize, L::Field::ONE.into());
        builder.set_to_expression_transition(
            &is_hash_initialize.next(),
            (cycle_96_end_bit.expr() * at_last_hash_compress.expr())
                + (cycle_4_end_bit.not_expr() * is_hash_initialize.expr()
                    + cycle_4_end_bit.expr() * const_nums.const_0.expr()),
        );

        // Flag if we are within the first four rows of a compress.  In these rows, we will need to
        // use the COMPRESS_IV values.
        let is_compress_initialize = builder.alloc::<BitRegister>();
        builder.set_to_expression_first_row(&is_compress_initialize, L::Field::ONE.into());
        builder.set_to_expression_transition(
            &is_compress_initialize.next(),
            (cycle_96_end_bit.expr() * const_nums.const_1.expr())
                + (cycle_96_end_bit.not_expr()
                    * (cycle_4_end_bit.expr() * const_nums.const_0.expr()
                        + cycle_4_end_bit.not_expr() * is_compress_initialize.expr())),
        );

        // Flag if we are in the first row of a hash.  In that case, we will need to do an
        // xor for the v_12 value.
        let is_compress_first_row = builder.alloc::<BitRegister>();
        builder.set_to_expression_first_row(&is_compress_first_row, L::Field::ONE.into());
        builder
            .set_to_expression_transition(&is_compress_first_row.next(), cycle_96_end_bit.expr());

        // Flag if we are in the 3rd row of a hash.  In that case, we will need to do a xor on
        // the v_14 value.
        let is_compress_third_row =
            builder.expression(is_compress_initialize.expr() * cycle_3_end_bit.expr());

        // Need to flag to the last 4 rows of the compress cycle.
        // At those rows, the V values should be saved to v_final, so that those values can be used
        // to calculate the compress h values.
        let save_h: Slice<BitRegister> = builder.uninit_slice();
        let true_const = builder.constant::<BitRegister>(&L::Field::from_canonical_usize(1));
        let false_const = builder.constant::<BitRegister>(&L::Field::from_canonical_usize(0));
        for i in 0..96 {
            builder.store(
                &save_h.get(i),
                if i < 92 { false_const } else { true_const },
                &Time::zero(),
                Some(*num_rounds_element),
            );
        }

        BLAKE2BTraceData {
            clk,
            is_hash_initialize,
            is_compress_initialize,
            is_compress_first_row,
            is_compress_third_row,
            at_first_compress,
            cycle_96_end_bit,
            digest_bit,
            save_h,
            compress_id,
            compress_index,
            compress_iteration,
            mix_index,
        }
    }

    pub fn blake2b_memory(
        builder: &mut BytesBuilder<L>,
        padded_chunks: &[ArrayRegister<U64Register>],
        t_values: &ArrayRegister<U64Register>,
        num_consts: &BLAKE2BConstNums,
        consts: &BLAKE2BConsts<L>,
        num_messages: &ElementRegister,
        num_compresses: &ElementRegister,
    ) -> BLAKE2BMemory {
        let h = builder.uninit_slice();

        // Initialize h with IV values
        // For every first compress of each message, the initial h values will be read twice.
        // Once when initializing the V vector (at the start of the compress function) and once
        // when mixing the V vector into the resulting h vector (at the end of the compress function).
        let num_initial_h_reads = builder.alloc_public::<ElementRegister>();
        builder.set_to_expression(
            &num_initial_h_reads,
            num_messages.expr() * (num_consts.const_2.expr() + num_consts.const_96.expr()),
        );
        for i in 0..8 {
            builder.store(
                &h.get(i),
                consts.iv_values.get(i),
                &Time::from_element(consts.dummy_ts),
                Some(num_initial_h_reads),
            );
        }
        // All of the non compress initial rows will read this dummy index
        let const_184 = builder.constant::<ElementRegister>(&L::Field::from_canonical_usize(184));
        let num_dummy_h_reads = builder.mul(*num_compresses, const_184);
        builder.store(
            &h.get_at(consts.dummy_index),
            num_consts.const_0_u64,
            &Time::from_element(consts.dummy_ts),
            Some(const_184),
        );

        // Initialize the v memory
        // Need to set DUMMY_VALUE for all v indices at DUMMY_TS with multiplicity of 1.
        // Dummy values will be read at very first four rows of the stark.
        let v = builder.uninit_slice();
        for i in 0..16 {
            builder.store(
                &v.get(i),
                num_consts.const_0_u64,
                &Time::from_element(consts.dummy_ts),
                None,
            );
        }

        // Initialize the v final memory
        // Need to set dummy reads.  Two reads will be accessed for every non final set of rows
        // for each compress.
        let v_final = builder.uninit_slice();
        let const_92: ElementRegister = builder.constant(&L::Field::from_canonical_u8(95));
        let num_dummy_v_final_accesses = builder.alloc_public::<ElementRegister>();
        builder.set_to_expression(
            &num_dummy_v_final_accesses,
            num_compresses.expr() * const_92.expr(),
        );
        for i in 0..16 {
            builder.store(
                &v_final.get(i),
                num_consts.const_0_u64,
                &Time::from_element(consts.dummy_ts),
                Some(num_dummy_v_final_accesses),
            );
        }

        // Initialize the m memory
        let m = builder.uninit_slice();

        let mut compress_id = 0;
        // Each message chunk will be read 24 times per compress.  Two times per compress iteration.
        let const_24 = builder.constant::<ElementRegister>(&L::Field::from_canonical_usize(24));
        for padded_chunk in padded_chunks.iter() {
            for (j, word) in padded_chunk.iter().enumerate().take(16) {
                builder.store(
                    &m.get(j),
                    word,
                    &Time::constant(compress_id),
                    Some(const_24),
                );
                compress_id += 1;
            }
        }

        let t = builder.uninit_slice();
        compress_id = 0;
        for t_value in t_values.iter() {
            builder.store(
                &t.get(compress_id),
                t_value,
                &Time::zero(),
                Some(num_consts.const_96),
            );
            compress_id += 1;
        }

        BLAKE2BMemory {
            h,
            v,
            v_final,
            m,
            t,
        }
    }

    pub fn blake2b_data(
        builder: &mut BytesBuilder<L>,
        padded_chunks: &[ArrayRegister<U64Register>],
        t_values: &ArrayRegister<U64Register>,
        end_bits: &ArrayRegister<BitRegister>,
        digest_bits: &ArrayRegister<BitRegister>,
        digest_indices: &ArrayRegister<ElementRegister>,
        num_messages: &ElementRegister,
    ) -> BLAKE2BData<L> {
        assert_eq!(padded_chunks.len(), end_bits.len());

        // create the const numbers data
        let const_nums = Self::blake2b_const_nums(builder);

        let num_compresses = padded_chunks.len();
        // Convert the number of rounds to a field element.
        let num_compresses_element =
            builder.constant(&L::Field::from_canonical_usize(num_compresses));

        let num_mix_iterations = num_compresses * 12;
        let num_mix_iterations_element =
            builder.constant(&L::Field::from_canonical_usize(num_mix_iterations));

        let public = BLAKE2BPublicData {
            padded_chunks: padded_chunks.to_vec(),
            t_values: *t_values,
            end_bits: *end_bits,
            digest_indices: *digest_indices,
        };

        // create the consts data
        let consts = Self::blake2b_const(
            builder,
            &num_compresses_element,
            &num_mix_iterations_element,
            &const_nums,
        );

        // create the trace data
        let trace = Self::blake2b_trace_data(
            builder,
            &const_nums,
            &num_compresses_element,
            end_bits,
            digest_bits,
        );

        // create the memory data
        let memory = Self::blake2b_memory(
            builder,
            padded_chunks,
            t_values,
            &const_nums,
            &consts,
            num_messages,
            &num_compresses_element,
        );

        BLAKE2BData {
            public,
            trace,
            memory,
            consts,
            const_nums,
        }
    }

    /// This function will retrieve the v values that will be inputted into the mix function
    pub fn blake2b_compress_initialize(
        builder: &mut BytesBuilder<L>,
        data: &BLAKE2BData<L>,
    ) -> ([ElementRegister; 4], [U64Register; 4], ElementRegister) {
        let mut init_idx_1 = data.trace.compress_index;
        let mut init_idx_2 = builder.add(data.trace.compress_index, data.const_nums.const_4);

        init_idx_1 = builder.select(
            data.trace.is_compress_initialize,
            &init_idx_1,
            &data.consts.dummy_index,
        );

        init_idx_2 = builder.select(
            data.trace.is_compress_initialize,
            &init_idx_2,
            &data.consts.dummy_index,
        );

        let mut previous_compress_id =
            builder.expression(data.trace.compress_id.expr() - data.const_nums.const_1.expr());

        // If we are within the first compress of a message, then read from a dummy h values from a dummy timestamp.
        previous_compress_id = builder.select(
            data.trace.at_first_compress,
            &data.consts.dummy_ts,
            &previous_compress_id,
        );

        builder.watch(&data.trace.at_first_compress, "at_first_compress");

        let h_value_1 = builder.load(
            &data.memory.h.get_at(init_idx_1),
            &Time::from_element(previous_compress_id),
        );
        let h_value_2 = builder.load(
            &data.memory.h.get_at(init_idx_2),
            &Time::from_element(previous_compress_id),
        );

        builder.watch(&h_value_1, "h value 1");

        let compress_iv_value_1 =
            builder.load(&data.consts.compress_iv.get_at(init_idx_1), &Time::zero());
        let compress_iv_value_2 =
            builder.load(&data.consts.compress_iv.get_at(init_idx_2), &Time::zero());

        // For all the other cycles of compress, read the v values from the v memory. Will need to
        // specify the age of the last write to the v memory entry.
        builder.watch(&data.trace.mix_index, "mix index");
        let v_indices = &data.consts.v_indices;
        let v1_idx = v_indices.get_at(builder, data.trace.mix_index, data.const_nums.const_0);
        let v2_idx = v_indices.get_at(builder, data.trace.mix_index, data.const_nums.const_1);
        let v3_idx = v_indices.get_at(builder, data.trace.mix_index, data.const_nums.const_2);
        let v4_idx = v_indices.get_at(builder, data.trace.mix_index, data.const_nums.const_3);

        let v_last_write_ages = &data.consts.v_last_write_ages;
        let v1_last_write_age =
            v_last_write_ages.get_at(builder, data.trace.mix_index, data.const_nums.const_0);
        let v2_last_write_age =
            v_last_write_ages.get_at(builder, data.trace.mix_index, data.const_nums.const_1);
        let v3_last_write_age =
            v_last_write_ages.get_at(builder, data.trace.mix_index, data.const_nums.const_2);
        let v4_last_write_age =
            v_last_write_ages.get_at(builder, data.trace.mix_index, data.const_nums.const_3);

        let mut v1_last_write_ts =
            builder.expression(data.trace.clk.expr() - v1_last_write_age.expr());
        let mut v2_last_write_ts =
            builder.expression(data.trace.clk.expr() - v2_last_write_age.expr());
        let mut v3_last_write_ts =
            builder.expression(data.trace.clk.expr() - v3_last_write_age.expr());
        let mut v4_last_write_ts =
            builder.expression(data.trace.clk.expr() - v4_last_write_age.expr());

        v1_last_write_ts = builder.select(
            data.trace.is_hash_initialize,
            &data.consts.dummy_ts,
            &v1_last_write_ts,
        );

        v2_last_write_ts = builder.select(
            data.trace.is_hash_initialize,
            &data.consts.dummy_ts,
            &v2_last_write_ts,
        );

        v3_last_write_ts = builder.select(
            data.trace.is_hash_initialize,
            &data.consts.dummy_ts,
            &v3_last_write_ts,
        );

        v4_last_write_ts = builder.select(
            data.trace.is_hash_initialize,
            &data.consts.dummy_ts,
            &v4_last_write_ts,
        );

        builder.watch(&v1_idx, "v1_idx");

        let v1_value = builder.load(
            &data.memory.v.get_at(v1_idx),
            &Time::from_element(v1_last_write_ts),
        );
        let v2_value = builder.load(
            &data.memory.v.get_at(v2_idx),
            &Time::from_element(v2_last_write_ts),
        );
        let v3_value = builder.load(
            &data.memory.v.get_at(v3_idx),
            &Time::from_element(v3_last_write_ts),
        );
        let v4_value = builder.load(
            &data.memory.v.get_at(v4_idx),
            &Time::from_element(v4_last_write_ts),
        );

        let v1_value = builder.select(data.trace.is_compress_initialize, &h_value_1, &v1_value);
        let v2_value = builder.select(data.trace.is_compress_initialize, &h_value_2, &v2_value);
        let v3_value = builder.select(
            data.trace.is_compress_initialize,
            &compress_iv_value_1,
            &v3_value,
        );
        let mut v4_value = builder.select(
            data.trace.is_compress_initialize,
            &compress_iv_value_2,
            &v4_value,
        );

        // If we are at the first compress row, then will need to xor v4 with t
        let t = builder.load(&data.memory.t.get_at(data.trace.compress_id), &Time::zero());

        let v4_xor_t = builder.xor(v4_value, t);
        v4_value = builder.select(data.trace.is_compress_first_row, &v4_xor_t, &v4_value);

        // If we are at the third compress row, then will need to xor v4 with 0xFFFFFFFFFFFFFFFF
        let inverse_v4_value = builder.xor(&v4_value, &data.const_nums.const_ffffffffffffffff);
        v4_value = builder.select(
            data.trace.is_compress_third_row,
            &inverse_v4_value,
            &v4_value,
        );

        builder.watch(&v4_value, "v4_value");

        (
            [v1_idx, v2_idx, v3_idx, v4_idx],
            [v1_value, v2_value, v3_value, v4_value],
            previous_compress_id,
        )
    }

    /// The processing step of a BLAKE2B round.
    pub fn blake2b_compress(
        builder: &mut BytesBuilder<L>,
        v_indices: &[ElementRegister; 4],
        v_values: &[U64Register; 4],
        previous_compress_id: ElementRegister,
        data: &BLAKE2BData<L>,
    ) -> Vec<ArrayRegister<U64Register>> {
        let num_digests = data.public.digest_indices.len();

        // Create the public registers to verify the hash.
        let hash_state_public = (0..num_digests)
            .map(|_| builder.alloc_array_public(4))
            .collect::<Vec<_>>();

        let state_ptr = builder.uninit_slice();

        for (i, h_slice) in data
            .public
            .digest_indices
            .iter()
            .zip(hash_state_public.iter())
        {
            for (j, h) in h_slice.iter().enumerate() {
                builder.free(&state_ptr.get(j), h, &Time::from_element(i));
            }
        }

        let mut permutation_col: ElementRegister =
            builder.mul(data.trace.mix_index, data.const_nums.const_2);

        builder.watch(&data.trace.compress_iteration, "compress iteration");
        builder.watch(&permutation_col, "permutation col");

        let m_idx_1 = data.consts.permutations.get_at(
            builder,
            data.trace.compress_iteration,
            permutation_col,
        );
        permutation_col = builder.add(permutation_col, data.const_nums.const_1);

        let m_idx_2 = data.consts.permutations.get_at(
            builder,
            data.trace.compress_iteration,
            permutation_col,
        );

        builder.watch(&m_idx_1, "m_idx_1");
        builder.watch(&data.trace.compress_id, "compress_id");

        let m_1 = builder.load(
            &data.memory.m.get_at(m_idx_1),
            &Time::from_element(data.trace.compress_id),
        );

        builder.watch(&m_idx_2, "m_idx_2");

        let m_2 = builder.load(
            &data.memory.m.get_at(m_idx_2),
            &Time::from_element(data.trace.compress_id),
        );

        builder.watch(&m_1, "m_1");

        let (updated_v0, updated_v1, updated_v2, updated_v3) = Self::blake2b_mix(
            builder,
            &v_values[0],
            &v_values[1],
            &v_values[2],
            &v_values[3],
            &m_1,
            &m_2,
        );

        let save_h = builder.load(
            &data.trace.save_h.get_at(data.trace.compress_index),
            &Time::zero(),
        );

        builder.watch(&save_h, "save_h");

        let write_ts = builder.select(save_h, &data.trace.compress_id, &data.consts.dummy_ts);

        builder.watch(&write_ts, "write_ts");
        builder.watch(&updated_v0, "updated v");
        let updated_v_values = [updated_v0, updated_v1, updated_v2, updated_v3];
        for (i, value) in updated_v_values.iter().enumerate() {
            builder.store(
                &data.memory.v.get_at(v_indices[i]),
                *value,
                &Time::from_element(builder.clk),
                None,
            );

            builder.watch(&v_indices[i], "v_indices[i]");

            // Note that this will do a "no-op" store if save_h == false.
            builder.store(
                &data.memory.v_final.get_at(v_indices[i]),
                *value,
                &Time::from_element(write_ts),
                Some(save_h.as_element()),
            );
        }

        // If we are at the last cycle of the round, then compute and save the h value.

        // First load the previous round's h value.
        let h_workspace_1 = builder.alloc_array::<U64Register>(8);
        for i in 0..8 {
            let h_value = builder.load(
                &data.memory.h.get(i),
                &Time::from_element(previous_compress_id),
            );
            builder.set_to_expression(&h_workspace_1.get(i), h_value.expr());
        }
        builder.watch(&h_workspace_1.get(0), "h_workspace_1[0]");

        // Xor the first 8 final v values
        let h_workspace_2 = builder.alloc_array::<U64Register>(8);
        let read_ts = builder.select(
            data.trace.cycle_96_end_bit,
            &data.trace.compress_id,
            &data.consts.dummy_ts,
        );
        builder.watch(&read_ts, "read_ts");
        for i in 0..8 {
            let v_i = builder.load(&data.memory.v_final.get(i), &Time::from_element(read_ts));
            let updated_h = builder.xor(h_workspace_1.get(i), v_i);
            builder.set_to_expression(&h_workspace_2.get(i), updated_h.expr());
        }

        builder.watch(&h_workspace_2.get(0), "h_workspace_2[0]");

        // Xor the second 8 final v values
        let digest_bit = builder.load(
            &data.trace.digest_bit.get_at(data.trace.compress_id),
            &Time::zero(),
        );
        let flag = builder.expression(data.trace.cycle_96_end_bit.expr() * digest_bit.expr());
        builder.watch(&flag, "flag");

        let h = builder.alloc_array::<U64Register>(8);
        for i in 0..8 {
            let v_value = builder.load(
                &data.memory.v_final.get(i + 8),
                &Time::from_element(read_ts),
            );
            builder.watch(&v_value, "v_value");
            let xor = builder.xor(h_workspace_2.get(i), v_value);
            builder.set_to_expression(&h.get(i), xor.expr());
            builder.store(
                &data.memory.h.get(i),
                xor,
                &Time::from_element(write_ts),
                Some(flag),
            );
        }

        for (i, element) in h.get_subarray(0..4).iter().enumerate() {
            builder.watch(&element, "element");
            builder.store(
                &state_ptr.get(i),
                element,
                &Time::from_element(data.trace.compress_id),
                Some(flag),
            );
        }

        hash_state_public
    }

    pub fn blake2b_mix(
        builder: &mut BytesBuilder<L>,
        v_a: &U64Register,
        v_b: &U64Register,
        v_c: &U64Register,
        v_d: &U64Register,
        x: &U64Register,
        y: &U64Register,
    ) -> (U64Register, U64Register, U64Register, U64Register) {
        let mut v_a_inter = builder.add(*v_a, *v_b);
        v_a_inter = builder.add(v_a_inter, *x);

        let mut v_d_inter = builder.xor(*v_d, *v_a);
        v_d_inter = builder.rotate_right(v_d_inter, 32);

        let mut v_c_inter = builder.add(*v_c, v_d_inter);

        let mut v_b_inter = builder.xor(*v_b, v_c_inter);
        v_b_inter = builder.rotate_right(v_b_inter, 24);

        v_a_inter = builder.add(v_a_inter, v_b_inter);
        v_a_inter = builder.add(v_a_inter, *y);

        v_d_inter = builder.xor(v_d_inter, v_a_inter);
        v_d_inter = builder.rotate_right(v_d_inter, 16);

        v_c_inter = builder.add(v_c_inter, v_d_inter);

        v_b_inter = builder.xor(v_b_inter, v_c_inter);
        v_b_inter = builder.rotate_right(v_b_inter, 63);

        (v_a_inter, v_b_inter, v_c_inter, v_d_inter)
    }
}
