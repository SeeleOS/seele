use crate::object::{config::ConfigurateRequest, error::ObjectError, misc::ObjectResult};

use super::{buffer_handlers, client_handlers, display_handlers};

pub(super) fn handle_configure(request: ConfigurateRequest) -> ObjectResult<isize> {
    match request {
        ConfigurateRequest::DrmVersion(ptr) => client_handlers::handle_version(ptr),
        ConfigurateRequest::DrmGetUnique(ptr) => client_handlers::handle_get_unique(ptr),
        ConfigurateRequest::DrmGetMagic(ptr) => client_handlers::handle_get_magic(ptr),
        ConfigurateRequest::DrmGetCap(ptr) => client_handlers::handle_get_cap(ptr),
        ConfigurateRequest::DrmWaitVblank(ptr) => client_handlers::handle_wait_vblank(ptr),
        ConfigurateRequest::DrmSetUnique(ptr) => client_handlers::handle_set_unique(ptr),
        ConfigurateRequest::DrmAuthMagic(ptr) => client_handlers::handle_auth_magic(ptr),
        ConfigurateRequest::DrmSetClientCap(ptr) => client_handlers::handle_set_client_cap(ptr),
        ConfigurateRequest::DrmSetMaster => client_handlers::handle_set_master(),
        ConfigurateRequest::DrmDropMaster => client_handlers::handle_drop_master(),
        ConfigurateRequest::DrmModeGetResources(ptr) => {
            display_handlers::handle_mode_get_resources(ptr)
        }
        ConfigurateRequest::DrmModeGetCrtc(ptr) => display_handlers::handle_mode_get_crtc(ptr),
        ConfigurateRequest::DrmModeSetCrtc(ptr) => display_handlers::handle_mode_set_crtc(ptr),
        ConfigurateRequest::DrmModeGetGamma(ptr) => display_handlers::handle_mode_get_gamma(ptr),
        ConfigurateRequest::DrmModeSetGamma(ptr) => display_handlers::handle_mode_set_gamma(ptr),
        ConfigurateRequest::DrmModeGetEncoder(ptr) => {
            display_handlers::handle_mode_get_encoder(ptr)
        }
        ConfigurateRequest::DrmModeGetConnector(ptr) => {
            display_handlers::handle_mode_get_connector(ptr)
        }
        ConfigurateRequest::DrmModeGetProperty(ptr) => {
            display_handlers::handle_mode_get_property(ptr)
        }
        ConfigurateRequest::DrmModeObjGetProperties(ptr) => {
            display_handlers::handle_mode_obj_get_properties(ptr)
        }
        ConfigurateRequest::DrmModeGetPlaneResources(ptr) => {
            display_handlers::handle_mode_get_plane_resources(ptr)
        }
        ConfigurateRequest::DrmModeGetPlane(ptr) => display_handlers::handle_mode_get_plane(ptr),
        ConfigurateRequest::DrmModeListLessees(ptr) => {
            display_handlers::handle_mode_list_lessees(ptr)
        }
        ConfigurateRequest::DrmModeAddFb(ptr) => buffer_handlers::handle_mode_add_fb(ptr),
        ConfigurateRequest::DrmModeAddFb2(ptr) => buffer_handlers::handle_mode_add_fb2(ptr),
        ConfigurateRequest::DrmModeRemoveFb(ptr) => buffer_handlers::handle_mode_remove_fb(ptr),
        ConfigurateRequest::DrmModePageFlip(ptr) => buffer_handlers::handle_mode_page_flip(ptr),
        ConfigurateRequest::DrmModeDirtyFb(ptr) => buffer_handlers::handle_mode_dirty_fb(ptr),
        ConfigurateRequest::DrmModeCreateDumb(ptr) => buffer_handlers::handle_mode_create_dumb(ptr),
        ConfigurateRequest::DrmModeMapDumb(ptr) => buffer_handlers::handle_mode_map_dumb(ptr),
        ConfigurateRequest::DrmModeDestroyDumb(ptr) => {
            buffer_handlers::handle_mode_destroy_dumb(ptr)
        }
        ConfigurateRequest::DrmGemClose(ptr) => buffer_handlers::handle_gem_close(ptr),
        ConfigurateRequest::RawIoctl { request, arg } => {
            crate::s_println!("drm raw ioctl request={:#x} arg={:#x}", request, arg);
            Err(ObjectError::InvalidArguments)
        }
        _ => Err(ObjectError::InvalidArguments),
    }
}
