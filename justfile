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

# Tag and publish a release; then bump url/sha256 in youguanxinqing/homebrew-tap
release-tag version:
    git tag -a v{{version}} -m "v{{version}}"
    git push origin v{{version}}
    gh release create v{{version}} --title v{{version}} --generate-notes
    @echo "sha256:"
    @curl -sL https://github.com/youguanxinqing/ai-commit-message/archive/refs/tags/v{{version}}.tar.gz | shasum -a 256
