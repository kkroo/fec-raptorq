# RaptorQ C Bindings Makefile

RUST_TARGET_DIR = target/release
LIB_NAME = raptorq_c

# Library outputs
STATIC_LIB = $(RUST_TARGET_DIR)/lib$(LIB_NAME).a
SHARED_LIB = $(RUST_TARGET_DIR)/lib$(LIB_NAME).so

# Installation paths (customizable)
PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include

.PHONY: all clean test install debug release

all: release

release:
	cargo build --release

debug:
	cargo build

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
