/**
 * RaptorQ C Bindings Test
 *
 * Tests the RaptorQ encoder/decoder C API.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "raptorq.h"

#define TEST_DATA_SIZE 1000
#define SYMBOL_SIZE 128
#define REPAIR_SYMBOLS 5

int test_encode_decode(void) {
    printf("Test: Basic encode/decode\n");

    /* Create test data */
    uint8_t* data = malloc(TEST_DATA_SIZE);
    if (!data) {
        printf("  FAIL: malloc failed\n");
        return -1;
    }

    for (int i = 0; i < TEST_DATA_SIZE; i++) {
        data[i] = i & 0xFF;
    }

    /* Create encoder */
    RaptorQEncoder* encoder = raptorq_encoder_new(data, TEST_DATA_SIZE, SYMBOL_SIZE, REPAIR_SYMBOLS);
    if (!encoder) {
        printf("  FAIL: encoder creation failed\n");
        free(data);
        return -1;
    }

    printf("  Encoder created successfully\n");
    printf("  Transfer length: %lu\n", (unsigned long)raptorq_encoder_transfer_length(encoder));
    printf("  Symbol size: %u\n", raptorq_encoder_symbol_size(encoder));
    printf("  Packet size: %zu\n", raptorq_encoder_packet_size(encoder));
    printf("  Source symbols: %u\n", raptorq_encoder_source_symbol_count(encoder));

    /* Get OTI */
    RaptorQOTI oti;
    if (raptorq_encoder_get_oti(encoder, &oti) != 0) {
        printf("  FAIL: get_oti failed\n");
        raptorq_encoder_free(encoder);
        free(data);
        return -1;
    }

    printf("  OTI: ");
    for (int i = 0; i < 12; i++) {
        printf("%02x ", oti.bytes[i]);
    }
    printf("\n");

    /* Get source packets */
    size_t packet_size = raptorq_encoder_packet_size(encoder);
    uint32_t source_count = raptorq_encoder_source_symbol_count(encoder);
    size_t source_buf_size = source_count * packet_size;
    uint8_t* source_packets = malloc(source_buf_size);

    if (raptorq_encoder_get_source_packets(encoder, source_packets, &source_buf_size) != 0) {
        printf("  FAIL: get_source_packets failed\n");
        free(source_packets);
        raptorq_encoder_free(encoder);
        free(data);
        return -1;
    }

    printf("  Got %zu bytes of source packets\n", source_buf_size);

    /* Get repair packets */
    size_t repair_buf_size = REPAIR_SYMBOLS * packet_size;
    uint8_t* repair_packets = malloc(repair_buf_size);

    if (raptorq_encoder_get_repair_packets(encoder, 0, REPAIR_SYMBOLS, repair_packets, &repair_buf_size) != 0) {
        printf("  FAIL: get_repair_packets failed\n");
        free(repair_packets);
        free(source_packets);
        raptorq_encoder_free(encoder);
        free(data);
        return -1;
    }

    printf("  Got %zu bytes of repair packets\n", repair_buf_size);

    raptorq_encoder_free(encoder);

    /* Create decoder */
    RaptorQDecoder* decoder = raptorq_decoder_new(&oti);
    if (!decoder) {
        printf("  FAIL: decoder creation failed\n");
        free(repair_packets);
        free(source_packets);
        free(data);
        return -1;
    }

    printf("  Decoder created successfully\n");
    printf("  Expected transfer length: %lu\n", (unsigned long)raptorq_decoder_transfer_length(decoder));

    /* Feed packets to decoder */
    int packets_added = 0;
    for (size_t i = 0; i < source_buf_size; i += packet_size) {
        int result = raptorq_decoder_add_packet(decoder, source_packets + i, packet_size);
        packets_added++;

        if (result == 1) {
            printf("  Decoding complete after %d source packets!\n", packets_added);
            break;
        } else if (result == -1) {
            printf("  FAIL: add_packet returned error\n");
            break;
        }
    }

    if (!raptorq_decoder_is_complete(decoder)) {
        printf("  FAIL: decoding not complete after all source packets\n");
        raptorq_decoder_free(decoder);
        free(repair_packets);
        free(source_packets);
        free(data);
        return -1;
    }

    raptorq_decoder_free(decoder);
    free(repair_packets);
    free(source_packets);
    free(data);

    printf("  PASS\n\n");
    return 0;
}

int test_loss_recovery(void) {
    printf("Test: Packet loss recovery\n");

    /* Create test data */
    uint8_t* data = malloc(TEST_DATA_SIZE);
    for (int i = 0; i < TEST_DATA_SIZE; i++) {
        data[i] = i & 0xFF;
    }

    /* Create encoder with more repair symbols */
    RaptorQEncoder* encoder = raptorq_encoder_new(data, TEST_DATA_SIZE, SYMBOL_SIZE, 10);
    if (!encoder) {
        printf("  FAIL: encoder creation failed\n");
        free(data);
        return -1;
    }

    RaptorQOTI oti;
    raptorq_encoder_get_oti(encoder, &oti);

    size_t packet_size = raptorq_encoder_packet_size(encoder);
    uint32_t source_count = raptorq_encoder_source_symbol_count(encoder);

    /* Get source packets */
    size_t source_buf_size = source_count * packet_size;
    uint8_t* source_packets = malloc(source_buf_size);
    raptorq_encoder_get_source_packets(encoder, source_packets, &source_buf_size);

    /* Get repair packets */
    size_t repair_buf_size = 10 * packet_size;
    uint8_t* repair_packets = malloc(repair_buf_size);
    raptorq_encoder_get_repair_packets(encoder, 0, 10, repair_packets, &repair_buf_size);

    raptorq_encoder_free(encoder);

    /* Create decoder and simulate loss */
    RaptorQDecoder* decoder = raptorq_decoder_new(&oti);

    printf("  Skipping first 3 source packets (simulating loss)\n");

    /* Skip first 3 source packets, feed the rest */
    int packets_added = 0;
    for (size_t i = 3 * packet_size; i < source_buf_size; i += packet_size) {
        int result = raptorq_decoder_add_packet(decoder, source_packets + i, packet_size);
        packets_added++;
        if (result == 1) {
            printf("  Decoding complete after %d source packets (no repair needed)!\n", packets_added);
            goto done;
        }
    }

    /* Add repair packets */
    printf("  Adding repair packets...\n");
    for (size_t i = 0; i < repair_buf_size; i += packet_size) {
        int result = raptorq_decoder_add_packet(decoder, repair_packets + i, packet_size);
        packets_added++;
        if (result == 1) {
            printf("  Decoding complete after adding %zu repair packets!\n", (i / packet_size) + 1);
            goto done;
        }
    }

    if (!raptorq_decoder_is_complete(decoder)) {
        printf("  FAIL: decoding not complete\n");
        raptorq_decoder_free(decoder);
        free(repair_packets);
        free(source_packets);
        free(data);
        return -1;
    }

done:
    raptorq_decoder_free(decoder);
    free(repair_packets);
    free(source_packets);
    free(data);

    printf("  PASS\n\n");
    return 0;
}

int test_oti_functions(void) {
    printf("Test: OTI utility functions\n");

    RaptorQOTI oti;
    if (raptorq_create_oti(TEST_DATA_SIZE, SYMBOL_SIZE, 1, 1, 8, &oti) != 0) {
        printf("  FAIL: create_oti failed\n");
        return -1;
    }

    uint64_t transfer_len = raptorq_oti_transfer_length(&oti);
    uint16_t symbol_size = raptorq_oti_symbol_size(&oti);

    printf("  Created OTI for %lu bytes, symbol size %u\n",
           (unsigned long)transfer_len, symbol_size);

    if (transfer_len != TEST_DATA_SIZE) {
        printf("  FAIL: transfer length mismatch\n");
        return -1;
    }

    if (symbol_size != SYMBOL_SIZE) {
        printf("  FAIL: symbol size mismatch\n");
        return -1;
    }

    printf("  PASS\n\n");
    return 0;
}

int main(void) {
    printf("RaptorQ C Bindings Test Suite\n");
    printf("=============================\n\n");

    int failures = 0;

    if (test_oti_functions() != 0) failures++;
    if (test_encode_decode() != 0) failures++;
    if (test_loss_recovery() != 0) failures++;

    printf("=============================\n");
    if (failures == 0) {
        printf("All tests passed!\n");
        return 0;
    } else {
        printf("%d test(s) failed\n", failures);
        return 1;
    }
}
