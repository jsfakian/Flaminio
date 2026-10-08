#!/usr/bin/env bash
# Sends one line to HOST:PORT. Usage: net-send.sh HOST PORT MESSAGE
echo "$3" | nc -N -w 5 "$1" "$2"
