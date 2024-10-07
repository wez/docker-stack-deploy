.PHONY: all fmt check test

all: check

test:
	cargo nextest run

check:
	cargo check

fmt:
	cargo +nightly fmt
