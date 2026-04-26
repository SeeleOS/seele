use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::{addrspace::AddrSpace, user_safe},
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, Statable},
    },
    process::misc::with_current_process,
};

use super::abi::{
    CARD0_RDEV, CONNECTOR0_ID, CRTC0_ID, DRIVER_DATE, DRIVER_DESC, DRIVER_NAME,
    DRM_CAP_DUMB_BUFFER, DRM_CAP_DUMB_PREFER_SHADOW, DRM_CAP_DUMB_PREFERRED_DEPTH,
    DRM_CAP_TIMESTAMP_MONOTONIC, DRM_CLIENT_CAP_ASPECT_RATIO, DRM_CLIENT_CAP_ATOMIC,
    DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, DRM_CLIENT_CAP_STEREO_3D, DRM_CLIENT_CAP_UNIVERSAL_PLANES,
    DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, DRM_MODE_CONNECTED, DRM_MODE_CONNECTOR_VIRTUAL,
    DRM_MODE_ENCODER_VIRTUAL, DRM_MODE_SUBPIXEL_UNKNOWN, ENCODER0_ID, current_framebuffer_info,
    current_mode_info,
};

#[derive(Default, Debug)]
pub struct DrmCardObject;

impl Object for DrmCardObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("statable", Statable);
}

impl Configuratable for DrmCardObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::DrmVersion(ptr) => {
                let mut version = read_user(ptr)?;
                version.version_major = 1;
                version.version_minor = 0;
                version.version_patchlevel = 0;
                copy_c_string(version.name, version.name_len, DRIVER_NAME)?;
                copy_c_string(version.date, version.date_len, DRIVER_DATE)?;
                copy_c_string(version.desc, version.desc_len, DRIVER_DESC)?;
                version.name_len = DRIVER_NAME.len();
                version.date_len = DRIVER_DATE.len();
                version.desc_len = DRIVER_DESC.len();
                user_safe::write(ptr, &version).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmGetCap(ptr) => {
                let mut cap = read_user(ptr)?;
                cap.value = match cap.capability {
                    DRM_CAP_DUMB_BUFFER => 1,
                    DRM_CAP_DUMB_PREFERRED_DEPTH => 32,
                    DRM_CAP_DUMB_PREFER_SHADOW => 0,
                    DRM_CAP_TIMESTAMP_MONOTONIC => 1,
                    _ => return Err(ObjectError::InvalidArguments),
                };
                user_safe::write(ptr, &cap).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmSetClientCap(ptr) => {
                let cap = read_user(ptr)?;
                match (cap.capability, cap.value) {
                    (DRM_CLIENT_CAP_STEREO_3D, 0 | 1)
                    | (DRM_CLIENT_CAP_UNIVERSAL_PLANES, 0 | 1)
                    | (DRM_CLIENT_CAP_ASPECT_RATIO, 0 | 1) => Ok(0),
                    (DRM_CLIENT_CAP_ATOMIC, 0)
                    | (DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, 0)
                    | (DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, 0) => Ok(0),
                    (DRM_CLIENT_CAP_ATOMIC, _)
                    | (DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, _)
                    | (DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, _) => Err(ObjectError::Unimplemented),
                    _ => Err(ObjectError::InvalidArguments),
                }
            }
            ConfigurateRequest::DrmSetMaster | ConfigurateRequest::DrmDropMaster => Ok(0),
            ConfigurateRequest::DrmModeGetResources(ptr) => {
                let mut resources = read_user(ptr)?;
                let fb = current_framebuffer_info();
                maybe_write_u32_slice(resources.crtc_id_ptr, resources.count_crtcs, &[CRTC0_ID])?;
                maybe_write_u32_slice(
                    resources.connector_id_ptr,
                    resources.count_connectors,
                    &[CONNECTOR0_ID],
                )?;
                maybe_write_u32_slice(
                    resources.encoder_id_ptr,
                    resources.count_encoders,
                    &[ENCODER0_ID],
                )?;
                maybe_write_u32_slice(resources.fb_id_ptr, resources.count_fbs, &[])?;
                resources.count_fbs = 0;
                resources.count_crtcs = 1;
                resources.count_connectors = 1;
                resources.count_encoders = 1;
                resources.min_width = 0;
                resources.max_width = u32::try_from(fb.width).unwrap_or(u32::MAX);
                resources.min_height = 0;
                resources.max_height = u32::try_from(fb.height).unwrap_or(u32::MAX);
                user_safe::write(ptr, &resources).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeGetCrtc(ptr) | ConfigurateRequest::DrmModeSetCrtc(ptr) => {
                let mut crtc = read_user(ptr)?;
                if crtc.crtc_id != 0 && crtc.crtc_id != CRTC0_ID {
                    return Err(ObjectError::InvalidArguments);
                }
                crtc.crtc_id = CRTC0_ID;
                crtc.x = 0;
                crtc.y = 0;
                crtc.gamma_size = 0;
                crtc.mode_valid = 1;
                crtc.mode = current_mode_info();
                user_safe::write(ptr, &crtc).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeGetEncoder(ptr) => {
                let mut encoder = read_user(ptr)?;
                if encoder.encoder_id != 0 && encoder.encoder_id != ENCODER0_ID {
                    return Err(ObjectError::InvalidArguments);
                }
                encoder.encoder_id = ENCODER0_ID;
                encoder.encoder_type = DRM_MODE_ENCODER_VIRTUAL;
                encoder.crtc_id = CRTC0_ID;
                encoder.possible_crtcs = 1;
                encoder.possible_clones = 0;
                user_safe::write(ptr, &encoder).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeGetConnector(ptr) => {
                let mut connector = read_user(ptr)?;
                if connector.connector_id != 0 && connector.connector_id != CONNECTOR0_ID {
                    return Err(ObjectError::InvalidArguments);
                }
                let mode = current_mode_info();
                maybe_write_u32_slice(
                    connector.encoders_ptr,
                    connector.count_encoders,
                    &[ENCODER0_ID],
                )?;
                maybe_write_struct_slice(connector.modes_ptr, connector.count_modes, &[mode])?;
                connector.count_props = 0;
                connector.count_encoders = 1;
                connector.count_modes = 1;
                connector.encoder_id = ENCODER0_ID;
                connector.connector_id = CONNECTOR0_ID;
                connector.connector_type = DRM_MODE_CONNECTOR_VIRTUAL;
                connector.connector_type_id = 1;
                connector.connection = DRM_MODE_CONNECTED;
                connector.mm_width = 0;
                connector.mm_height = 0;
                connector.subpixel = DRM_MODE_SUBPIXEL_UNKNOWN;
                connector.pad = 0;
                user_safe::write(ptr, &connector).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            _ => Err(ObjectError::InvalidArguments),
        }
    }
}

impl Statable for DrmCardObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o660, CARD0_RDEV)
    }
}

fn read_user<T: Copy>(ptr: *mut T) -> ObjectResult<T> {
    user_safe::read(ptr.cast_const()).map_err(|_| ObjectError::InvalidArguments)
}

fn maybe_write_u32_slice(ptr: u64, capacity: u32, values: &[u32]) -> ObjectResult<()> {
    maybe_write_struct_slice(ptr, capacity, values)
}

fn maybe_write_struct_slice<T: Copy>(ptr: u64, capacity: u32, values: &[T]) -> ObjectResult<()> {
    if values.is_empty() || ptr == 0 || capacity < values.len() as u32 {
        return Ok(());
    }

    with_current_process(|process| {
        process
            .addrspace
            .write(ptr as *mut T, values)
            .map_err(|_| ObjectError::InvalidArguments)
    })
}

fn copy_c_string(ptr: *mut u8, len: usize, value: &str) -> ObjectResult<()> {
    if ptr.is_null() || len == 0 {
        return Ok(());
    }

    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(len.saturating_sub(1));
    with_current_process(|process| {
        write_c_string(&mut process.addrspace, ptr, len, bytes, copy_len)
    })
}

fn write_c_string(
    addrspace: &mut AddrSpace,
    ptr: *mut u8,
    len: usize,
    bytes: &[u8],
    copy_len: usize,
) -> ObjectResult<()> {
    addrspace
        .write(ptr, &bytes[..copy_len])
        .map_err(|_| ObjectError::InvalidArguments)?;
    if len > copy_len {
        addrspace
            .write(unsafe { ptr.add(copy_len) }, &[0u8])
            .map_err(|_| ObjectError::InvalidArguments)?;
    }
    Ok(())
}
