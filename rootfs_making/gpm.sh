case "${TERM:-}" in
    linux)
        if [ -r /run/gpm.pid ] && [ -x /usr/bin/disable-paste ]; then
            /usr/bin/disable-paste
        fi
        ;;
esac
