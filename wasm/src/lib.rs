//! RaptorQ WASM bindings for browser-based FEC decoding/encoding
//!
//! Implements RFC 6330 RaptorQ forward error correction for use in web applications.

use wasm_bindgen::prelude::*;
use raptorq::{Encoder, Decoder, SourceBlockDecoder, EncodingPacket, ObjectTransmissionInformation};

#[cfg(feature = "console_error_panic_hook")]
pub use console_error_panic_hook::set_once as set_panic_hook;

/// Initialize panic hook for better error messages in console
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// RaptorQ Encoder for generating source and repair symbols
#[wasm_bindgen]
pub struct RaptorQEncoder {
    encoder: Encoder,
    config: ObjectTransmissionInformation,
    repair_symbols_per_block: u32,
}

#[wasm_bindgen]
impl RaptorQEncoder {
    /// Create a new encoder
    ///
    /// # Arguments
    /// * `data` - The data to encode
    /// * `symbol_size` - Size of each symbol in bytes (typically 1024-1400 for network MTU)
    /// * `repair_symbols` - Number of repair symbols per source block
    /// * `source_blocks` - Number of source blocks (usually 1)
    /// * `sub_blocks` - Number of sub-blocks (usually 1)
    /// * `symbol_alignment` - Symbol alignment in bytes (usually 8)
    #[wasm_bindgen(constructor)]
    pub fn new(
        data: &[u8],
        symbol_size: u16,
        repair_symbols: u32,
        source_blocks: u8,
        sub_blocks: u16,
        symbol_alignment: u8,
    ) -> Result<RaptorQEncoder, JsValue> {
        if data.is_empty() {
            return Err(JsValue::from_str("Data cannot be empty"));
        }
        if symbol_alignment == 0 {
            return Err(JsValue::from_str("Symbol alignment must be > 0"));
        }
        if symbol_size % symbol_alignment as u16 != 0 {
            return Err(JsValue::from_str("Symbol size must be divisible by alignment"));
        }

        let config = ObjectTransmissionInformation::new(
            data.len() as u64,
            symbol_size,
            source_blocks,
            sub_blocks,
            symbol_alignment,
        );

        let encoder = Encoder::new(data, config);

        Ok(RaptorQEncoder {
            encoder,
            config,
            repair_symbols_per_block: repair_symbols,
        })
    }

    /// Get the OTI (Object Transmission Information) as 12 bytes
    /// This must be transmitted to the decoder
    #[wasm_bindgen]
    pub fn get_oti(&self) -> Vec<u8> {
        self.config.serialize().to_vec()
    }

    /// Get transfer length (original data size)
    #[wasm_bindgen]
    pub fn transfer_length(&self) -> u64 {
        self.config.transfer_length()
    }

    /// Get symbol size
    #[wasm_bindgen]
    pub fn symbol_size(&self) -> u16 {
        self.config.symbol_size()
    }

    /// Get number of source blocks
    #[wasm_bindgen]
    pub fn source_blocks(&self) -> u8 {
        self.config.source_blocks()
    }

    /// Get all source packets for a specific block
    /// Returns array of serialized packets (each is PayloadId + symbol data)
    #[wasm_bindgen]
    pub fn get_source_packets(&self, block_index: usize) -> Result<Vec<u8>, JsValue> {
        let block_encoders = self.encoder.get_block_encoders();
        if block_index >= block_encoders.len() {
            return Err(JsValue::from_str("Block index out of range"));
        }

        let packets = block_encoders[block_index].source_packets();
        let mut result = Vec::new();
        for packet in packets {
            result.extend_from_slice(&packet.serialize());
        }
        Ok(result)
    }

    /// Get repair packets for a specific block
    /// Returns array of serialized packets
    #[wasm_bindgen]
    pub fn get_repair_packets(&self, block_index: usize, start: u32, count: u32) -> Result<Vec<u8>, JsValue> {
        let block_encoders = self.encoder.get_block_encoders();
        if block_index >= block_encoders.len() {
            return Err(JsValue::from_str("Block index out of range"));
        }

        let packets = block_encoders[block_index].repair_packets(start, count);
        let mut result = Vec::new();
        for packet in packets {
            result.extend_from_slice(&packet.serialize());
        }
        Ok(result)
    }

    /// Get all encoding packets (source + repair) for all blocks
    /// Format: concatenated serialized packets
    #[wasm_bindgen]
    pub fn get_all_packets(&self) -> Vec<u8> {
        let mut result = Vec::new();
        let block_encoders = self.encoder.get_block_encoders();

        for block_encoder in block_encoders {
            // Source packets
            for packet in block_encoder.source_packets() {
                result.extend_from_slice(&packet.serialize());
            }
            // Repair packets
            for packet in block_encoder.repair_packets(0, self.repair_symbols_per_block) {
                result.extend_from_slice(&packet.serialize());
            }
        }
        result
    }

    /// Get the packet size (PayloadId + symbol)
    #[wasm_bindgen]
    pub fn packet_size(&self) -> usize {
        4 + self.config.symbol_size() as usize
    }
}

/// RaptorQ Decoder for recovering data from encoding packets
#[wasm_bindgen]
pub struct RaptorQDecoder {
    decoder: Decoder,
    config: ObjectTransmissionInformation,
    packets_received: u32,
    is_complete: bool,
    decoded_data: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl RaptorQDecoder {
    /// Create a new decoder from OTI bytes
    ///
    /// # Arguments
    /// * `oti` - 12-byte Object Transmission Information from encoder
    #[wasm_bindgen(constructor)]
    pub fn new(oti: &[u8]) -> Result<RaptorQDecoder, JsValue> {
        if oti.len() != 12 {
            return Err(JsValue::from_str("OTI must be exactly 12 bytes"));
        }

        let mut oti_array = [0u8; 12];
        oti_array.copy_from_slice(oti);
        let config = ObjectTransmissionInformation::deserialize(&oti_array);

        let decoder = Decoder::new(config);

        Ok(RaptorQDecoder {
            decoder,
            config,
            packets_received: 0,
            is_complete: false,
            decoded_data: None,
        })
    }

    /// Create decoder with explicit parameters (no OTI needed)
    #[wasm_bindgen]
    pub fn with_params(
        transfer_length: u64,
        symbol_size: u16,
        source_blocks: u8,
        sub_blocks: u16,
        symbol_alignment: u8,
    ) -> Result<RaptorQDecoder, JsValue> {
        let config = ObjectTransmissionInformation::new(
            transfer_length,
            symbol_size,
            source_blocks,
            sub_blocks,
            symbol_alignment,
        );

        let decoder = Decoder::new(config);

        Ok(RaptorQDecoder {
            decoder,
            config,
            packets_received: 0,
            is_complete: false,
            decoded_data: None,
        })
    }

    /// Add an encoding packet to the decoder
    /// Returns true if decoding is now complete
    ///
    /// # Arguments
    /// * `packet` - Serialized encoding packet (PayloadId + symbol data)
    #[wasm_bindgen]
    pub fn add_packet(&mut self, packet: &[u8]) -> Result<bool, JsValue> {
        if self.is_complete {
            return Ok(true);
        }

        let expected_size = 4 + self.config.symbol_size() as usize;
        if packet.len() != expected_size {
            return Err(JsValue::from_str(&format!(
                "Packet size mismatch: expected {}, got {}",
                expected_size,
                packet.len()
            )));
        }

        let encoding_packet = EncodingPacket::deserialize(packet);
        self.packets_received += 1;

        // Try to decode - returns Some(data) when decoding is complete
        if let Some(data) = self.decoder.decode(encoding_packet) {
            self.is_complete = true;
            self.decoded_data = Some(data);
        }

        Ok(self.is_complete)
    }

    /// Add multiple packets at once (more efficient for batches)
    #[wasm_bindgen]
    pub fn add_packets(&mut self, packets_data: &[u8]) -> Result<bool, JsValue> {
        if self.is_complete {
            return Ok(true);
        }

        let packet_size = 4 + self.config.symbol_size() as usize;
        if packets_data.len() % packet_size != 0 {
            return Err(JsValue::from_str("Packets data length must be multiple of packet size"));
        }

        for chunk in packets_data.chunks(packet_size) {
            let encoding_packet = EncodingPacket::deserialize(chunk);
            self.packets_received += 1;

            if let Some(data) = self.decoder.decode(encoding_packet) {
                self.is_complete = true;
                self.decoded_data = Some(data);
                break;
            }
        }

        Ok(self.is_complete)
    }

    /// Check if decoding is complete
    #[wasm_bindgen]
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Get number of packets received
    #[wasm_bindgen]
    pub fn packets_received(&self) -> u32 {
        self.packets_received
    }

    /// Get the decoded data (only valid after is_complete() returns true)
    #[wasm_bindgen]
    pub fn get_result(&self) -> Result<Vec<u8>, JsValue> {
        if !self.is_complete {
            return Err(JsValue::from_str("Decoding not complete"));
        }

        match &self.decoded_data {
            Some(data) => Ok(data.clone()),
            None => Err(JsValue::from_str("No decoded data available")),
        }
    }

    /// Get transfer length
    #[wasm_bindgen]
    pub fn transfer_length(&self) -> u64 {
        self.config.transfer_length()
    }

    /// Get symbol size
    #[wasm_bindgen]
    pub fn symbol_size(&self) -> u16 {
        self.config.symbol_size()
    }

    /// Get expected packet size
    #[wasm_bindgen]
    pub fn packet_size(&self) -> usize {
        4 + self.config.symbol_size() as usize
    }
}

/// Source block decoder for fine-grained control over individual blocks
#[wasm_bindgen]
pub struct RaptorQBlockDecoder {
    decoder: SourceBlockDecoder,
    packets_received: u32,
    is_complete: bool,
    result: Option<Vec<u8>>,
}

#[wasm_bindgen]
impl RaptorQBlockDecoder {
    /// Create a new block decoder
    ///
    /// # Arguments
    /// * `block_number` - Source block number (0-254)
    /// * `symbol_size` - Size of each symbol
    /// * `block_length` - Number of bytes in this block
    #[wasm_bindgen(constructor)]
    pub fn new(block_number: u8, symbol_size: u16, block_length: u64) -> RaptorQBlockDecoder {
        // Create a minimal config for this block
        let config = ObjectTransmissionInformation::new(
            block_length,
            symbol_size,
            1,  // single source block
            1,  // single sub-block
            8,  // default alignment
        );
        let decoder = SourceBlockDecoder::new(block_number, &config, block_length);

        RaptorQBlockDecoder {
            decoder,
            packets_received: 0,
            is_complete: false,
            result: None,
        }
    }

    /// Add a packet to this block decoder
    /// Returns true if this block is now fully decoded
    #[wasm_bindgen]
    pub fn add_packet(&mut self, packet: &[u8]) -> bool {
        if self.is_complete {
            return true;
        }

        let encoding_packet = EncodingPacket::deserialize(packet);
        self.packets_received += 1;

        if let Some(data) = self.decoder.decode(core::iter::once(encoding_packet)) {
            self.is_complete = true;
            self.result = Some(data);
        }

        self.is_complete
    }

    /// Check if decoding is complete
    #[wasm_bindgen]
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Get the decoded block data
    #[wasm_bindgen]
    pub fn get_result(&self) -> Result<Vec<u8>, JsValue> {
        match &self.result {
            Some(data) => Ok(data.clone()),
            None => Err(JsValue::from_str("Block not yet decoded")),
        }
    }

    /// Get packets received count
    #[wasm_bindgen]
    pub fn packets_received(&self) -> u32 {
        self.packets_received
    }
}

/// Utility function to parse OTI
#[wasm_bindgen]
pub fn parse_oti(oti: &[u8]) -> Result<js_sys::Object, JsValue> {
    if oti.len() != 12 {
        return Err(JsValue::from_str("OTI must be exactly 12 bytes"));
    }

    let mut oti_array = [0u8; 12];
    oti_array.copy_from_slice(oti);
    let config = ObjectTransmissionInformation::deserialize(&oti_array);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"transfer_length".into(), &JsValue::from(config.transfer_length()))?;
    js_sys::Reflect::set(&obj, &"symbol_size".into(), &JsValue::from(config.symbol_size()))?;
    js_sys::Reflect::set(&obj, &"source_blocks".into(), &JsValue::from(config.source_blocks()))?;
    js_sys::Reflect::set(&obj, &"sub_blocks".into(), &JsValue::from(config.sub_blocks()))?;
    js_sys::Reflect::set(&obj, &"symbol_alignment".into(), &JsValue::from(config.symbol_alignment()))?;

    Ok(obj)
}

/// Create OTI from parameters
#[wasm_bindgen]
pub fn create_oti(
    transfer_length: u64,
    symbol_size: u16,
    source_blocks: u8,
    sub_blocks: u16,
    symbol_alignment: u8,
) -> Vec<u8> {
    let config = ObjectTransmissionInformation::new(
        transfer_length,
        symbol_size,
        source_blocks,
        sub_blocks,
        symbol_alignment,
    );
    config.serialize().to_vec()
}
