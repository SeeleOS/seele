#!/bin/sh

set -eu

udev_data_dir=/run/udev/data
mkdir -p "$udev_data_dir"

write_record() {
    path="$1"
    shift
    cat >"${udev_data_dir}/${path}"
}

write_record c13:64 <<'EOF'
I:1
E:ID_INPUT=1
E:ID_INPUT_KEY=1
E:ID_INPUT_KEYBOARD=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-keyboard
V:1
EOF

write_record +input:input <<'EOF'
I:1
V:1
EOF

write_record +input:input0 <<'EOF'
I:1
E:ID_INPUT=1
E:ID_INPUT_KEY=1
E:ID_INPUT_KEYBOARD=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-keyboard
G:seat
Q:seat
V:1
EOF

write_record c13:65 <<'EOF'
I:1
E:ID_INPUT=1
E:ID_INPUT_MOUSE=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-mouse
V:1
EOF

write_record +input:input1 <<'EOF'
I:1
E:ID_INPUT=1
E:ID_INPUT_MOUSE=1
E:ID_SEAT=seat0
E:WL_SEAT=seat0
E:LIBINPUT_DEVICE_GROUP=seele-mouse
G:seat
Q:seat
V:1
EOF
