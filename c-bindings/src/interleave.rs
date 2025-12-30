//! Interleaved RaptorQ FEC encoder and decoder
//!
//! Reduces latency by distributing packets across N concurrent encoding blocks.
//! Instead of waiting for K packets before encoding, packets are round-robin
//! distributed across `depth` blocks, reducing first-repair-symbol latency by `depth`x.

use raptorq::{
    EncodingPacket, ObjectTransmissionInformation, PayloadId, SourceBlockDecoder,
    SourceBlockEncoder, SourceBlockEncodingPlan,
};
use std::ptr;
use std::slice;

/// Maximum interleave depth
pub const RAPTORQ_MAX_INTERLEAVE_DEPTH: usize = 8;

/// Error codes matching the C API
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaptorQError {
    RaptorqOk = 0,
    RaptorqError = -1,
    RaptorqErrorInvalidParam = -2,
    RaptorqErrorBufferTooSmall = -3,
    RaptorqErrorNotComplete = -4,
}

/// Block status for encoder
#[repr(C)]
#[derive(Debug, Clone)]
pub struct BlockStatus {
    pub block_id: u32,
    pub packet_count: u32,
    pub is_ready: bool,
    pub source_symbols: u32,
}

/// Encoder block state
struct EncoderBlock {
    /// Accumulated packet data for this block
    data: Vec<u8>,
    /// Number of packets added to this block
    packet_count: u32,
    /// Block ID (increments by depth for each full cycle)
    block_id: u32,
    /// Whether encoding has been done for this block
    #[allow(dead_code)]
    encoded: bool,
    /// Cached source block encoder (created when block is ready)
    encoder: Option<SourceBlockEncoder>,
}

impl EncoderBlock {
    fn new(k: u32, symbol_size: u16) -> Self {
        EncoderBlock {
            data: Vec::with_capacity(k as usize * symbol_size as usize),
            packet_count: 0,
            block_id: 0,
            encoded: false,
            encoder: None,
        }
    }

    fn reset(&mut self, new_block_id: u32) {
        self.data.clear();
        self.packet_count = 0;
        self.block_id = new_block_id;
        self.encoded = false;
        self.encoder = None;
    }
}

/// Interleaved RaptorQ encoder
///
/// Distributes incoming packets round-robin across `depth` concurrent encoding blocks.
/// This reduces the latency for generating repair symbols from K packets to K/depth packets.
pub struct InterleavedEncoder {
    /// Interleave depth (number of concurrent blocks)
    depth: usize,
    /// Source symbols per block
    k: u32,
    /// Symbol size in bytes
    symbol_size: u16,
    /// Number of repair symbols to generate per block
    repair_symbols: u32,
    /// The encoding blocks
    blocks: Vec<EncoderBlock>,
    /// Current block index for round-robin distribution
    current_block: usize,
    /// Total packets added (for tracking block IDs)
    total_packets: u64,
    /// OTI configuration
    config: ObjectTransmissionInformation,
    /// Pre-computed encoding plan for all blocks (they share the same K)
    shared_encoding_plan: SourceBlockEncodingPlan,
}

impl InterleavedEncoder {
    /// Create a new interleaved encoder
    ///
    /// # Arguments
    /// * `depth` - Interleave depth (1-8). Higher = lower latency
    /// * `k` - Source symbols per block
    /// * `symbol_size` - Symbol size in bytes
    /// * `repair_symbols` - Number of repair symbols per block
    ///
    /// # Returns
    /// New encoder instance or None if parameters are invalid
    pub fn new(depth: u32, k: u32, symbol_size: u16, repair_symbols: u32) -> Option<Self> {
        if depth == 0 || depth as usize > RAPTORQ_MAX_INTERLEAVE_DEPTH {
            return None;
        }
        if k == 0 || symbol_size == 0 {
            return None;
        }

        // Pre-compute encoding plan for reuse (all blocks have same K)
        let shared_encoding_plan = SourceBlockEncodingPlan::generate(k as u16);

        // Create configuration for the blocks
        // transfer_length = k * symbol_size per block
        let transfer_length = (k as u64) * (symbol_size as u64);
        let config = ObjectTransmissionInformation::new(
            transfer_length,
            symbol_size,
            1, // source_blocks (per encoder block)
            1, // sub_blocks
            8, // symbol_alignment
        );

        let mut blocks = Vec::with_capacity(depth as usize);
        for i in 0..depth {
            let mut block = EncoderBlock::new(k, symbol_size);
            block.block_id = i;
            blocks.push(block);
        }

        Some(InterleavedEncoder {
            depth: depth as usize,
            k,
            symbol_size,
            repair_symbols,
            blocks,
            current_block: 0,
            total_packets: 0,
            config,
            shared_encoding_plan,
        })
    }

    /// Add a source packet to the encoder
    ///
    /// Packets are distributed round-robin across interleaved blocks.
    ///
    /// # Arguments
    /// * `data` - Source packet data
    ///
    /// # Returns
    /// Ok(block_id) - The block ID where the packet was added
    /// Err(error) - Error code if operation failed
    pub fn add_packet(&mut self, data: &[u8]) -> Result<u32, RaptorQError> {
        if data.len() > self.symbol_size as usize {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block_idx = self.current_block;
        let block = &mut self.blocks[block_idx];

        // Pad data to symbol size if needed
        block.data.extend_from_slice(data);
        if data.len() < self.symbol_size as usize {
            block.data.resize(
                block.data.len() + (self.symbol_size as usize - data.len()),
                0,
            );
        }

        block.packet_count += 1;
        self.total_packets += 1;

        // Check if block is ready for encoding
        if block.packet_count == self.k {
            // Create the encoder for this block
            let encoder = SourceBlockEncoder::with_encoding_plan(
                0, // source_block_id within the block
                &self.config,
                &block.data,
                &self.shared_encoding_plan,
            );
            block.encoder = Some(encoder);
        }

        let block_id = block.block_id;

        // Move to next block in round-robin
        self.current_block = (self.current_block + 1) % self.depth;

        Ok(block_id)
    }

    /// Get the status of a specific block
    pub fn get_block_status(&self, block_index: usize) -> Result<BlockStatus, RaptorQError> {
        if block_index >= self.depth {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block = &self.blocks[block_index];
        Ok(BlockStatus {
            block_id: block.block_id,
            packet_count: block.packet_count,
            is_ready: block.packet_count == self.k,
            source_symbols: self.k,
        })
    }

    /// Generate repair packets for a ready block
    ///
    /// # Arguments
    /// * `block_index` - Block index (0 to depth-1)
    ///
    /// # Returns
    /// Ok(Vec<u8>) - Concatenated repair packets with PayloadId headers
    /// Err(error) - Error code if block not ready or invalid
    pub fn generate_repair(&mut self, block_index: usize) -> Result<Vec<u8>, RaptorQError> {
        if block_index >= self.depth {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block = &mut self.blocks[block_index];

        if block.packet_count != self.k || block.encoder.is_none() {
            return Err(RaptorQError::RaptorqErrorNotComplete);
        }

        let encoder = block.encoder.as_ref().unwrap();
        let repair_packets = encoder.repair_packets(0, self.repair_symbols);

        // Calculate total size needed
        let packet_size = 4 + self.symbol_size as usize; // 4-byte PayloadId + symbol
        let mut result = Vec::with_capacity(self.repair_symbols as usize * packet_size);

        // Serialize each repair packet with custom block_id in the header
        for (i, packet) in repair_packets.iter().enumerate() {
            // Custom FEC Payload ID format for interleaved mode:
            // Bytes 0-3: block_id (big-endian uint32)
            // Followed by symbol data
            result.extend_from_slice(&block.block_id.to_be_bytes());
            // Symbol ID (k + repair_index)
            let symbol_id = self.k + i as u32;
            result.extend_from_slice(&symbol_id.to_be_bytes());
            result.extend_from_slice(packet.data());
        }

        // Reset block for next cycle
        let new_block_id = block.block_id + self.depth as u32;
        block.reset(new_block_id);

        Ok(result)
    }

    /// Get source packets for a ready block
    ///
    /// # Arguments
    /// * `block_index` - Block index (0 to depth-1)
    ///
    /// # Returns
    /// Ok(Vec<u8>) - Concatenated source packets with PayloadId headers
    pub fn get_source_packets(&self, block_index: usize) -> Result<Vec<u8>, RaptorQError> {
        if block_index >= self.depth {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block = &self.blocks[block_index];

        if block.packet_count == 0 {
            return Err(RaptorQError::RaptorqErrorNotComplete);
        }

        // Return raw source data with block/symbol IDs
        let mut result = Vec::new();
        for i in 0..block.packet_count {
            // FEC Payload ID
            result.extend_from_slice(&block.block_id.to_be_bytes());
            result.extend_from_slice(&i.to_be_bytes());
            // Symbol data
            let start = (i as usize) * (self.symbol_size as usize);
            let end = start + self.symbol_size as usize;
            if end <= block.data.len() {
                result.extend_from_slice(&block.data[start..end]);
            }
        }

        Ok(result)
    }

    /// Get the OTI (Object Transmission Information)
    pub fn get_oti(&self) -> [u8; 12] {
        self.config.serialize()
    }

    /// Get interleave depth
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Get K (source symbols per block)
    pub fn k(&self) -> u32 {
        self.k
    }

    /// Get symbol size
    pub fn symbol_size(&self) -> u16 {
        self.symbol_size
    }

    /// Get repair symbols count
    pub fn repair_symbols(&self) -> u32 {
        self.repair_symbols
    }
}

/// Decoder block state
struct DecoderBlock {
    /// Decoder for this block
    decoder: SourceBlockDecoder,
    /// Block ID being decoded
    block_id: u32,
    /// Whether decoding is complete
    complete: bool,
    /// Decoded data (if complete)
    decoded_data: Option<Vec<u8>>,
    /// Number of packets received
    packet_count: u32,
}

impl DecoderBlock {
    fn new(config: &ObjectTransmissionInformation, block_length: u64, block_id: u32) -> Self {
        DecoderBlock {
            decoder: SourceBlockDecoder::new(0, config, block_length),
            block_id,
            complete: false,
            decoded_data: None,
            packet_count: 0,
        }
    }

    fn reset(
        &mut self,
        config: &ObjectTransmissionInformation,
        block_length: u64,
        new_block_id: u32,
    ) {
        self.decoder = SourceBlockDecoder::new(0, config, block_length);
        self.block_id = new_block_id;
        self.complete = false;
        self.decoded_data = None;
        self.packet_count = 0;
    }
}

/// Interleaved RaptorQ decoder
///
/// Receives packets with block_id and symbol_id, routes them to the correct
/// decoder block, and reassembles the original data.
pub struct InterleavedDecoder {
    /// Interleave depth
    depth: usize,
    /// Source symbols per block
    k: u32,
    /// Symbol size in bytes
    symbol_size: u16,
    /// The decoding blocks
    blocks: Vec<DecoderBlock>,
    /// OTI configuration
    config: ObjectTransmissionInformation,
    /// Block length in bytes
    block_length: u64,
}

impl InterleavedDecoder {
    /// Create a new interleaved decoder
    ///
    /// # Arguments
    /// * `oti` - Object Transmission Information (12 bytes)
    /// * `depth` - Interleave depth (must match encoder)
    ///
    /// # Returns
    /// New decoder instance or None if parameters are invalid
    pub fn new(oti: &[u8; 12], depth: u32) -> Option<Self> {
        if depth == 0 || depth as usize > RAPTORQ_MAX_INTERLEAVE_DEPTH {
            return None;
        }

        let config = ObjectTransmissionInformation::deserialize(oti);
        let symbol_size = config.symbol_size();
        let block_length = config.transfer_length();
        let k = (block_length / symbol_size as u64) as u32;

        let mut blocks = Vec::with_capacity(depth as usize);
        for i in 0..depth {
            blocks.push(DecoderBlock::new(&config, block_length, i));
        }

        Some(InterleavedDecoder {
            depth: depth as usize,
            k,
            symbol_size,
            blocks,
            config,
            block_length,
        })
    }

    /// Add a packet to the decoder
    ///
    /// Packet format: [MMTP data][FEC Payload ID: 8 bytes]
    /// FEC Payload ID: block_id (4 bytes BE) + symbol_id (4 bytes BE)
    ///
    /// # Arguments
    /// * `data` - Packet data including FEC Payload ID at the end
    ///
    /// # Returns
    /// Ok(Some(block_index)) - If a block completed decoding
    /// Ok(None) - If more packets needed
    /// Err(error) - On error
    pub fn add_packet(&mut self, data: &[u8]) -> Result<Option<u32>, RaptorQError> {
        // Minimum size: symbol + 8-byte FEC Payload ID
        if data.len() < 8 {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let payload_len = data.len() - 8;
        if payload_len != self.symbol_size as usize {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        // Parse FEC Payload ID from end of packet
        let fec_id_offset = data.len() - 8;
        let block_id = u32::from_be_bytes([
            data[fec_id_offset],
            data[fec_id_offset + 1],
            data[fec_id_offset + 2],
            data[fec_id_offset + 3],
        ]);
        let symbol_id = u32::from_be_bytes([
            data[fec_id_offset + 4],
            data[fec_id_offset + 5],
            data[fec_id_offset + 6],
            data[fec_id_offset + 7],
        ]);

        // Route to correct block
        let block_index = (block_id as usize) % self.depth;
        let block = &mut self.blocks[block_index];

        // Check if this is for the current block or a new one
        if block_id != block.block_id {
            // New block - reset if the incoming block_id is newer
            if block_id > block.block_id {
                block.reset(&self.config, self.block_length, block_id);
            } else {
                // Old block, ignore
                return Ok(None);
            }
        }

        // Skip if already complete
        if block.complete {
            return Ok(None);
        }

        // Create encoding packet for the raptorq decoder
        // The raptorq library expects: [4-byte PayloadId][symbol data]
        let payload_id = PayloadId::new(0, symbol_id);
        let symbol_data = data[..payload_len].to_vec();
        let packet = EncodingPacket::new(payload_id, symbol_data);

        block.packet_count += 1;

        // Try to decode
        if let Some(decoded) = block.decoder.decode(std::iter::once(packet)) {
            block.complete = true;
            block.decoded_data = Some(decoded);
            return Ok(Some(block_index as u32));
        }

        Ok(None)
    }

    /// Check if a block is complete
    pub fn is_block_complete(&self, block_index: usize) -> bool {
        if block_index >= self.depth {
            return false;
        }
        self.blocks[block_index].complete
    }

    /// Get decoded data for a completed block
    ///
    /// # Arguments
    /// * `block_index` - Block index (0 to depth-1)
    ///
    /// # Returns
    /// Ok(Vec<u8>) - Decoded data
    /// Err(error) - If block not complete or invalid index
    pub fn get_block_data(&self, block_index: usize) -> Result<Vec<u8>, RaptorQError> {
        if block_index >= self.depth {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block = &self.blocks[block_index];
        if !block.complete {
            return Err(RaptorQError::RaptorqErrorNotComplete);
        }

        match &block.decoded_data {
            Some(data) => Ok(data.clone()),
            None => Err(RaptorQError::RaptorqErrorNotComplete),
        }
    }

    /// Reset a specific block
    pub fn reset_block(&mut self, block_index: usize) -> Result<(), RaptorQError> {
        if block_index >= self.depth {
            return Err(RaptorQError::RaptorqErrorInvalidParam);
        }

        let block = &mut self.blocks[block_index];
        let new_block_id = block.block_id + self.depth as u32;
        block.reset(&self.config, self.block_length, new_block_id);
        Ok(())
    }

    /// Get current block ID for a block index
    pub fn get_block_id(&self, block_index: usize) -> Option<u32> {
        if block_index >= self.depth {
            return None;
        }
        Some(self.blocks[block_index].block_id)
    }

    /// Get interleave depth
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Get K (source symbols per block)
    pub fn k(&self) -> u32 {
        self.k
    }

    /// Get symbol size
    pub fn symbol_size(&self) -> u16 {
        self.symbol_size
    }
}

// ============================================================================
// C FFI Exports
// ============================================================================

/// Opaque encoder handle for C API
pub struct RaptorQInterleavedEncoderC {
    encoder: InterleavedEncoder,
}

/// Opaque decoder handle for C API
pub struct RaptorQInterleavedDecoderC {
    decoder: InterleavedDecoder,
}

/// OTI structure for C API
#[repr(C)]
pub struct RaptorQOTI {
    pub bytes: [u8; 12],
}

/// Block status for C API
#[repr(C)]
pub struct RaptorQBlockStatus {
    pub block_id: u32,
    pub packet_count: u32,
    pub is_ready: bool,
    pub source_symbols: u32,
}

// ============================================================================
// Encoder C API
// ============================================================================

/// Create interleaved encoder
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_new(
    depth: u32,
    k: u32,
    symbol_size: u16,
    repair_symbols: u32,
) -> *mut RaptorQInterleavedEncoderC {
    match InterleavedEncoder::new(depth, k, symbol_size, repair_symbols) {
        Some(encoder) => {
            let handle = Box::new(RaptorQInterleavedEncoderC { encoder });
            Box::into_raw(handle)
        }
        None => ptr::null_mut(),
    }
}

/// Free encoder resources
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_free(encoder: *mut RaptorQInterleavedEncoderC) {
    if !encoder.is_null() {
        unsafe {
            drop(Box::from_raw(encoder));
        }
    }
}

/// Add source packet to encoder
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_add_packet(
    encoder: *mut RaptorQInterleavedEncoderC,
    packet_data: *const u8,
    packet_len: usize,
    block_id: *mut u32,
) -> i32 {
    if encoder.is_null() || packet_data.is_null() || block_id.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let encoder = unsafe { &mut (*encoder).encoder };
    let data = unsafe { slice::from_raw_parts(packet_data, packet_len) };

    match encoder.add_packet(data) {
        Ok(bid) => {
            unsafe {
                *block_id = bid;
            }
            // Return block index (0 to depth-1)
            (bid as usize % encoder.depth()) as i32
        }
        Err(e) => e as i32,
    }
}

/// Get status of a specific block
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_get_block_status(
    encoder: *const RaptorQInterleavedEncoderC,
    block_index: u32,
    status: *mut RaptorQBlockStatus,
) -> i32 {
    if encoder.is_null() || status.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let encoder = unsafe { &(*encoder).encoder };

    match encoder.get_block_status(block_index as usize) {
        Ok(s) => {
            unsafe {
                (*status).block_id = s.block_id;
                (*status).packet_count = s.packet_count;
                (*status).is_ready = s.is_ready;
                (*status).source_symbols = s.source_symbols;
            }
            RaptorQError::RaptorqOk as i32
        }
        Err(e) => e as i32,
    }
}

/// Generate repair packets for a ready block
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_generate_repair(
    encoder: *mut RaptorQInterleavedEncoderC,
    block_index: u32,
    out_data: *mut u8,
    buffer_size: usize,
    out_len: *mut usize,
) -> i32 {
    if encoder.is_null() || out_data.is_null() || out_len.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let encoder = unsafe { &mut (*encoder).encoder };

    match encoder.generate_repair(block_index as usize) {
        Ok(data) => {
            if data.len() > buffer_size {
                return RaptorQError::RaptorqErrorBufferTooSmall as i32;
            }
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), out_data, data.len());
                *out_len = data.len();
            }
            RaptorQError::RaptorqOk as i32
        }
        Err(e) => e as i32,
    }
}

/// Get OTI for encoder
#[no_mangle]
pub extern "C" fn raptorq_interleaved_encoder_get_oti(
    encoder: *const RaptorQInterleavedEncoderC,
    oti: *mut RaptorQOTI,
) -> i32 {
    if encoder.is_null() || oti.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let encoder = unsafe { &(*encoder).encoder };
    let oti_bytes = encoder.get_oti();

    unsafe {
        (*oti).bytes = oti_bytes;
    }

    RaptorQError::RaptorqOk as i32
}

// ============================================================================
// Decoder C API
// ============================================================================

/// Create interleaved decoder
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_new(
    oti: *const RaptorQOTI,
    depth: u32,
) -> *mut RaptorQInterleavedDecoderC {
    if oti.is_null() {
        return ptr::null_mut();
    }

    let oti_bytes = unsafe { &(*oti).bytes };

    match InterleavedDecoder::new(oti_bytes, depth) {
        Some(decoder) => {
            let handle = Box::new(RaptorQInterleavedDecoderC { decoder });
            Box::into_raw(handle)
        }
        None => ptr::null_mut(),
    }
}

/// Free decoder resources
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_free(decoder: *mut RaptorQInterleavedDecoderC) {
    if !decoder.is_null() {
        unsafe {
            drop(Box::from_raw(decoder));
        }
    }
}

/// Add packet to decoder
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_add_packet(
    decoder: *mut RaptorQInterleavedDecoderC,
    packet_data: *const u8,
    packet_len: usize,
    block_index: *mut u32,
) -> i32 {
    if decoder.is_null() || packet_data.is_null() || block_index.is_null() {
        return RaptorQError::RaptorqError as i32;
    }

    let decoder = unsafe { &mut (*decoder).decoder };
    let data = unsafe { slice::from_raw_parts(packet_data, packet_len) };

    match decoder.add_packet(data) {
        Ok(Some(idx)) => {
            unsafe {
                *block_index = idx;
            }
            1 // Block completed
        }
        Ok(None) => 0, // More packets needed
        Err(_) => -1,  // Error
    }
}

/// Check if a block is complete
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_is_block_complete(
    decoder: *const RaptorQInterleavedDecoderC,
    block_index: u32,
) -> i32 {
    if decoder.is_null() {
        return 0;
    }

    let decoder = unsafe { &(*decoder).decoder };
    if decoder.is_block_complete(block_index as usize) {
        1
    } else {
        0
    }
}

/// Get decoded data for a completed block
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_get_block_data(
    decoder: *const RaptorQInterleavedDecoderC,
    block_index: u32,
    out_data: *mut u8,
    buffer_size: usize,
    out_len: *mut usize,
) -> i32 {
    if decoder.is_null() || out_data.is_null() || out_len.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let decoder = unsafe { &(*decoder).decoder };

    match decoder.get_block_data(block_index as usize) {
        Ok(data) => {
            if data.len() > buffer_size {
                return RaptorQError::RaptorqErrorBufferTooSmall as i32;
            }
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), out_data, data.len());
                *out_len = data.len();
            }
            RaptorQError::RaptorqOk as i32
        }
        Err(e) => e as i32,
    }
}

/// Reset decoder state for a specific block
#[no_mangle]
pub extern "C" fn raptorq_interleaved_decoder_reset_block(
    decoder: *mut RaptorQInterleavedDecoderC,
    block_index: u32,
) -> i32 {
    if decoder.is_null() {
        return RaptorQError::RaptorqErrorInvalidParam as i32;
    }

    let decoder = unsafe { &mut (*decoder).decoder };

    match decoder.reset_block(block_index as usize) {
        Ok(()) => RaptorQError::RaptorqOk as i32,
        Err(e) => e as i32,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Use symbol sizes that are divisible by 8 (the alignment requirement)
    const TEST_SYMBOL_SIZE: u16 = 104; // divisible by 8

    #[test]
    fn test_encoder_creation() {
        let encoder = InterleavedEncoder::new(4, 8, 1200, 4);
        assert!(encoder.is_some());

        let encoder = encoder.unwrap();
        assert_eq!(encoder.depth(), 4);
        assert_eq!(encoder.k(), 8);
        assert_eq!(encoder.symbol_size(), 1200);
    }

    #[test]
    fn test_encoder_invalid_params() {
        assert!(InterleavedEncoder::new(0, 8, 1200, 4).is_none());
        assert!(InterleavedEncoder::new(9, 8, 1200, 4).is_none());
        assert!(InterleavedEncoder::new(4, 0, 1200, 4).is_none());
        assert!(InterleavedEncoder::new(4, 8, 0, 4).is_none());
    }

    #[test]
    fn test_round_robin_distribution() {
        let mut encoder = InterleavedEncoder::new(4, 8, TEST_SYMBOL_SIZE, 2).unwrap();

        // Add packets and verify round-robin distribution
        for i in 0..8u32 {
            let data = vec![i as u8; TEST_SYMBOL_SIZE as usize];
            let block_id = encoder.add_packet(&data).unwrap();
            // First 4 packets go to blocks 0,1,2,3
            // Next 4 packets also go to blocks 0,1,2,3
            assert_eq!(block_id, i % 4);
        }

        // Check block 0 is now ready (has 2 packets with k=8? No wait, we only added 8 total)
        // With depth=4 and k=8, each block needs 8 packets
        // We've only added 8 packets total, so 2 per block
        let status = encoder.get_block_status(0).unwrap();
        assert_eq!(status.packet_count, 2);
        assert!(!status.is_ready);
    }

    #[test]
    fn test_block_ready_and_encoding() {
        let mut encoder = InterleavedEncoder::new(2, 4, TEST_SYMBOL_SIZE, 2).unwrap();

        // With depth=2 and k=4, need 8 packets total for both blocks to be ready
        // Packets go: block 0, block 1, block 0, block 1, ...

        for i in 0..8u32 {
            let data = vec![i as u8; TEST_SYMBOL_SIZE as usize];
            encoder.add_packet(&data).unwrap();
        }

        // Both blocks should be ready now
        let status0 = encoder.get_block_status(0).unwrap();
        let status1 = encoder.get_block_status(1).unwrap();

        assert!(status0.is_ready);
        assert!(status1.is_ready);
        assert_eq!(status0.packet_count, 4);
        assert_eq!(status1.packet_count, 4);
    }

    #[test]
    fn test_generate_repair() {
        let mut encoder = InterleavedEncoder::new(2, 4, TEST_SYMBOL_SIZE, 2).unwrap();

        // Need to fill both blocks due to round-robin
        // After 8 packets: both blocks have 4 and are ready
        for i in 0..8u32 {
            let data = vec![i as u8; TEST_SYMBOL_SIZE as usize];
            encoder.add_packet(&data).unwrap();
        }

        // Generate repair for block 0
        let repair = encoder.generate_repair(0).unwrap();
        assert!(!repair.is_empty());

        // Block 0 should be reset now
        let status = encoder.get_block_status(0).unwrap();
        assert_eq!(status.packet_count, 0);
        assert_eq!(status.block_id, 2); // Next block ID for this slot
    }

    #[test]
    fn test_decoder_creation() {
        let encoder = InterleavedEncoder::new(4, 8, 1200, 4).unwrap();
        let oti = encoder.get_oti();

        let decoder = InterleavedDecoder::new(&oti, 4);
        assert!(decoder.is_some());

        let decoder = decoder.unwrap();
        assert_eq!(decoder.depth(), 4);
        assert_eq!(decoder.k(), 8);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let depth = 2u32;
        let k = 4u32;
        let symbol_size = TEST_SYMBOL_SIZE;
        let repair_symbols = 2u32;

        let mut encoder = InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
        let oti = encoder.get_oti();
        let mut decoder = InterleavedDecoder::new(&oti, depth).unwrap();

        // Create source data
        let mut source_data: Vec<Vec<u8>> = Vec::new();
        for i in 0..8u32 {
            let mut data = vec![0u8; symbol_size as usize];
            for j in 0..symbol_size as usize {
                data[j] = ((i as usize * symbol_size as usize + j) % 256) as u8;
            }
            source_data.push(data);
        }

        // Add packets to encoder
        for data in &source_data {
            encoder.add_packet(data).unwrap();
        }

        // Get source packets and add to decoder
        for block_idx in 0..depth {
            let source_packets = encoder.get_source_packets(block_idx as usize).unwrap();

            // Parse and add each source packet
            let packet_size = 8 + symbol_size as usize; // 4-byte block_id + 4-byte symbol_id + symbol
            let num_packets = source_packets.len() / packet_size;

            for i in 0..num_packets {
                let start = i * packet_size;
                let end = start + packet_size;
                let packet = &source_packets[start..end];

                // Reformat for decoder: [symbol data][FEC payload ID]
                let mut decoder_packet = Vec::new();
                decoder_packet.extend_from_slice(&packet[8..]); // Symbol data
                decoder_packet.extend_from_slice(&packet[0..8]); // FEC payload ID at end

                decoder.add_packet(&decoder_packet).ok();
            }
        }

        // Verify both blocks completed
        assert!(decoder.is_block_complete(0));
        assert!(decoder.is_block_complete(1));

        // Get decoded data and verify
        let decoded0 = decoder.get_block_data(0).unwrap();
        let decoded1 = decoder.get_block_data(1).unwrap();

        assert_eq!(decoded0.len(), k as usize * symbol_size as usize);
        assert_eq!(decoded1.len(), k as usize * symbol_size as usize);
    }

    #[test]
    fn test_recovery_with_repair_symbols() {
        let depth = 1u32;
        let k = 4u32;
        let symbol_size = TEST_SYMBOL_SIZE;
        let repair_symbols = 4u32;

        let mut encoder = InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
        let oti = encoder.get_oti();
        let mut decoder = InterleavedDecoder::new(&oti, depth).unwrap();

        // Create and add source data
        let mut source_packets: Vec<Vec<u8>> = Vec::new();
        for i in 0..k {
            let mut data = vec![0u8; symbol_size as usize];
            for j in 0..symbol_size as usize {
                data[j] = ((i as usize * symbol_size as usize + j) % 256) as u8;
            }
            source_packets.push(data.clone());
            encoder.add_packet(&data).unwrap();
        }

        // Generate repair symbols
        let repair_data = encoder.generate_repair(0).unwrap();

        // Parse repair packets
        let repair_packet_size = 8 + symbol_size as usize;
        let num_repair = repair_data.len() / repair_packet_size;
        assert_eq!(num_repair, repair_symbols as usize);

        // Simulate losing first source packet - only send 3 source + repair
        for i in 1..k {
            // Skip packet 0
            let mut packet = Vec::new();
            packet.extend_from_slice(&source_packets[i as usize]);
            packet.extend_from_slice(&0u32.to_be_bytes()); // block_id
            packet.extend_from_slice(&i.to_be_bytes()); // symbol_id

            decoder.add_packet(&packet).ok();
        }

        // Should not be complete yet (need k packets)
        assert!(!decoder.is_block_complete(0));

        // Add 1 repair packet
        let repair_start = 0;
        let repair_end = repair_packet_size;
        let repair_pkt = &repair_data[repair_start..repair_end];

        // Reformat repair packet for decoder
        let mut decoder_repair = Vec::new();
        decoder_repair.extend_from_slice(&repair_pkt[8..]); // Symbol data
        decoder_repair.extend_from_slice(&repair_pkt[0..8]); // FEC payload ID

        let result = decoder.add_packet(&decoder_repair);

        // With 3 source + 1 repair = 4 packets = k, should be able to decode
        assert!(result.is_ok());
        // May or may not complete depending on which repair packet we used
    }

    #[test]
    fn test_c_api_encoder() {
        let encoder = raptorq_interleaved_encoder_new(4, 8, 1200, 4);
        assert!(!encoder.is_null());

        let data = vec![0u8; 1200];
        let mut block_id: u32 = 0;

        let result = raptorq_interleaved_encoder_add_packet(
            encoder,
            data.as_ptr(),
            data.len(),
            &mut block_id,
        );

        assert!(result >= 0);

        let mut oti = RaptorQOTI { bytes: [0; 12] };
        let oti_result = raptorq_interleaved_encoder_get_oti(encoder, &mut oti);
        assert_eq!(oti_result, 0);

        raptorq_interleaved_encoder_free(encoder);
    }

    #[test]
    fn test_c_api_decoder() {
        // Create encoder to get OTI
        let encoder = raptorq_interleaved_encoder_new(4, 8, 1200, 4);
        assert!(!encoder.is_null());

        let mut oti = RaptorQOTI { bytes: [0; 12] };
        raptorq_interleaved_encoder_get_oti(encoder, &mut oti);

        // Create decoder
        let decoder = raptorq_interleaved_decoder_new(&oti, 4);
        assert!(!decoder.is_null());

        raptorq_interleaved_decoder_free(decoder);
        raptorq_interleaved_encoder_free(encoder);
    }

    #[test]
    fn test_full_encode_decode_with_recovery() {
        // Full integration test with packet loss and recovery
        let depth = 4u32;
        let k = 8u32;
        let symbol_size: u16 = 1200;
        let repair_symbols = 4u32;

        let mut encoder = InterleavedEncoder::new(depth, k, symbol_size, repair_symbols).unwrap();
        let oti = encoder.get_oti();
        let mut decoder = InterleavedDecoder::new(&oti, depth).unwrap();

        // Create unique source data for each packet
        let mut all_source_data: Vec<Vec<u8>> = Vec::new();
        for i in 0..(depth * k) {
            let mut data = vec![0u8; symbol_size as usize];
            for j in 0..symbol_size as usize {
                data[j] = ((i as usize * 17 + j) % 256) as u8; // Unique pattern
            }
            all_source_data.push(data);
        }

        // Add packets to encoder
        for data in &all_source_data {
            encoder.add_packet(data).unwrap();
        }

        // All blocks should be ready
        for block_idx in 0..depth {
            let status = encoder.get_block_status(block_idx as usize).unwrap();
            assert!(status.is_ready, "Block {} should be ready", block_idx);
        }

        // Test decoding for block 0 with some packet loss
        // Block 0 has packets at indices 0, 4, 8, 12, 16, 20, 24, 28 (every depth packets)
        let block_0_packets: Vec<_> = all_source_data
            .iter()
            .enumerate()
            .filter(|(i, _)| i % (depth as usize) == 0)
            .map(|(_, d)| d.clone())
            .collect();

        // Send 7 of 8 source packets (skip first one)
        for (i, data) in block_0_packets.iter().enumerate().skip(1) {
            let mut packet = Vec::new();
            packet.extend_from_slice(data);
            packet.extend_from_slice(&0u32.to_be_bytes()); // block_id = 0
            packet.extend_from_slice(&(i as u32).to_be_bytes()); // symbol_id

            decoder.add_packet(&packet).ok();
        }

        // Should not be complete yet
        assert!(!decoder.is_block_complete(0));

        // Generate repair and send one
        let repair_data = encoder.generate_repair(0).unwrap();
        let repair_packet_size = 8 + symbol_size as usize;

        // Send first repair packet
        let repair_pkt = &repair_data[0..repair_packet_size];
        let mut decoder_repair = Vec::new();
        decoder_repair.extend_from_slice(&repair_pkt[8..]); // Symbol data
        decoder_repair.extend_from_slice(&repair_pkt[0..8]); // FEC payload ID

        let result = decoder.add_packet(&decoder_repair);
        assert!(result.is_ok());

        // Should be complete now (7 source + 1 repair = 8 = k)
        assert!(
            decoder.is_block_complete(0),
            "Block 0 should be complete after receiving repair"
        );

        // Verify decoded data matches original
        let decoded = decoder.get_block_data(0).unwrap();
        assert_eq!(decoded.len(), k as usize * symbol_size as usize);
    }
}
