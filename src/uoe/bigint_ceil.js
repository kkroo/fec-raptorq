import { bigint_pos_mod } from "./bigint_pos_mod.js";
import { error_user_payload } from "./error_user_payload.js";
import { throw_error } from "./throw_error.js";

/**
 * @stability 2 - Provisional
 *
 * Allows for obtaining the ceiling of a bigint division.
 */
export const bigint_ceil = (numerator, denominator) => {
	if (typeof numerator !== "bigint") {
		throw_error(error_user_payload("Provided numerator must be bigint."));
	}

	if (typeof denominator !== "bigint") {
		throw_error(error_user_payload("Provided denominator must be bigint."));
	}

	if (denominator === 0n) {
		throw_error(error_user_payload("Provided denominator must be non-zero."));
	}

	const is_negative = (numerator < 0n && denominator >= 0n) || (numerator >= 0n && denominator < 0n);

	if (is_negative) {
		return numerator / denominator;
	}

	const has_remainder = bigint_pos_mod(numerator, denominator) > 0n;
	return numerator / denominator + (has_remainder ? 1n : 0n);
};

export const bigintCeil = bigint_ceil;
