# smk { overwrite: true }
# Rust's build, test and clean, rewritten on every render: the package at the repository root, where cargo init puts
# Cargo.toml. .smk/targets.mk includes this file, and make joins its hooks (build: build-rust) with the Makefile's own
# build, test and clean.
.PHONY: build-rust test-rust clean-rust
build: build-rust
test: test-rust
clean: clean-rust

# Every target in the package, into target/. CARGO_ARGS goes to cargo build: make build CARGO_ARGS=--release.
build-rust:
	cd "$(SMK_ROOT)" && cargo build --all-targets $(CARGO_ARGS)

# Every test in the package. TEST_ARGS goes to cargo test: make test TEST_ARGS=--no-fail-fast.
test-rust:
	cd "$(SMK_ROOT)" && cargo test $(TEST_ARGS)

clean-rust:
	cd "$(SMK_ROOT)" && cargo clean
