# shellcheck shell=sh
if [ -z "${DEBUGINFOD_URLS:-}" ]; then
    DEBUGINFOD_URLS=
    for url_file in /etc/debuginfod/*.urls; do
        [ -r "$url_file" ] || continue
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            DEBUGINFOD_URLS="${DEBUGINFOD_URLS}${DEBUGINFOD_URLS:+ }$line"
        done <"$url_file"
    done
    [ -n "$DEBUGINFOD_URLS" ] && export DEBUGINFOD_URLS || unset DEBUGINFOD_URLS
fi

if [ -z "${DEBUGINFOD_IMA_CERT_PATH:-}" ]; then
    DEBUGINFOD_IMA_CERT_PATH=
    for cert_file in /etc/debuginfod/*.certpath; do
        [ -r "$cert_file" ] || continue
        while IFS= read -r line; do
            [ -n "$line" ] || continue
            DEBUGINFOD_IMA_CERT_PATH="${DEBUGINFOD_IMA_CERT_PATH}${DEBUGINFOD_IMA_CERT_PATH:+:}$line"
        done <"$cert_file"
    done
    [ -n "$DEBUGINFOD_IMA_CERT_PATH" ] && export DEBUGINFOD_IMA_CERT_PATH || unset DEBUGINFOD_IMA_CERT_PATH
fi
