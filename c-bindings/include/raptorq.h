/**
 * RaptorQ FEC C Bindings
 *
 * RFC 6330 Forward Error Correction implementation.
 *
 * Usage:
 *   1. Create encoder with raptorq_encoder_new()
 *   2. Get OTI with raptorq_encoder_get_oti() - must be sent to decoder
 *   3. Get source packets with raptorq_encoder_get_source_packets()
 *   4. Get repair packets with raptorq_encoder_get_repair_packets()
 *   5. Create decoder with raptorq_decoder_new()
 *   6. Add packets with raptorq_decoder_add_packet()
 *   7. Check completion with raptorq_decoder_is_complete()
 */

#ifndef RAPTORQ_H
#define RAPTORQ_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handles */
typedef struct RaptorQEncoderC RaptorQEncoder;
typedef struct RaptorQDecoderC RaptorQDecoder;

/**
 * OTI (Object Transmission Information) - 12 bytes per RFC 6330
 * Must be transmitted from encoder to decoder.
 */
typedef struct {
    uint8_t bytes[12];
} RaptorQOTI;

/* ============================================================================
 * Encoder API
 * ============================================================================ */

/**
 * Create a new RaptorQ encoder
 *
 * @param data         Pointer to data to encode
 * @param data_len     Length of data in bytes
 * @param symbol_size  Size of each symbol (typically 1024-1280 for network MTU)
 * @param repair_symbols Number of repair symbols to generate per block
 *
 * @return Encoder handle or NULL on error
 */
RaptorQEncoder* raptorq_encoder_new(
    const uint8_t* data,
    size_t data_len,
    uint16_t symbol_size,
    uint32_t repair_symbols
);

/**
 * Free encoder resources
 */
void raptorq_encoder_free(RaptorQEncoder* encoder);

/**
 * Get OTI (Object Transmission Information)
 * Must be transmitted to decoder for proper decoding.
 *
 * @param encoder  Encoder handle
 * @param oti      Output OTI structure
 *
 * @return 0 on success, -1 on error
 */
int raptorq_encoder_get_oti(
    const RaptorQEncoder* encoder,
    RaptorQOTI* oti
);

/**
 * Get transfer length (original data size)
 */
uint64_t raptorq_encoder_transfer_length(const RaptorQEncoder* encoder);

/**
 * Get symbol size
 */
uint16_t raptorq_encoder_symbol_size(const RaptorQEncoder* encoder);

/**
 * Get packet size (4-byte PayloadId + symbol)
 */
size_t raptorq_encoder_packet_size(const RaptorQEncoder* encoder);

/**
 * Get number of source symbols for block 0
 */
uint32_t raptorq_encoder_source_symbol_count(const RaptorQEncoder* encoder);

/**
 * Get all source packets for block 0
 * Returns concatenated serialized packets (each is 4-byte PayloadId + symbol)
 *
 * @param encoder   Encoder handle
 * @param out_data  Output buffer (must be large enough)
 * @param out_len   On input: buffer size. On output: bytes written
 *
 * @return 0 on success, -1 on error (buffer too small)
 */
int raptorq_encoder_get_source_packets(
    const RaptorQEncoder* encoder,
    uint8_t* out_data,
    size_t* out_len
);

/**
 * Get repair packets for block 0
 *
 * @param encoder     Encoder handle
 * @param start_index Starting repair symbol index
 * @param count       Number of repair symbols to generate
 * @param out_data    Output buffer
 * @param out_len     On input: buffer size. On output: bytes written
 *
 * @return 0 on success, -1 on error
 */
int raptorq_encoder_get_repair_packets(
    const RaptorQEncoder* encoder,
    uint32_t start_index,
    uint32_t count,
    uint8_t* out_data,
    size_t* out_len
);

/* ============================================================================
 * Decoder API
 * ============================================================================ */

/**
 * Create a new RaptorQ decoder from OTI
 *
 * @param oti  OTI from encoder
 *
 * @return Decoder handle or NULL on error
 */
RaptorQDecoder* raptorq_decoder_new(const RaptorQOTI* oti);

/**
 * Create decoder with explicit parameters (no OTI needed)
 */
RaptorQDecoder* raptorq_decoder_new_with_params(
    uint64_t transfer_length,
    uint16_t symbol_size,
    uint8_t source_blocks,
    uint16_t sub_blocks,
    uint8_t symbol_alignment
);

/**
 * Free decoder resources
 */
void raptorq_decoder_free(RaptorQDecoder* decoder);

/**
 * Add a packet to the decoder
 *
 * @param decoder    Decoder handle
 * @param packet     Serialized packet (4-byte PayloadId + symbol)
 * @param packet_len Packet length
 *
 * @return 1 if decoding complete, 0 if more packets needed, -1 on error
 */
int raptorq_decoder_add_packet(
    RaptorQDecoder* decoder,
    const uint8_t* packet,
    size_t packet_len
);

/**
 * Check if decoding is complete
 *
 * @return 1 if complete, 0 otherwise
 */
int raptorq_decoder_is_complete(const RaptorQDecoder* decoder);

/**
 * Get transfer length
 */
uint64_t raptorq_decoder_transfer_length(const RaptorQDecoder* decoder);

/**
 * Get symbol size
 */
uint16_t raptorq_decoder_symbol_size(const RaptorQDecoder* decoder);

/**
 * Get expected packet size
 */
size_t raptorq_decoder_packet_size(const RaptorQDecoder* decoder);

/**
 * Get decoded data after decoding is complete
 *
 * @param decoder  Decoder handle
 * @param out_data Output buffer (must be large enough for transfer_length bytes)
 * @param max_len  Maximum bytes to write (buffer size)
 * @param out_len  On output: actual bytes written
 *
 * @return 0 on success, -1 if decoding not complete or error
 */
int raptorq_decoder_get_data(
    const RaptorQDecoder* decoder,
    uint8_t* out_data,
    size_t max_len,
    size_t* out_len
);

/* ============================================================================
 * Utility Functions
 * ============================================================================ */

/**
 * Create OTI from parameters
 *
 * @param transfer_length  Total data length
 * @param symbol_size      Symbol size in bytes
 * @param source_blocks    Number of source blocks (usually 1)
 * @param sub_blocks       Number of sub-blocks (usually 1)
 * @param symbol_alignment Alignment in bytes (usually 8)
 * @param oti              Output OTI structure
 *
 * @return 0 on success, -1 on error
 */
int raptorq_create_oti(
    uint64_t transfer_length,
    uint16_t symbol_size,
    uint8_t source_blocks,
    uint16_t sub_blocks,
    uint8_t symbol_alignment,
    RaptorQOTI* oti
);

/**
 * Parse OTI and extract transfer length
 */
uint64_t raptorq_oti_transfer_length(const RaptorQOTI* oti);

/**
 * Parse OTI and extract symbol size
 */
uint16_t raptorq_oti_symbol_size(const RaptorQOTI* oti);

/**
 * Free memory allocated by this library
 */
void raptorq_free(uint8_t* ptr, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* RAPTORQ_H */
