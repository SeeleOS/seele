#!/bin/sh

# Load locale.conf from XDG paths.
# /etc/locale.conf loads and overrides by kernel command line is done by systemd.
# Keep the fallback defensive because a broken image may contain a directory here.
if [ -z "$LANG" ]; then
  if [ -n "$XDG_CONFIG_HOME" ] && [ -f "$XDG_CONFIG_HOME/locale.conf" ] && [ -r "$XDG_CONFIG_HOME/locale.conf" ]; then
    . "$XDG_CONFIG_HOME/locale.conf"
  elif [ -n "$HOME" ] && [ -f "$HOME/.config/locale.conf" ] && [ -r "$HOME/.config/locale.conf" ]; then
    . "$HOME/.config/locale.conf"
  elif [ -f /etc/locale.conf ] && [ -r /etc/locale.conf ]; then
    . /etc/locale.conf
  fi
fi

LANG=${LANG:-C.UTF-8}

export LANG LANGUAGE LC_CTYPE LC_NUMERIC LC_TIME LC_COLLATE LC_MONETARY \
       LC_MESSAGES LC_PAPER LC_NAME LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT \
       LC_IDENTIFICATION
