/**
 * RaptorQ Interleaved FEC API
 *
 * Provides encoder and decoder for interleaved FEC blocks.
 * Reduces latency by distributing packets across N concurrent blocks.
 *
 * STABLE INTERFACE - DO NOT MODIFY WITHOUT APPROVAL
 * Version: 1.0.0
 */

#ifndef RAPTORQ_INTERLEAVED_H
#define RAPTORQ_INTERLEAVED_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================
 * Types and Constants
 * ============================================================================ */

/** Maximum interleave depth
 * Higher depth = longer burst protection window
 * At 30fps with K=32: depth=30 gives ~1s protection, depth=60 gives ~2s
 */
#define RAPTORQ_MAX_INTERLEAVE_DEPTH 64

/** Opaque encoder handle */
typedef struct RaptorQInterleavedEncoderC RaptorQInterleavedEncoderC;

/** Opaque decoder handle */
typedef struct RaptorQInterleavedDecoderC RaptorQInterleavedDecoderC;

/** OTI (Object Transmission Information) - RFC 6330 */
typedef struct {
    uint8_t bytes[12];
} RaptorQOTI;

/** Error codes */
typedef enum {
    RAPTORQ_OK = 0,
    RAPTORQ_ERROR = -1,
    RAPTORQ_ERROR_INVALID_PARAM = -2,
    RAPTORQ_ERROR_BUFFER_TOO_SMALL = -3,
    RAPTORQ_ERROR_NOT_COMPLETE = -4,
} RaptorQError;

/** Block status for encoder */
typedef struct {
    uint32_t block_id;          /* Block identifier */
    uint32_t packet_count;      /* Number of packets in this block */
    bool is_ready;              /* True if block is ready for encoding */
    uint32_t source_symbols;    /* Number of source symbols (k) */
} RaptorQBlockStatus;

/* ============================================================================
 * Encoder API
 * ============================================================================ */

/**
 * Create interleaved encoder
 *
 * @param depth         Interleave depth (1-64). Higher = better burst protection
 * @param k             Source symbols per block (max packets before encoding)
 * @param symbol_size   Symbol size in bytes (typically 1200-1280 for network MTU)
 * @param repair_symbols Number of repair symbols to generate per block
 *
 * @return Encoder handle or NULL on error
 *
 * Example:
 *   // Depth=4, k=8, symbol_size=1200, repair=4
 *   // Latency: First repair after 8/4 = 2 packets (vs 8 for sequential)
 *   encoder = raptorq_interleaved_encoder_new(4, 8, 1200, 4);
 */
RaptorQInterleavedEncoderC* raptorq_interleaved_encoder_new(
    uint32_t depth,
    uint32_t k,
    uint16_t symbol_size,
    uint32_t repair_symbols
);

/**
 * Free encoder resources
 *
 * @param encoder Encoder handle
 */
void raptorq_interleaved_encoder_free(RaptorQInterleavedEncoderC* encoder);

/**
 * Add source packet to encoder (round-robin distribution)
 *
 * Packets are distributed round-robin across interleaved blocks.
 * When a block reaches k packets, it's automatically marked ready for encoding.
 *
 * @param encoder       Encoder handle
 * @param packet_data   Source packet data (MMTP packet without FEC Payload ID)
 * @param packet_len    Packet length
 * @param block_id      [OUT] Block ID where packet was added
 *
 * @return Block index (0 to depth-1) where packet was added, or negative error code
 *
 * Note: Call raptorq_interleaved_encoder_get_block_status() to check if
 *       block is ready for repair symbol generation.
 */
int32_t raptorq_interleaved_encoder_add_packet(
    RaptorQInterleavedEncoderC* encoder,
    const uint8_t* packet_data,
    size_t packet_len,
    uint32_t* block_id
);

/**
 * Get status of a specific block
 *
 * @param encoder       Encoder handle
 * @param block_index   Block index (0 to depth-1)
 * @param status        [OUT] Block status
 *
 * @return RAPTORQ_OK on success, error code on failure
 */
RaptorQError raptorq_interleaved_encoder_get_block_status(
    const RaptorQInterleavedEncoderC* encoder,
    uint32_t block_index,
    RaptorQBlockStatus* status
);

/**
 * Generate repair packets for a ready block
 *
 * Must be called after block reaches k source packets.
 *
 * @param encoder       Encoder handle
 * @param block_index   Block index (0 to depth-1)
 * @param out_data      [OUT] Buffer for repair packets (concatenated)
 * @param buffer_size   Size of output buffer
 * @param out_len       [OUT] Actual bytes written
 *
 * @return RAPTORQ_OK on success, error code on failure
 *
 * Format of out_data:
 *   [Packet 1][Packet 2]...[Packet N]
 *   Each packet = [4-byte PayloadId][Symbol data]
 *
 * Note: After calling this, block is reset and ready for new packets.
 */
RaptorQError raptorq_interleaved_encoder_generate_repair(
    RaptorQInterleavedEncoderC* encoder,
    uint32_t block_index,
    uint8_t* out_data,
    size_t buffer_size,
    size_t* out_len
);

/**
 * Get OTI (Object Transmission Information) for encoder
 *
 * OTI must be transmitted to decoder (typically in signaling message).
 *
 * @param encoder   Encoder handle
 * @param oti       [OUT] OTI structure (12 bytes)
 *
 * @return RAPTORQ_OK on success, error code on failure
 */
RaptorQError raptorq_interleaved_encoder_get_oti(
    const RaptorQInterleavedEncoderC* encoder,
    RaptorQOTI* oti
);

/* ============================================================================
 * Decoder API
 * ============================================================================ */

/**
 * Create interleaved decoder
 *
 * @param oti   Object Transmission Information (from encoder)
 * @param depth Interleave depth (must match encoder)
 *
 * @return Decoder handle or NULL on error
 */
RaptorQInterleavedDecoderC* raptorq_interleaved_decoder_new(
    const RaptorQOTI* oti,
    uint32_t depth
);

/**
 * Free decoder resources
 *
 * @param decoder Decoder handle
 */
void raptorq_interleaved_decoder_free(RaptorQInterleavedDecoderC* decoder);

/**
 * Add packet to decoder
 *
 * Packet can be source or repair packet. Decoder automatically routes
 * to correct block based on FEC Payload ID.
 *
 * @param decoder       Decoder handle
 * @param packet_data   Packet with FEC Payload ID appended (see format below)
 * @param packet_len    Packet length (including FEC Payload ID)
 * @param block_index   [OUT] Block index that completed (if return value is 1)
 *
 * @return:
 *   1  = Block completed decoding (check block_index)
 *   0  = More packets needed
 *  -1  = Error
 *
 * Packet format for interleaved mode (depth > 1):
 *   [MMTP packet data][FEC Payload ID: 8 bytes]
 *
 *   FEC Payload ID (8 bytes):
 *     Bytes 0-3: block_id (uint32_t, big-endian)
 *     Bytes 4-7: symbol_id (uint32_t, big-endian)
 *
 * Example:
 *   uint8_t packet[1316];
 *   // ... MMTP packet data (1308 bytes) ...
 *   // Append FEC Payload ID (8 bytes)
 *   uint32_t block_id = 42;
 *   uint32_t symbol_id = 5;
 *   memcpy(&packet[1308], &block_id, 4);  // Big-endian!
 *   memcpy(&packet[1312], &symbol_id, 4);
 *
 *   uint32_t completed_block;
 *   int ret = raptorq_interleaved_decoder_add_packet(decoder, packet, 1316, &completed_block);
 *   if (ret == 1) {
 *       // Block completed! Get decoded data.
 *   }
 */
int32_t raptorq_interleaved_decoder_add_packet(
    RaptorQInterleavedDecoderC* decoder,
    const uint8_t* packet_data,
    size_t packet_len,
    uint32_t* block_index
);

/**
 * Check if a block is complete
 *
 * @param decoder       Decoder handle
 * @param block_index   Block index (0 to depth-1)
 *
 * @return 1 if complete, 0 if not complete or error
 */
int32_t raptorq_interleaved_decoder_is_block_complete(
    const RaptorQInterleavedDecoderC* decoder,
    uint32_t block_index
);

/**
 * Get decoded data for a completed block
 *
 * @param decoder       Decoder handle
 * @param block_index   Block index (0 to depth-1)
 * @param out_data      [OUT] Buffer for decoded data
 * @param buffer_size   Size of output buffer
 * @param out_len       [OUT] Actual bytes written
 *
 * @return RAPTORQ_OK on success, error code on failure
 *
 * Format of out_data:
 *   [Packet 1][Packet 2]...[Packet k]
 *   Each packet = original MMTP packet (without FEC Payload ID)
 *
 * Note: Data remains available until block is replaced by new data
 *       with same block_index (after depth blocks have been processed).
 */
RaptorQError raptorq_interleaved_decoder_get_block_data(
    const RaptorQInterleavedDecoderC* decoder,
    uint32_t block_index,
    uint8_t* out_data,
    size_t buffer_size,
    size_t* out_len
);

/**
 * Reset decoder state for a specific block
 *
 * Useful when switching streams or recovering from errors.
 *
 * @param decoder       Decoder handle
 * @param block_index   Block index (0 to depth-1)
 *
 * @return RAPTORQ_OK on success, error code on failure
 */
RaptorQError raptorq_interleaved_decoder_reset_block(
    RaptorQInterleavedDecoderC* decoder,
    uint32_t block_index
);

#ifdef __cplusplus
}
#endif

#endif /* RAPTORQ_INTERLEAVED_H */
