#!/usr/bin/env bash

set -euo pipefail

ps -efww | grep 'target/debug/seeleos-runner --agent\|qemu-system-x86_64.*seele-os-linux' | grep -v grep
