default: build

build:
    cargo build

release:
    cargo build --release

run:
    cargo run

install:
    cargo install --path .

check:
    cargo check

fmt:
    cargo fmt

lint:
    cargo clippy -- -D warnings

clean:
    cargo clean
