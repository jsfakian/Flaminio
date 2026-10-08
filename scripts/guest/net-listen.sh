#!/usr/bin/env bash
# Listens once on a TCP port and prints what arrives. Usage: net-listen.sh PORT TIMEOUT
timeout "$2" nc -l "$1" || true
