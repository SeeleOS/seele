use alloc::vec::Vec;

use crate::{
    drm::{
        card::{CONNECTOR0_ID, CRTC0_ID, ENCODER0_ID, PLANE_TYPE_PROP_ID, PRIMARY_PLANE0_ID},
        mode::{
            DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_MODE_CONNECTED,
            DRM_MODE_CONNECTOR_VIRTUAL, DRM_MODE_ENCODER_VIRTUAL, DRM_MODE_OBJECT_CONNECTOR,
            DRM_MODE_OBJECT_CRTC, DRM_MODE_OBJECT_ENCODER, DRM_MODE_OBJECT_FB,
            DRM_MODE_OBJECT_PLANE, DRM_MODE_PROP_ENUM, DRM_MODE_PROP_IMMUTABLE,
            DRM_MODE_SUBPIXEL_UNKNOWN, DRM_PLANE_TYPE_CURSOR, DRM_PLANE_TYPE_OVERLAY,
            DRM_PLANE_TYPE_PRIMARY, current_framebuffer_info, current_mode_info,
        },
    },
    memory::user_safe,
    object::{error::ObjectError, misc::ObjectResult},
    process::misc::with_current_process,
};

use super::{
    framebuffer,
    object::DRM_STATE,
    user::{
        copy_property_name, make_property_enum, maybe_write_struct_slice, maybe_write_u32_slice,
        maybe_write_u64_slice, read_user,
    },
};

pub(super) fn handle_mode_get_resources(
    ptr: *mut crate::drm::mode_types::DrmModeCardRes,
) -> ObjectResult<isize> {
    let mut resources = read_user(ptr)?;
    let fb = current_framebuffer_info();
    let framebuffer_ids: Vec<u32> = DRM_STATE.lock().framebuffers.keys().copied().collect();
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
    maybe_write_u32_slice(resources.fb_id_ptr, resources.count_fbs, &framebuffer_ids)?;
    resources.count_fbs = framebuffer_ids.len() as u32;
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

pub(super) fn handle_mode_get_crtc(
    ptr: *mut crate::drm::mode_types::DrmModeCrtc,
) -> ObjectResult<isize> {
    let mut crtc = read_user(ptr)?;
    if crtc.crtc_id != 0 && crtc.crtc_id != CRTC0_ID {
        return Err(ObjectError::InvalidArguments);
    }
    crtc.crtc_id = CRTC0_ID;
    crtc.fb_id = DRM_STATE.lock().current_fb_id.unwrap_or(0);
    crtc.x = 0;
    crtc.y = 0;
    crtc.gamma_size = 0;
    crtc.mode_valid = 1;
    crtc.mode = current_mode_info();
    user_safe::write(ptr, &crtc).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_set_crtc(
    ptr: *mut crate::drm::mode_types::DrmModeCrtc,
) -> ObjectResult<isize> {
    let mut crtc = read_user(ptr)?;
    if crtc.crtc_id != 0 && crtc.crtc_id != CRTC0_ID {
        return Err(ObjectError::InvalidArguments);
    }

    if crtc.count_connectors > 1 {
        return Err(ObjectError::InvalidArguments);
    }
    if crtc.count_connectors == 1 {
        let connector = with_current_process(|process| {
            process
                .addrspace
                .read(crtc.set_connectors_ptr as *const u32)
                .map_err(|_| ObjectError::InvalidArguments)
        })?;
        if connector != CONNECTOR0_ID {
            return Err(ObjectError::InvalidArguments);
        }
    }

    if crtc.fb_id == 0 {
        DRM_STATE.lock().current_fb_id = None;
        crate::misc::framebuffer::framebuffer_set_user_controlled(false);
    } else {
        framebuffer::scanout_framebuffer_id(crtc.fb_id)?;
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

pub(super) fn handle_mode_get_encoder(
    ptr: *mut crate::drm::mode_types::DrmModeGetEncoder,
) -> ObjectResult<isize> {
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

pub(super) fn handle_mode_get_connector(
    ptr: *mut crate::drm::mode_types::DrmModeGetConnector,
) -> ObjectResult<isize> {
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

pub(super) fn handle_mode_get_property(
    ptr: *mut crate::drm::mode_types::DrmModeGetProperty,
) -> ObjectResult<isize> {
    let mut property = read_user(ptr)?;
    match property.prop_id {
        PLANE_TYPE_PROP_ID => {
            let enums = [
                make_property_enum(DRM_PLANE_TYPE_OVERLAY, "Overlay"),
                make_property_enum(DRM_PLANE_TYPE_PRIMARY, "Primary"),
                make_property_enum(DRM_PLANE_TYPE_CURSOR, "Cursor"),
            ];
            maybe_write_struct_slice(property.enum_blob_ptr, property.count_enum_blobs, &enums)?;
            property.flags = DRM_MODE_PROP_ENUM | DRM_MODE_PROP_IMMUTABLE;
            property.name = [0; 32];
            copy_property_name(&mut property.name, "type");
            property.count_values = 0;
            property.count_enum_blobs = enums.len() as u32;
        }
        _ => return Err(ObjectError::InvalidArguments),
    }
    user_safe::write(ptr, &property).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_obj_get_properties(
    ptr: *mut crate::drm::mode_types::DrmModeObjGetProperties,
) -> ObjectResult<isize> {
    let mut properties = read_user(ptr)?;
    match properties.obj_type {
        DRM_MODE_OBJECT_CRTC if properties.obj_id == CRTC0_ID => {}
        DRM_MODE_OBJECT_CONNECTOR if properties.obj_id == CONNECTOR0_ID => {}
        DRM_MODE_OBJECT_ENCODER if properties.obj_id == ENCODER0_ID => {}
        DRM_MODE_OBJECT_PLANE if properties.obj_id == PRIMARY_PLANE0_ID => {
            maybe_write_u32_slice(
                properties.props_ptr,
                properties.count_props,
                &[PLANE_TYPE_PROP_ID],
            )?;
            maybe_write_u64_slice(
                properties.prop_values_ptr,
                properties.count_props,
                &[DRM_PLANE_TYPE_PRIMARY],
            )?;
            properties.count_props = 1;
            user_safe::write(ptr, &properties).map_err(|_| ObjectError::InvalidArguments)?;
            return Ok(0);
        }
        DRM_MODE_OBJECT_FB
            if DRM_STATE
                .lock()
                .framebuffers
                .contains_key(&properties.obj_id) => {}
        _ => return Err(ObjectError::InvalidArguments),
    }
    properties.count_props = 0;
    user_safe::write(ptr, &properties).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_get_plane_resources(
    ptr: *mut crate::drm::mode_types::DrmModeGetPlaneRes,
) -> ObjectResult<isize> {
    let mut planes = read_user(ptr)?;
    maybe_write_u32_slice(
        planes.plane_id_ptr,
        planes.count_planes,
        &[PRIMARY_PLANE0_ID],
    )?;
    planes.count_planes = 1;
    user_safe::write(ptr, &planes).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}

pub(super) fn handle_mode_get_plane(
    ptr: *mut crate::drm::mode_types::DrmModeGetPlane,
) -> ObjectResult<isize> {
    let mut plane = read_user(ptr)?;
    if plane.plane_id != 0 && plane.plane_id != PRIMARY_PLANE0_ID {
        return Err(ObjectError::InvalidArguments);
    }
    let formats = [DRM_FORMAT_XRGB8888, DRM_FORMAT_ARGB8888];
    maybe_write_u32_slice(plane.format_type_ptr, plane.count_format_types, &formats)?;
    plane.plane_id = PRIMARY_PLANE0_ID;
    plane.crtc_id = CRTC0_ID;
    plane.fb_id = DRM_STATE.lock().current_fb_id.unwrap_or(0);
    plane.possible_crtcs = 1;
    plane.gamma_size = 0;
    plane.count_format_types = formats.len() as u32;
    user_safe::write(ptr, &plane).map_err(|_| ObjectError::InvalidArguments)?;
    Ok(0)
}
