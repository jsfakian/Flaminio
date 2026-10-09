#!/usr/bin/env bash
# Builds fio as a static binary (FIO_VERSION, a tag of github.com/axboe/fio), for a container
# image with nothing else in it. Prints the binary's path. Built once per version.
v=${FIO_VERSION:?}
out="$HOME/fio-static/$v/fio"
if [[ ! -x $out ]]; then
    log "building $v statically"
    src=$(mktemp -d)
    git -c advice.detachedHead=false clone -q --depth 1 -b "$v" https://github.com/axboe/fio "$src"
    (cd "$src" && ./configure --build-static --disable-native >/dev/null && make -j"$(nproc)" >/dev/null 2>&1)
    mkdir -p "$(dirname "$out")"
    cp "$src/fio" "$out"
    git -C "$src" rev-parse HEAD >"$(dirname "$out")/commit"
    rm -rf "$src"
fi
log "$v at commit $(cat "$(dirname "$out")/commit"): $(file -b "$out" | cut -d, -f1-2)"
echo "$out"
