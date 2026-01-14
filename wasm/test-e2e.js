#!/usr/bin/env node
/**
 * End-to-End Test for RaptorQ WASM with SIMD128
 *
 * Tests:
 * 1. SIMD support detection
 * 2. Encode/decode round-trip
 * 3. FEC recovery with packet loss
 * 4. Data integrity verification
 * 5. Performance benchmarking
 */

import fs from "fs";
import path from "path";
import { performance } from "perf_hooks";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// ANSI color codes
const colors = {
	reset: "\x1b[0m",
	green: "\x1b[32m",
	red: "\x1b[31m",
	yellow: "\x1b[33m",
	blue: "\x1b[34m",
	cyan: "\x1b[36m",
	bold: "\x1b[1m",
};

function log(message, color = "reset") {
	console.log(`${colors[color]}${message}${colors.reset}`);
}

function logSection(title) {
	console.log("\n" + "=".repeat(80));
	log(title, "bold");
	console.log("=".repeat(80) + "\n");
}

function logTest(name, passed, details = "") {
	const icon = passed ? "✅" : "❌";
	const color = passed ? "green" : "red";
	log(`${icon} ${name}`, color);
	if (details) {
		console.log(`   ${details}`);
	}
}

// Check if WASM file exists
function checkWasmFile() {
	const wasmPath = path.join(__dirname, "pkg", "raptorq_wasm_bg.wasm");
	if (!fs.existsSync(wasmPath)) {
		log("❌ WASM file not found. Please build first:", "red");
		log("   cd " + __dirname, "yellow");
		log("   ./build-simd.sh", "yellow");
		process.exit(1);
	}
	return wasmPath;
}

// Check SIMD support (Node.js 16.4+)
function checkSIMDSupport() {
	const nodeVersion = process.versions.node.split(".").map(Number);
	const major = nodeVersion[0];
	const minor = nodeVersion[1];

	const supported = major > 16 || (major === 16 && minor >= 4);

	logTest(
		"Node.js SIMD Support",
		supported,
		`Node.js ${process.versions.node} ${supported ? "supports" : "does not support"} WASM SIMD`,
	);

	return supported;
}

// Load WASM module
async function loadWasm() {
	try {
		const wasmJsPath = path.join(__dirname, "pkg", "raptorq_wasm.js");
		const wasmBinaryPath = path.join(__dirname, "pkg", "raptorq_wasm_bg.wasm");

		// Read WASM binary
		const wasmBinary = fs.readFileSync(wasmBinaryPath);

		// Import module
		const wasm = await import(wasmJsPath);

		// Initialize with binary
		await wasm.default(wasmBinary);

		return wasm;
	} catch (error) {
		log("❌ Failed to load WASM module:", "red");
		console.error(error);
		process.exit(1);
	}
}

// Test 1: Basic encode/decode
function testBasicEncodeDecode(wasm) {
	log("Test 1: Basic Encode/Decode", "cyan");

	try {
		// Create test data
		const dataSize = 10000;
		const data = new Uint8Array(dataSize);
		for (let i = 0; i < dataSize; i++) {
			data[i] = i % 256;
		}

		// Encode
		const encoder = new wasm.RaptorQEncoder(
			data,
			1280, // symbol_size
			4, // repair_symbols
			1, // source_blocks
			1, // sub_blocks
			8, // alignment
		);

		const oti = encoder.get_oti();
		const allPackets = encoder.get_all_packets();
		const packetSize = encoder.packet_size();

		logTest("Encoder Creation", true, `OTI: ${oti.length} bytes, Packets: ${allPackets.length} bytes`);

		// Decode (no packet loss)
		const decoder = new wasm.RaptorQDecoder(oti);

		let packetsAdded = 0;
		for (let i = 0; i < allPackets.length / packetSize; i++) {
			const offset = i * packetSize;
			const packet = allPackets.slice(offset, offset + packetSize);
			const complete = decoder.add_packet(packet);
			packetsAdded++;
			if (complete) break;
		}

		const isComplete = decoder.is_complete();
		logTest("Decoding Complete", isComplete, `Packets added: ${packetsAdded}`);

		if (isComplete) {
			const decoded = decoder.get_result();

			// Verify data integrity
			let matches = decoded.length === data.length;
			if (matches) {
				for (let i = 0; i < data.length; i++) {
					if (data[i] !== decoded[i]) {
						matches = false;
						break;
					}
				}
			}

			logTest("Data Integrity", matches, `Original: ${data.length} bytes, Decoded: ${decoded.length} bytes`);

			return matches;
		}

		return false;
	} catch (error) {
		logTest("Basic Encode/Decode", false, error.message);
		return false;
	}
}

// Test 2: FEC recovery with packet loss
function testFECRecovery(wasm) {
	log("\nTest 2: FEC Recovery with Packet Loss", "cyan");

	const testCases = [
		{ lossRate: 0.1, name: "10% loss", repairSymbols: 3 },
		{ lossRate: 0.2, name: "20% loss", repairSymbols: 5 },
		{ lossRate: 0.3, name: "30% loss", repairSymbols: 8 },
	];

	let allPassed = true;

	for (const testCase of testCases) {
		try {
			// Create test data
			const dataSize = 12800;
			const data = new Uint8Array(dataSize);
			for (let i = 0; i < dataSize; i++) {
				data[i] = (i * 137 + 42) % 256; // Pseudo-random pattern
			}

			// Encode with FEC
			const encoder = new wasm.RaptorQEncoder(
				data,
				1280, // symbol_size
				testCase.repairSymbols, // repair_symbols (adjusted per test)
				1, // source_blocks
				1, // sub_blocks
				8, // alignment
			);

			const oti = encoder.get_oti();
			const allPackets = encoder.get_all_packets();
			const packetSize = encoder.packet_size();
			const totalPackets = allPackets.length / packetSize;

			// Simulate packet loss
			const decoder = new wasm.RaptorQDecoder(oti);
			let sent = 0;
			let lost = 0;
			let received = 0;

			for (let i = 0; i < totalPackets; i++) {
				sent++;
				// Drop packets randomly
				if (Math.random() < testCase.lossRate) {
					lost++;
					continue;
				}

				const offset = i * packetSize;
				const packet = allPackets.slice(offset, offset + packetSize);
				const complete = decoder.add_packet(packet);
				received++;

				if (complete) break;
			}

			const isComplete = decoder.is_complete();
			const actualLossRate = ((lost / sent) * 100).toFixed(1);

			if (isComplete) {
				const decoded = decoder.get_result();

				// Verify
				let matches = decoded.length === data.length;
				if (matches) {
					for (let i = 0; i < data.length; i++) {
						if (data[i] !== decoded[i]) {
							matches = false;
							break;
						}
					}
				}

				logTest(
					testCase.name,
					matches,
					`Sent: ${sent}, Lost: ${lost} (${actualLossRate}%), Received: ${received}, Decoded: ${decoded.length} bytes`,
				);

				allPassed = allPassed && matches;
			} else {
				logTest(
					testCase.name,
					false,
					`Failed to decode. Sent: ${sent}, Lost: ${lost} (${actualLossRate}%), Received: ${received}`,
				);
				allPassed = false;
			}
		} catch (error) {
			logTest(testCase.name, false, error.message);
			allPassed = false;
		}
	}

	return allPassed;
}

// Test 3: Performance benchmark
function testPerformance(wasm) {
	log("\nTest 3: Performance Benchmark", "cyan");

	const iterations = 50;
	const dataSize = 10000;

	try {
		// Prepare test data
		const data = new Uint8Array(dataSize);
		for (let i = 0; i < dataSize; i++) {
			data[i] = Math.floor(Math.random() * 256);
		}

		// Benchmark encoding
		const encodeStart = performance.now();
		let lastEncoder = null;

		for (let i = 0; i < iterations; i++) {
			lastEncoder = new wasm.RaptorQEncoder(data, 1280, 4, 1, 1, 8);
			lastEncoder.get_all_packets();
		}

		const encodeEnd = performance.now();
		const encodeTime = (encodeEnd - encodeStart) / iterations;
		const encodeThroughput = (dataSize / encodeTime / 1024).toFixed(2);

		log(`  Encode: ${encodeTime.toFixed(2)} ms/operation (${encodeThroughput} MB/s)`, "green");

		// Benchmark decoding
		const oti = lastEncoder.get_oti();
		const allPackets = lastEncoder.get_all_packets();
		const packetSize = lastEncoder.packet_size();

		const decodeStart = performance.now();

		for (let i = 0; i < iterations; i++) {
			const decoder = new wasm.RaptorQDecoder(oti);

			for (let j = 0; j < allPackets.length / packetSize; j++) {
				const offset = j * packetSize;
				const packet = allPackets.slice(offset, offset + packetSize);
				const complete = decoder.add_packet(packet);
				if (complete) break;
			}

			if (decoder.is_complete()) {
				decoder.get_result();
			}
		}

		const decodeEnd = performance.now();
		const decodeTime = (decodeEnd - decodeStart) / iterations;
		const decodeThroughput = (dataSize / decodeTime / 1024).toFixed(2);

		log(`  Decode: ${decodeTime.toFixed(2)} ms/operation (${decodeThroughput} MB/s)`, "green");

		// Estimate speedup (compared to typical scalar performance)
		const scalarEncode = 180; // MB/s (typical scalar)
		const scalarDecode = 130; // MB/s (typical scalar)
		const encodeSpeedup = (parseFloat(encodeThroughput) / scalarEncode).toFixed(2);
		const decodeSpeedup = (parseFloat(decodeThroughput) / scalarDecode).toFixed(2);

		log(`  Estimated speedup vs scalar: ${encodeSpeedup}x encode, ${decodeSpeedup}x decode`, "blue");

		return true;
	} catch (error) {
		logTest("Performance Benchmark", false, error.message);
		return false;
	}
}

// Test 4: Edge cases
function testEdgeCases(wasm) {
	log("\nTest 4: Edge Cases", "cyan");

	let allPassed = true;

	// Test 4a: Small data
	try {
		const smallData = new Uint8Array(100);
		for (let i = 0; i < 100; i++) smallData[i] = i;

		const encoder = new wasm.RaptorQEncoder(smallData, 64, 2, 1, 1, 8);
		const oti = encoder.get_oti();
		const packets = encoder.get_all_packets();

		const decoder = new wasm.RaptorQDecoder(oti);
		const packetSize = encoder.packet_size();

		for (let i = 0; i < packets.length / packetSize; i++) {
			const offset = i * packetSize;
			const packet = packets.slice(offset, offset + packetSize);
			const complete = decoder.add_packet(packet);
			if (complete) break;
		}

		const decoded = decoder.get_result();
		let matches = decoded.length === smallData.length;
		if (matches) {
			for (let i = 0; i < smallData.length; i++) {
				if (smallData[i] !== decoded[i]) {
					matches = false;
					break;
				}
			}
		}

		logTest("Small data (100 bytes)", matches);
		allPassed = allPassed && matches;
	} catch (error) {
		logTest("Small data", false, error.message);
		allPassed = false;
	}

	// Test 4b: Large data
	try {
		const largeData = new Uint8Array(100000);
		for (let i = 0; i < 100000; i++) largeData[i] = i % 256;

		const encoder = new wasm.RaptorQEncoder(largeData, 1280, 4, 1, 1, 8);
		const oti = encoder.get_oti();
		const packets = encoder.get_all_packets();

		const decoder = new wasm.RaptorQDecoder(oti);
		const packetSize = encoder.packet_size();

		for (let i = 0; i < packets.length / packetSize; i++) {
			const offset = i * packetSize;
			const packet = packets.slice(offset, offset + packetSize);
			const complete = decoder.add_packet(packet);
			if (complete) break;
		}

		const decoded = decoder.get_result();
		const matches = decoded.length === largeData.length;

		logTest("Large data (100KB)", matches, `Decoded ${decoded.length} bytes`);
		allPassed = allPassed && matches;
	} catch (error) {
		logTest("Large data", false, error.message);
		allPassed = false;
	}

	return allPassed;
}

// Main test runner
async function runTests() {
	logSection("RaptorQ WASM SIMD128 - End-to-End Test");

	log("Node.js Version: " + process.versions.node, "blue");
	log("V8 Version: " + process.versions.v8, "blue");
	log("Platform: " + process.platform + " " + process.arch, "blue");

	// Pre-checks
	logSection("Pre-checks");
	checkWasmFile();
	const simdSupported = checkSIMDSupport();

	if (!simdSupported) {
		log("\n⚠️  WASM SIMD may not be available. Tests will use scalar fallback.", "yellow");
	}

	// Load WASM
	logSection("Loading WASM Module");
	const wasm = await loadWasm();
	logTest("WASM Module Loaded", true);

	// Run tests
	logSection("Running Tests");

	const results = {
		basicEncodeDecode: testBasicEncodeDecode(wasm),
		fecRecovery: testFECRecovery(wasm),
		performance: testPerformance(wasm),
		edgeCases: testEdgeCases(wasm),
	};

	// Summary
	logSection("Test Summary");

	const passed = Object.values(results).filter((r) => r).length;
	const total = Object.keys(results).length;

	console.log(`Tests Passed: ${passed}/${total}`);
	console.log("");

	if (passed === total) {
		log("✅ ALL TESTS PASSED", "green");
		process.exit(0);
	} else {
		log("❌ SOME TESTS FAILED", "red");
		process.exit(1);
	}
}

// Run tests
runTests().catch((error) => {
	console.error("Fatal error:", error);
	process.exit(1);
});
