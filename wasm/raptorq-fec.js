/**
 * RaptorQ FEC Integration
 *
 * Provides RaptorQ (RFC 6330) FEC encoding/decoding for web applications.
 *
 * This module wraps the WASM RaptorQ implementation for browser use.
 */

// Import the WASM module (will be initialized on first use)
let wasmModule = null;
let wasmInitPromise = null;

/**
 * Initialize the WASM module
 * @returns {Promise<void>}
 */
export async function initRaptorQ() {
	if (wasmModule) return;

	if (!wasmInitPromise) {
		wasmInitPromise = (async () => {
			// Dynamic import of the WASM module
			const wasm = await import("./pkg/raptorq_wasm.js");
			await wasm.default(); // Initialize WASM
			wasmModule = wasm;
		})();
	}

	await wasmInitPromise;
}

/**
 * Check if RaptorQ WASM is initialized
 */
export function isInitialized() {
	return wasmModule !== null;
}

/**
 * RaptorQ FEC Decoder
 *
 * Handles FEC repair packets and recovers lost source packets.
 */
export class RaptorQFECDecoder {
	constructor(options = {}) {
		this.options = {
			symbolSize: options.symbolSize || 1280,
			sourceBlocks: options.sourceBlocks || 1,
			subBlocks: options.subBlocks || 1,
			alignment: options.alignment || 8,
			...options,
		};

		this.decoders = new Map(); // streamId -> { decoder, oti, packets }
		this.recoveredPackets = [];
		this.stats = {
			packetsReceived: 0,
			repairPacketsReceived: 0,
			packetsRecovered: 0,
			blocksDecoded: 0,
		};
	}

	/**
	 * Initialize decoder for a stream
	 * @param {number} streamId - Stream identifier
	 * @param {Uint8Array} oti - 12-byte OTI
	 */
	initStream(streamId, oti) {
		if (!wasmModule) {
			throw new Error("RaptorQ WASM not initialized. Call initRaptorQ() first.");
		}

		if (oti && oti.length === 12) {
			const decoder = new wasmModule.RaptorQDecoder(oti);
			this.decoders.set(streamId, {
				decoder,
				oti,
				sourcePackets: new Map(), // seqNum -> packet
				repairPackets: [],
				blockComplete: false,
			});
		}
	}

	/**
	 * Initialize decoder with explicit parameters (no OTI message needed)
	 */
	initStreamWithParams(streamId, transferLength, symbolSize, sourceBlocks = 1) {
		if (!wasmModule) {
			throw new Error("RaptorQ WASM not initialized. Call initRaptorQ() first.");
		}

		const decoder = wasmModule.RaptorQDecoder.with_params(
			BigInt(transferLength),
			symbolSize,
			sourceBlocks,
			this.options.subBlocks,
			this.options.alignment,
		);

		this.decoders.set(streamId, {
			decoder,
			sourcePackets: new Map(),
			repairPackets: [],
			blockComplete: false,
			symbolSize,
			transferLength,
		});
	}

	/**
	 * Add a source packet
	 * @param {number} streamId - Stream identifier
	 * @param {number} sequenceNumber - Sequence number
	 * @param {Uint8Array} payload - Packet payload
	 */
	addSourcePacket(streamId, sequenceNumber, payload) {
		const stream = this.decoders.get(streamId);
		if (!stream) return;

		stream.sourcePackets.set(sequenceNumber, payload);
		this.stats.packetsReceived++;
	}

	/**
	 * Add a FEC repair packet
	 * @param {number} streamId - Stream identifier
	 * @param {Uint8Array} repairPayload - Repair symbol data (with FEC payload ID)
	 */
	addRepairPacket(streamId, repairPayload) {
		const stream = this.decoders.get(streamId);
		if (!stream || stream.blockComplete) return;

		stream.repairPackets.push(repairPayload);
		this.stats.repairPacketsReceived++;

		// Try to decode if we have enough packets
		this.tryDecode(streamId);
	}

	/**
	 * Attempt to decode/recover lost packets for a stream
	 */
	tryDecode(streamId) {
		const stream = this.decoders.get(streamId);
		if (!stream || stream.blockComplete) return [];

		const { decoder, sourcePackets, repairPackets } = stream;
		const recovered = [];

		// Add all received source packets to decoder
		for (const [seq, packet] of sourcePackets) {
			try {
				// Create RaptorQ encoding packet format: PayloadId (4 bytes) + symbol
				// PayloadId = SBN (1 byte) + ESI (3 bytes)
				const encodingPacket = new Uint8Array(4 + packet.length);
				encodingPacket[0] = 0; // SBN = 0 for single block
				encodingPacket[1] = (seq >> 16) & 0xff;
				encodingPacket[2] = (seq >> 8) & 0xff;
				encodingPacket[3] = seq & 0xff;
				encodingPacket.set(packet, 4);

				if (decoder.add_packet(encodingPacket)) {
					stream.blockComplete = true;
					this.stats.blocksDecoded++;
					break;
				}
			} catch (e) {
				console.warn("RaptorQ decode error:", e);
			}
		}

		// Add repair packets
		if (!stream.blockComplete) {
			for (const repairPacket of repairPackets) {
				try {
					if (decoder.add_packet(repairPacket)) {
						stream.blockComplete = true;
						this.stats.blocksDecoded++;
						break;
					}
				} catch (e) {
					console.warn("RaptorQ repair decode error:", e);
				}
			}
		}

		// If decoding is complete, we could reconstruct missing packets
		// For now, just track stats
		if (stream.blockComplete) {
			try {
				const result = decoder.get_result();
				// Result contains the full decoded block
				// In practice, we'd need to extract individual packets
				this.stats.packetsRecovered += result.length > 0 ? 1 : 0;
			} catch (e) {
				// get_result may not be available for streaming decoder
			}
		}

		return recovered;
	}

	/**
	 * Get statistics
	 */
	getStats() {
		return { ...this.stats };
	}

	/**
	 * Reset decoder state for a stream
	 */
	resetStream(streamId) {
		const stream = this.decoders.get(streamId);
		if (stream) {
			stream.decoder.free();
			this.decoders.delete(streamId);
		}
	}

	/**
	 * Cleanup all decoders
	 */
	dispose() {
		for (const [, stream] of this.decoders) {
			stream.decoder.free();
		}
		this.decoders.clear();
	}
}

/**
 * RaptorQ FEC Encoder
 *
 * Generates FEC repair packets for streams.
 */
export class RaptorQFECEncoder {
	constructor(options = {}) {
		this.options = {
			symbolSize: options.symbolSize || 1280,
			repairSymbols: options.repairSymbols || 5, // Number of repair symbols per block
			sourceBlocks: options.sourceBlocks || 1,
			subBlocks: options.subBlocks || 1,
			alignment: options.alignment || 8,
			...options,
		};

		this.stats = {
			blocksEncoded: 0,
			repairPacketsGenerated: 0,
		};
	}

	/**
	 * Encode a block of source data and generate repair packets
	 * @param {Uint8Array} data - Source data to protect
	 * @returns {{ sourcePackets: Uint8Array[], repairPackets: Uint8Array[], oti: Uint8Array }}
	 */
	encode(data) {
		if (!wasmModule) {
			throw new Error("RaptorQ WASM not initialized. Call initRaptorQ() first.");
		}

		const encoder = new wasmModule.RaptorQEncoder(
			data,
			this.options.symbolSize,
			this.options.repairSymbols,
			this.options.sourceBlocks,
			this.options.subBlocks,
			this.options.alignment,
		);

		const oti = encoder.get_oti();
		const packetSize = encoder.packet_size();

		// Get source packets
		const sourceData = encoder.get_source_packets(0);
		const sourcePackets = [];
		for (let i = 0; i < sourceData.length; i += packetSize) {
			sourcePackets.push(sourceData.slice(i, i + packetSize));
		}

		// Get repair packets
		const repairData = encoder.get_repair_packets(0, 0, this.options.repairSymbols);
		const repairPackets = [];
		for (let i = 0; i < repairData.length; i += packetSize) {
			repairPackets.push(repairData.slice(i, i + packetSize));
		}

		encoder.free();

		this.stats.blocksEncoded++;
		this.stats.repairPacketsGenerated += repairPackets.length;

		return {
			sourcePackets,
			repairPackets,
			oti,
			packetSize,
		};
	}

	/**
	 * Create OTI (Object Transmission Information) for signaling
	 * @param {number} transferLength - Total data length
	 */
	createOTI(transferLength) {
		if (!wasmModule) {
			throw new Error("RaptorQ WASM not initialized. Call initRaptorQ() first.");
		}

		return wasmModule.create_oti(
			BigInt(transferLength),
			this.options.symbolSize,
			this.options.sourceBlocks,
			this.options.subBlocks,
			this.options.alignment,
		);
	}

	/**
	 * Get encoding statistics
	 */
	getStats() {
		return { ...this.stats };
	}
}

/**
 * Parse OTI bytes into object
 */
export function parseOTI(oti) {
	if (!wasmModule) {
		throw new Error("RaptorQ WASM not initialized. Call initRaptorQ() first.");
	}
	return wasmModule.parse_oti(oti);
}

/**
 * FEC coding type constants
 */
export const FECCodingType = {
	RAPTORQ: "raptorq", // RFC 6330 RaptorQ
	REED_SOLOMON: "rs", // Reed-Solomon (legacy)
	XOR_PARITY: "xor", // Simple XOR (basic)
};

/**
 * Default export for easy importing
 */
export default {
	initRaptorQ,
	isInitialized,
	RaptorQFECEncoder,
	RaptorQFECDecoder,
	parseOTI,
	FECCodingType,
};
