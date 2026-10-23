#!/bin/sh
# Serve the WordPress site set up by setup.sh with goro-httpd, over HTTP
# and HTTPS (with a self-signed certificate made by goro's ext/openssl).
#
# Usage: samples/wordpress/run.sh [extra goro-httpd options]
#
# Environment:
#   HTTP_ADDR   plain HTTP listener (default 127.0.0.1:8080)
#   HTTPS_ADDR  HTTPS listener (default 127.0.0.1:8443, "" disables HTTPS)
#   GORO_BIN    directory with the goro and goro-httpd binaries (default:
#               $CARGO_TARGET_DIR/release, else target/release; built with
#               cargo build --release when missing)
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
SITE="$HERE/site"
HTTP_ADDR=${HTTP_ADDR-127.0.0.1:8080}
HTTPS_ADDR=${HTTPS_ADDR-127.0.0.1:8443}

if [ ! -f "$SITE/wp-config.php" ]; then
    "$HERE/setup.sh"
fi

# Ask cargo for the target directory: it can also come from a cargo config
# file (build.target-dir), not only from $CARGO_TARGET_DIR.
TARGET_DIR=$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null |
    sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
BIN=${GORO_BIN:-${TARGET_DIR:-${CARGO_TARGET_DIR:-$ROOT/target}}/release}
if [ ! -x "$BIN/goro-httpd" ] || [ ! -x "$BIN/goro" ]; then
    echo "Building goro and goro-httpd (release)..."
    (cd "$ROOT" && cargo build --release --bin goro --bin goro-httpd)
fi

set -- -t "$SITE" "$@"
if [ -n "$HTTP_ADDR" ]; then
    set -- "$@" -l "$HTTP_ADDR"
fi
if [ -n "$HTTPS_ADDR" ]; then
    CERTS="$HERE/certs"
    if [ ! -f "$CERTS/cert.pem" ]; then
        mkdir -p "$CERTS"
        "$BIN/goro" "$HERE/mkcert.php" "$CERTS/cert.pem" "$CERTS/key.pem" localhost 127.0.0.1
    fi
    set -- "$@" -s "$HTTPS_ADDR" --tls-cert "$CERTS/cert.pem" --tls-key "$CERTS/key.pem"
fi

# upload_max_filesize/post_max_size: the PHP defaults are 2M/8M.
exec "$BIN/goro-httpd" -d upload_max_filesize=64M -d post_max_size=64M "$@"
