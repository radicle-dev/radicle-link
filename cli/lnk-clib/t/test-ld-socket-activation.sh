#!/usr/bin/env bash
set -eou pipefail

: "${USER:?}"

cleanup() {
    echo "cleanup"
    launchctl unload -w ~/Library/LaunchAgents/org.example.echo.plist
    rm ~/Library/LaunchAgents/org.example.echo.plist
}

assert() {
    if [[ "$1" != "$2" ]]; then
        echo "expected $1 == $2";
        exit 1;
    fi
}

assert "Darwin" "$(uname -s)"

cwd="$(dirname "$(greadlink -nf "${BASH_SOURCE[0]}")")"
cd $cwd

echo "building echo server..."
cargo install --root "$cwd" --example echo --debug --path .

echo "setting up job..."
CWD=$cwd envsubst \
    <"$cwd/examples/org.example.echo.plist.in" \
    >~/Library/LaunchAgents/org.example.echo.plist
launchctl load -w ~/Library/LaunchAgents/org.example.echo.plist
trap 'cleanup' EXIT

echo "testing echo server..."
pong=$(echo "ping" | socat -t 10 UNIX-CONNECT:/tmp/echo.sock -)
assert "ping" "$pong"
