# shellcheck shell=sh
append_xdg_data_dir() {
    case ":${XDG_DATA_DIRS:-}:" in
        *:"$1":*|*:"$1/":*) ;;
        *)
            if [ -n "${XDG_DATA_DIRS:-}" ]; then
                XDG_DATA_DIRS="${XDG_DATA_DIRS}:$1"
            else
                XDG_DATA_DIRS="$1"
            fi
            ;;
    esac
}

if [ -d "${XDG_DATA_HOME:-"$HOME/.local/share"}/flatpak/exports/share" ]; then
    append_xdg_data_dir "${XDG_DATA_HOME:-"$HOME/.local/share"}/flatpak/exports/share"
fi

if [ -d /var/lib/flatpak/exports/share ]; then
    append_xdg_data_dir /var/lib/flatpak/exports/share
fi

append_xdg_data_dir /usr/local/share
append_xdg_data_dir /usr/share
export XDG_DATA_DIRS

unset -f append_xdg_data_dir
