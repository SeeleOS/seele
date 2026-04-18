#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct LinuxInputId {
    pub(super) bustype: u16,
    pub(super) vendor: u16,
    pub(super) product: u16,
    pub(super) version: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventDeviceKind {
    Keyboard,
    Mouse,
}

const BUS_I8042: u16 = 0x11;

impl EventDeviceKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Keyboard => "AT Translated Set 2 keyboard",
            Self::Mouse => "PS/2 Generic Mouse",
        }
    }

    pub(super) fn phys(self) -> &'static str {
        match self {
            Self::Keyboard => "isa0060/serio0/input0",
            Self::Mouse => "isa0060/serio1/input0",
        }
    }

    pub(super) fn input_id(self) -> LinuxInputId {
        LinuxInputId {
            bustype: BUS_I8042,
            vendor: 0x0001,
            product: match self {
                Self::Keyboard => 0x0001,
                Self::Mouse => 0x0002,
            },
            version: 0x0100,
        }
    }

    pub(super) fn minor(self) -> u64 {
        match self {
            Self::Keyboard => 64,
            Self::Mouse => 65,
        }
    }
}
