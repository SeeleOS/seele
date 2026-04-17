#!/bin/sh

export PATH=/bin:/usr/bin
export TERM=xterm-256color
export HOME=/root

/bin/xinit
status=$?

echo "init: xinit exited with status ${status}"
exec /bin/bash
