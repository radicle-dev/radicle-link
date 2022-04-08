#!/usr/bin/env bash
set -eou pipefail

cleanup() {
    echo "cleanup"
    systemctl --user stop echo.socket echo.service
    systemctl --user disable echo.socket echo.service
    systemctl --user daemon-reload
}

assert() {
    if [[ "$1" != "$2" ]]; then
        echo "expected $1 == $2";
        exit 1;
    fi
}

assert "Linux" "$(uname -s)"

cwd="$(dirname "$(readlink -nf "${BASH_SOURCE[0]}")")"
cd $cwd

echo "building echo server..."
cargo install --root "$cwd" --example echo --debug --path .

echo "setting up unit..."
CWD=$cwd envsubst <"$cwd/examples/echo.service.in" >"$cwd/examples/echo.service"
systemctl --user link "$cwd/examples/echo.service"
systemctl --user link "$cwd/examples/echo.socket"
systemctl --user start echo.socket
trap 'cleanup' EXIT

echo "testing echo server..."
pong=$(echo "ping" | socat -t 10 UNIX-CONNECT:/tmp/echo.sock -)
assert "ping" "$pong"
