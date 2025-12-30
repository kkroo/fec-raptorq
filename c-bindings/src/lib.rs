//! C bindings for RaptorQ FEC encoding/decoding
//!
//! Provides FFI-safe interface for RFC 6330 RaptorQ.
//!
//! Includes both standard RaptorQ API and interleaved FEC API for reduced latency.

use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};
use std::ptr;
use std::slice;

// Interleaved FEC module
pub mod interleave;

// Re-export interleaved API
pub use interleave::{
    raptorq_interleaved_decoder_add_packet, raptorq_interleaved_decoder_free,
    raptorq_interleaved_decoder_get_block_data, raptorq_interleaved_decoder_is_block_complete,
    raptorq_interleaved_decoder_new, raptorq_interleaved_decoder_reset_block,
    raptorq_interleaved_encoder_add_packet, raptorq_interleaved_encoder_free,
    raptorq_interleaved_encoder_generate_repair, raptorq_interleaved_encoder_get_block_status,
    raptorq_interleaved_encoder_get_oti, raptorq_interleaved_encoder_new, InterleavedDecoder,
    InterleavedEncoder, RaptorQBlockStatus, RaptorQInterleavedDecoderC, RaptorQInterleavedEncoderC,
};

/// Opaque encoder handle
pub struct RaptorQEncoderC {
    encoder: Encoder,
    config: ObjectTransmissionInformation,
}

/// Opaque decoder handle
pub struct RaptorQDecoderC {
    decoder: Decoder,
    config: ObjectTransmissionInformation,
    is_complete: bool,
    decoded_data: Option<Vec<u8>>,
}

/// Result structure for encoding operations
#[repr(C)]
pub struct RaptorQResult {
    pub data: *mut u8,
    pub len: usize,
    pub success: i32,
}

/// OTI (Object Transmission Information) structure - 12 bytes per RFC 6330
#[repr(C)]
pub struct RaptorQOTI {
    pub bytes: [u8; 12],
}

// ============================================================================
// Encoder API
// ============================================================================

/// Create a new RaptorQ encoder
///
/// # Arguments
/// * `data` - Pointer to data to encode
/// * `data_len` - Length of data
/// * `symbol_size` - Size of each symbol (typically 1024-1280 for network MTU)
/// * `repair_symbols` - Number of repair symbols to generate per block
///
/// # Returns
/// Encoder handle or NULL on error
#[no_mangle]
pub extern "C" fn raptorq_encoder_new(
    data: *const u8,
    data_len: usize,
    symbol_size: u16,
    _repair_symbols: u32,
) -> *mut RaptorQEncoderC {
    if data.is_null() || data_len == 0 {
        return ptr::null_mut();
    }

    let data_slice = unsafe { slice::from_raw_parts(data, data_len) };

    // Use standard parameters: 1 source block, 1 sub-block, 8-byte alignment
    let config = ObjectTransmissionInformation::new(
        data_len as u64,
        symbol_size,
        1, // source_blocks
        1, // sub_blocks
        8, // symbol_alignment
    );

    let encoder = Encoder::new(data_slice, config);

    let enc = Box::new(RaptorQEncoderC { encoder, config });

    Box::into_raw(enc)
}

/// Free encoder resources
#[no_mangle]
pub extern "C" fn raptorq_encoder_free(encoder: *mut RaptorQEncoderC) {
    if !encoder.is_null() {
        unsafe {
            drop(Box::from_raw(encoder));
        }
    }
}

/// Get OTI (Object Transmission Information) for this encoder
/// Must be transmitted to decoder for proper decoding
#[no_mangle]
pub extern "C" fn raptorq_encoder_get_oti(
    encoder: *const RaptorQEncoderC,
    oti: *mut RaptorQOTI,
) -> i32 {
    if encoder.is_null() || oti.is_null() {
        return -1;
    }

    let enc = unsafe { &*encoder };
    let serialized = enc.config.serialize();

    unsafe {
        (*oti).bytes.copy_from_slice(&serialized);
    }

    0
}

/// Get transfer length (original data size)
#[no_mangle]
pub extern "C" fn raptorq_encoder_transfer_length(encoder: *const RaptorQEncoderC) -> u64 {
    if encoder.is_null() {
        return 0;
    }
    let enc = unsafe { &*encoder };
    enc.config.transfer_length()
}

/// Get symbol size
#[no_mangle]
pub extern "C" fn raptorq_encoder_symbol_size(encoder: *const RaptorQEncoderC) -> u16 {
    if encoder.is_null() {
        return 0;
    }
    let enc = unsafe { &*encoder };
    enc.config.symbol_size()
}

/// Get packet size (4-byte PayloadId + symbol)
#[no_mangle]
pub extern "C" fn raptorq_encoder_packet_size(encoder: *const RaptorQEncoderC) -> usize {
    if encoder.is_null() {
        return 0;
    }
    let enc = unsafe { &*encoder };
    4 + enc.config.symbol_size() as usize
}

/// Get number of source symbols for block 0
#[no_mangle]
pub extern "C" fn raptorq_encoder_source_symbol_count(encoder: *const RaptorQEncoderC) -> u32 {
    if encoder.is_null() {
        return 0;
    }
    let enc = unsafe { &*encoder };
    let block_encoders = enc.encoder.get_block_encoders();
    if block_encoders.is_empty() {
        return 0;
    }
    block_encoders[0].source_packets().len() as u32
}

/// Get all source packets for block 0
/// Returns concatenated serialized packets (each is 4-byte PayloadId + symbol)
///
/// # Arguments
/// * `encoder` - Encoder handle
/// * `out_data` - Output buffer (must be large enough)
/// * `out_len` - On input: buffer size. On output: bytes written
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn raptorq_encoder_get_source_packets(
    encoder: *const RaptorQEncoderC,
    out_data: *mut u8,
    out_len: *mut usize,
) -> i32 {
    if encoder.is_null() || out_data.is_null() || out_len.is_null() {
        return -1;
    }

    let enc = unsafe { &*encoder };
    let block_encoders = enc.encoder.get_block_encoders();

    if block_encoders.is_empty() {
        unsafe {
            *out_len = 0;
        }
        return 0;
    }

    let packets = block_encoders[0].source_packets();
    let mut result = Vec::new();

    for packet in packets {
        result.extend_from_slice(&packet.serialize());
    }

    let max_len = unsafe { *out_len };
    if result.len() > max_len {
        return -1; // Buffer too small
    }

    unsafe {
        ptr::copy_nonoverlapping(result.as_ptr(), out_data, result.len());
        *out_len = result.len();
    }

    0
}

/// Get repair packets for block 0
///
/// # Arguments
/// * `encoder` - Encoder handle
/// * `start_index` - Starting repair symbol index
/// * `count` - Number of repair symbols to generate
/// * `out_data` - Output buffer
/// * `out_len` - On input: buffer size. On output: bytes written
///
/// # Returns
/// 0 on success, -1 on error
#[no_mangle]
pub extern "C" fn raptorq_encoder_get_repair_packets(
    encoder: *const RaptorQEncoderC,
    start_index: u32,
    count: u32,
    out_data: *mut u8,
    out_len: *mut usize,
) -> i32 {
    if encoder.is_null() || out_data.is_null() || out_len.is_null() {
        return -1;
    }

    let enc = unsafe { &*encoder };
    let block_encoders = enc.encoder.get_block_encoders();

    if block_encoders.is_empty() {
        unsafe {
            *out_len = 0;
        }
        return 0;
    }

    let packets = block_encoders[0].repair_packets(start_index, count);
    let mut result = Vec::new();

    for packet in packets {
        result.extend_from_slice(&packet.serialize());
    }

    let max_len = unsafe { *out_len };
    if result.len() > max_len {
        return -1; // Buffer too small
    }

    unsafe {
        ptr::copy_nonoverlapping(result.as_ptr(), out_data, result.len());
        *out_len = result.len();
    }

    0
}

// ============================================================================
// Decoder API
// ============================================================================

/// Create a new RaptorQ decoder from OTI
#[no_mangle]
pub extern "C" fn raptorq_decoder_new(oti: *const RaptorQOTI) -> *mut RaptorQDecoderC {
    if oti.is_null() {
        return ptr::null_mut();
    }

    let oti_bytes = unsafe { &(*oti).bytes };
    let config = ObjectTransmissionInformation::deserialize(oti_bytes);
    let decoder = Decoder::new(config);

    let dec = Box::new(RaptorQDecoderC {
        decoder,
        config,
        is_complete: false,
        decoded_data: None,
    });

    Box::into_raw(dec)
}

/// Create decoder with explicit parameters (no OTI needed)
#[no_mangle]
pub extern "C" fn raptorq_decoder_new_with_params(
    transfer_length: u64,
    symbol_size: u16,
    source_blocks: u8,
    sub_blocks: u16,
    symbol_alignment: u8,
) -> *mut RaptorQDecoderC {
    let config = ObjectTransmissionInformation::new(
        transfer_length,
        symbol_size,
        source_blocks,
        sub_blocks,
        symbol_alignment,
    );
    let decoder = Decoder::new(config);

    let dec = Box::new(RaptorQDecoderC {
        decoder,
        config,
        is_complete: false,
        decoded_data: None,
    });

    Box::into_raw(dec)
}

/// Free decoder resources
#[no_mangle]
pub extern "C" fn raptorq_decoder_free(decoder: *mut RaptorQDecoderC) {
    if !decoder.is_null() {
        unsafe {
            drop(Box::from_raw(decoder));
        }
    }
}

/// Add a packet to the decoder
///
/// # Arguments
/// * `decoder` - Decoder handle
/// * `packet` - Serialized packet (4-byte PayloadId + symbol)
/// * `packet_len` - Packet length
///
/// # Returns
/// 1 if decoding is now complete, 0 if more packets needed, -1 on error
#[no_mangle]
pub extern "C" fn raptorq_decoder_add_packet(
    decoder: *mut RaptorQDecoderC,
    packet: *const u8,
    packet_len: usize,
) -> i32 {
    if decoder.is_null() || packet.is_null() {
        return -1;
    }

    let dec = unsafe { &mut *decoder };

    if dec.is_complete {
        return 1;
    }

    let expected_size = 4 + dec.config.symbol_size() as usize;
    if packet_len != expected_size {
        return -1;
    }

    let packet_slice = unsafe { slice::from_raw_parts(packet, packet_len) };
    let encoding_packet = EncodingPacket::deserialize(packet_slice);

    if let Some(data) = dec.decoder.decode(encoding_packet) {
        dec.is_complete = true;
        dec.decoded_data = Some(data);
        return 1;
    }

    0
}

/// Check if decoding is complete
#[no_mangle]
pub extern "C" fn raptorq_decoder_is_complete(decoder: *const RaptorQDecoderC) -> i32 {
    if decoder.is_null() {
        return 0;
    }
    let dec = unsafe { &*decoder };
    if dec.is_complete {
        1
    } else {
        0
    }
}

/// Get transfer length
#[no_mangle]
pub extern "C" fn raptorq_decoder_transfer_length(decoder: *const RaptorQDecoderC) -> u64 {
    if decoder.is_null() {
        return 0;
    }
    let dec = unsafe { &*decoder };
    dec.config.transfer_length()
}

/// Get symbol size
#[no_mangle]
pub extern "C" fn raptorq_decoder_symbol_size(decoder: *const RaptorQDecoderC) -> u16 {
    if decoder.is_null() {
        return 0;
    }
    let dec = unsafe { &*decoder };
    dec.config.symbol_size()
}

/// Get expected packet size
#[no_mangle]
pub extern "C" fn raptorq_decoder_packet_size(decoder: *const RaptorQDecoderC) -> usize {
    if decoder.is_null() {
        return 0;
    }
    let dec = unsafe { &*decoder };
    4 + dec.config.symbol_size() as usize
}

/// Get decoded data after decoding is complete
///
/// # Arguments
/// * `decoder` - Decoder handle
/// * `out_data` - Output buffer (must be large enough for transfer_length bytes)
/// * `out_len` - On input: buffer size. On output: bytes written
///
/// # Returns
/// 0 on success, -1 if decoding not complete or error
#[no_mangle]
pub extern "C" fn raptorq_decoder_get_data(
    decoder: *const RaptorQDecoderC,
    out_data: *mut u8,
    max_len: usize,
    out_len: *mut usize,
) -> i32 {
    if decoder.is_null() || out_data.is_null() || out_len.is_null() {
        return -1;
    }

    let dec = unsafe { &*decoder };

    if !dec.is_complete {
        return -1; // Decoding not complete
    }

    match &dec.decoded_data {
        Some(data) => {
            if data.len() > max_len {
                return -1; // Buffer too small
            }
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), out_data, data.len());
                *out_len = data.len();
            }
            0
        }
        None => -1, // No data available
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Create OTI from parameters
#[no_mangle]
pub extern "C" fn raptorq_create_oti(
    transfer_length: u64,
    symbol_size: u16,
    source_blocks: u8,
    sub_blocks: u16,
    symbol_alignment: u8,
    oti: *mut RaptorQOTI,
) -> i32 {
    if oti.is_null() {
        return -1;
    }

    let config = ObjectTransmissionInformation::new(
        transfer_length,
        symbol_size,
        source_blocks,
        sub_blocks,
        symbol_alignment,
    );

    unsafe {
        (*oti).bytes.copy_from_slice(&config.serialize());
    }

    0
}

/// Parse OTI bytes and extract transfer length
#[no_mangle]
pub extern "C" fn raptorq_oti_transfer_length(oti: *const RaptorQOTI) -> u64 {
    if oti.is_null() {
        return 0;
    }
    let oti_bytes = unsafe { &(*oti).bytes };
    let config = ObjectTransmissionInformation::deserialize(oti_bytes);
    config.transfer_length()
}

/// Parse OTI bytes and extract symbol size
#[no_mangle]
pub extern "C" fn raptorq_oti_symbol_size(oti: *const RaptorQOTI) -> u16 {
    if oti.is_null() {
        return 0;
    }
    let oti_bytes = unsafe { &(*oti).bytes };
    let config = ObjectTransmissionInformation::deserialize(oti_bytes);
    config.symbol_size()
}

/// Free memory allocated by this library
#[no_mangle]
pub extern "C" fn raptorq_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, len, len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let data = vec![0u8; 1000];

        let encoder = raptorq_encoder_new(data.as_ptr(), data.len(), 128, 5);
        assert!(!encoder.is_null());

        let mut oti = RaptorQOTI { bytes: [0; 12] };
        assert_eq!(raptorq_encoder_get_oti(encoder, &mut oti), 0);

        let decoder = raptorq_decoder_new(&oti);
        assert!(!decoder.is_null());

        raptorq_encoder_free(encoder);
        raptorq_decoder_free(decoder);
    }
}
