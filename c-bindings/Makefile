# RaptorQ C Bindings Makefile

RUST_TARGET_DIR = target/release
WASM_TARGET_DIR = target/wasm32-unknown-emscripten/release
LIB_NAME = raptorq_c

# Library outputs
STATIC_LIB = $(RUST_TARGET_DIR)/lib$(LIB_NAME).a
SHARED_LIB = $(RUST_TARGET_DIR)/lib$(LIB_NAME).so
WASM_LIB = $(WASM_TARGET_DIR)/lib$(LIB_NAME).a
WASM_MODULE = $(WASM_TARGET_DIR)/$(LIB_NAME).wasm

# Installation paths (customizable)
PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include

.PHONY: all clean test install debug release wasm wasm-release

all: release

release:
	cargo build --release

debug:
	cargo build

# Build for WebAssembly (Emscripten target)
# Requires: rustup target add wasm32-unknown-emscripten
# Requires: Emscripten SDK installed and activated
wasm-release wasm:
	@echo "Building RaptorQ for wasm32-unknown-emscripten..."
	@if ! rustup target list | grep -q "wasm32-unknown-emscripten (installed)"; then \
		echo "Installing wasm32-unknown-emscripten target..."; \
		rustup target add wasm32-unknown-emscripten; \
	fi
	@if [ -z "$$EMSDK" ]; then \
		echo "Error: Emscripten SDK not activated. Run: source ~/emsdk/emsdk_env.sh"; \
		exit 1; \
	fi
	RUSTFLAGS="-C link-args=-sSIDE_MODULE=1 -C link-args=-sERROR_ON_UNDEFINED_SYMBOLS=0" \
		cargo build --release --target wasm32-unknown-emscripten
	@echo ""
	@echo "WASM build complete:"
	@ls -lh $(WASM_LIB) $(WASM_MODULE) 2>/dev/null || echo "  Static lib: $(WASM_LIB)"

test: release
	cargo test
	$(CC) -o test_raptorq test/test_raptorq.c -I include -L $(RUST_TARGET_DIR) -l$(LIB_NAME) -lpthread -ldl -lm
	LD_LIBRARY_PATH=$(RUST_TARGET_DIR) ./test_raptorq

clean:
	cargo clean
	rm -f test_raptorq

install: release
	mkdir -p $(LIBDIR) $(INCLUDEDIR)
	cp $(STATIC_LIB) $(LIBDIR)/
	cp $(SHARED_LIB) $(LIBDIR)/
	cp include/raptorq.h $(INCLUDEDIR)/
	ldconfig || true

# Print library info
info:
	@echo "RaptorQ C Bindings"
	@echo "=================="
	@echo "Static library: $(STATIC_LIB)"
	@echo "Shared library: $(SHARED_LIB)"
	@echo "Header:         include/raptorq.h"
	@echo ""
	@echo "Link flags:"
	@echo "  CFLAGS:  -I$(CURDIR)/include"
	@echo "  LDFLAGS: -L$(CURDIR)/$(RUST_TARGET_DIR) -lraptorq_c -lpthread -ldl -lm"
