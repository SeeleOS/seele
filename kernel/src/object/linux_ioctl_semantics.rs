use crate::object::linux_ioctl::{LinuxIoctlOp, LinuxIoctlTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxIoctlTestKind {
    Unit,
    Integration,
    CoverageGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxIoctlCoverage {
    pub target: LinuxIoctlTarget,
    pub op: LinuxIoctlOp,
    pub kind: LinuxIoctlTestKind,
    pub test: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxIoctlTargetSupport {
    pub target: LinuxIoctlTarget,
    pub ops: &'static [LinuxIoctlOp],
}

pub const KNOWN_LINUX_IOCTL_COVERAGE_GAPS: usize = 0;

macro_rules! linux_ioctl_targets {
    (
        $(
            $const_name:ident: $target:ident => $test:literal {
                $($op:ident,)*
            }
        )*
    ) => {
        $(
            pub const $const_name: &[LinuxIoctlOp] = &[
                $(LinuxIoctlOp::$op,)*
            ];
        )*

        pub const SUPPORTED_LINUX_IOCTL_ABI: &[LinuxIoctlTargetSupport] = &[
            $(
                LinuxIoctlTargetSupport {
                    target: LinuxIoctlTarget::$target,
                    ops: $const_name,
                },
            )*
        ];

        pub const LINUX_IOCTL_SEMANTICS_COVERAGE: &[LinuxIoctlCoverage] = &[
            $(
                $(
                    LinuxIoctlCoverage {
                        target: LinuxIoctlTarget::$target,
                        op: LinuxIoctlOp::$op,
                        kind: LinuxIoctlTestKind::Unit,
                        test: $test,
                    },
                )*
            )*
        ];
    };
}

linux_ioctl_targets! {
    FRAMEBUFFER_SUPPORTED_IOCTL_OPS: Framebuffer => "framebuffer_ioctls_follow_linux_rules" {
        FbGetVariableScreenInfo,
        FbPutVariableScreenInfo,
        FbGetFixedScreenInfo,
        FbGetColorMap,
        FbPutColorMap,
        FbPanDisplay,
        FbBlank,
    }
    TERMINAL_SUPPORTED_IOCTL_OPS: Terminal => "terminal_and_tty_ioctls_follow_linux_rules" {
        LinuxTcGets,
        LinuxTcSets,
        LinuxTcGets2,
        LinuxTcSets2,
        LinuxTiocgwinsz,
        LinuxTiocswinsz,
        LinuxKdGetKeyboardMode,
        LinuxKdSetKeyboardMode,
        LinuxKdGetKeyboardType,
        LinuxKdGetKeyboardEntry,
        LinuxKdGetDisplayMode,
        LinuxKdSetDisplayMode,
        LinuxKdSignalAccept,
        LinuxVtOpenQuery,
        LinuxVtGetMode,
        LinuxVtGetState,
        LinuxVtSetMode,
        LinuxVtActivate,
        LinuxVtWaitActive,
        LinuxVtRelDisp,
    }
    TTY_DEVICE_SUPPORTED_IOCTL_OPS: TtyDevice => "terminal_and_tty_ioctls_follow_linux_rules" {
        LinuxTcGets,
        LinuxTcSets,
        LinuxTcFlush,
        LinuxTcGets2,
        LinuxTcSets2,
        LinuxTiocnxcl,
        LinuxTiocsctty,
        LinuxTiocgPgrp,
        LinuxTiocnotty,
        LinuxTiocspgrp,
        LinuxTiocgwinsz,
        LinuxTiocswinsz,
        LinuxTiocvhangup,
        LinuxKdGetKeyboardMode,
        LinuxKdSetKeyboardMode,
        LinuxKdGetKeyboardType,
        LinuxKdGetKeyboardEntry,
        LinuxKdGetDisplayMode,
        LinuxKdSetDisplayMode,
        LinuxKdSignalAccept,
        LinuxVtOpenQuery,
        LinuxVtGetMode,
        LinuxVtGetState,
        LinuxVtSetMode,
        LinuxVtActivate,
        LinuxVtWaitActive,
        LinuxVtRelDisp,
    }
    PTY_SLAVE_SUPPORTED_IOCTL_OPS: PtySlave => "pty_ioctls_follow_linux_rules" {
        LinuxTcGets,
        LinuxTcSets,
        LinuxTcGets2,
        LinuxTcSets2,
        LinuxTiocsctty,
        LinuxTiocgPgrp,
        LinuxTiocnotty,
        LinuxTiocspgrp,
        LinuxTiocgwinsz,
        LinuxTiocswinsz,
        LinuxTiocvhangup,
        LinuxKdGetKeyboardMode,
        LinuxKdSetKeyboardMode,
        LinuxKdGetKeyboardType,
        LinuxKdGetKeyboardEntry,
        LinuxKdGetDisplayMode,
        LinuxKdSetDisplayMode,
        LinuxKdSignalAccept,
        LinuxVtOpenQuery,
        LinuxVtGetMode,
        LinuxVtGetState,
        LinuxVtSetMode,
        LinuxVtActivate,
        LinuxVtWaitActive,
        LinuxVtRelDisp,
    }
    PTY_MASTER_SUPPORTED_IOCTL_OPS: PtyMaster => "pty_ioctls_follow_linux_rules" {
        LinuxTcGets,
        LinuxTcSets,
        LinuxTcGets2,
        LinuxTcSets2,
        LinuxTiocsctty,
        LinuxTiocgPgrp,
        LinuxTiocnotty,
        LinuxTiocspgrp,
        LinuxTiocgwinsz,
        LinuxTiocswinsz,
        LinuxTiocgptn,
        LinuxTiocsptlck,
        LinuxTiocgptpeer,
        LinuxTiocvhangup,
        LinuxKdGetKeyboardMode,
        LinuxKdSetKeyboardMode,
        LinuxKdGetKeyboardType,
        LinuxKdGetKeyboardEntry,
        LinuxKdGetDisplayMode,
        LinuxKdSetDisplayMode,
        LinuxKdSignalAccept,
        LinuxVtOpenQuery,
        LinuxVtGetMode,
        LinuxVtGetState,
        LinuxVtSetMode,
        LinuxVtActivate,
        LinuxVtWaitActive,
        LinuxVtRelDisp,
    }
    DRM_CARD_SUPPORTED_IOCTL_OPS: DrmCard => "drm_card_ioctls_follow_linux_rules" {
        DrmVersion,
        DrmGetUnique,
        DrmGetMagic,
        DrmGetCap,
        DrmWaitVblank,
        DrmSetUnique,
        DrmAuthMagic,
        DrmSetClientCap,
        DrmSetMaster,
        DrmDropMaster,
        DrmModeGetResources,
        DrmModeGetCrtc,
        DrmModeSetCrtc,
        DrmModeCursor,
        DrmModeCursor2,
        DrmModeGetGamma,
        DrmModeSetGamma,
        DrmModeGetEncoder,
        DrmModeGetConnector,
        DrmModeGetProperty,
        DrmModeObjGetProperties,
        DrmModeGetPlaneResources,
        DrmModeGetPlane,
        DrmModeListLessees,
        DrmModeAddFb,
        DrmModeAddFb2,
        DrmModeRemoveFb,
        DrmModePageFlip,
        DrmModeDirtyFb,
        DrmModeCreateDumb,
        DrmModeMapDumb,
        DrmModeDestroyDumb,
        DrmGemClose,
        DrmPrimeHandleToFd,
        DrmPrimeFdToHandle,
    }
    DRM_PRIME_SUPPORTED_IOCTL_OPS: DrmPrime => "drm_prime_ioctls_follow_linux_rules" {
        DmaBufSync,
        DmaBufExportSyncFile,
        DmaBufImportSyncFile,
    }
    EVDEV_SUPPORTED_IOCTL_OPS: EvdevClient => "evdev_ioctls_follow_linux_rules" {
        EvdevGetVersion,
        EvdevGetId,
        EvdevGetRepeat,
        EvdevGetName,
        EvdevGetPhys,
        EvdevGetUniq,
        EvdevGetProp,
        EvdevGetKey,
        EvdevGetLed,
        EvdevGetSnd,
        EvdevGetSw,
        EvdevGetBit,
        EvdevGrab,
        EvdevRevoke,
        EvdevSetClockId,
    }
    UNIX_SOCKET_SUPPORTED_IOCTL_OPS: UnixSocket => "socket_and_netlink_ioctls_follow_linux_rules" {
        RawFioclex,
        RawFionbio,
        LinuxTiocoutq,
    }
    INET_SOCKET_SUPPORTED_IOCTL_OPS: InetSocket => "socket_and_netlink_ioctls_follow_linux_rules" {
        RawFioclex,
        RawFionbio,
        LinuxTiocoutq,
    }
    NETLINK_SOCKET_SUPPORTED_IOCTL_OPS: NetlinkSocket => "socket_and_netlink_ioctls_follow_linux_rules" {
        RawFioclex,
        RawFionbio,
        LinuxTiocoutq,
    }
}
