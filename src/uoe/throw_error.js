/**
 * @stability 2 - provisional
 *
 * Converts a uoe-error into a JavaScript error and throws it.
 */
function createError(name, message, details) {
	const err = new Error(message);
	err.name = name;
	err.details = details;
	return err;
}

export const throw_error = (error) => {
	if (error?.sym === "error") {
		console.log("actually found error");
		throw_error(error.data);
	}

	switch (error.type) {
		case "user":
			throw createError(
				"UserError",
				`\ntype   ${error.type}\nclass  ${error.class}\n${error.message}\n`,
				error,
			);

		case "state":
			throw createError(
				"StateError",
				`\ntype   ${error.type}\nname   ${error.name}\n${error.message}\n`,
				error,
			);

		case "internal":
			throw createError("InternalError", `\ntype   ${error.type}\n${error.message}\n`, error);

		default:
			throw new Error(`Unknown error type: ${error.type}`);
	}
};
