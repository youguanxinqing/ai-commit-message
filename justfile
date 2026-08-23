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
    git archive --format=tar.gz --prefix=ai-commit-message-{{version}}/ \
        -o ai-commit-message-{{version}}.tar.gz v{{version}}
    gh release create v{{version}} --title v{{version}} --generate-notes \
        ai-commit-message-{{version}}.tar.gz
    shasum -a 256 ai-commit-message-{{version}}.tar.gz
    rm ai-commit-message-{{version}}.tar.gz
