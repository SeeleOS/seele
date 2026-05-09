use alloc::vec::Vec;

use crate::drm::mode::{
    DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_MODE_CURSOR_BO, DRM_MODE_CURSOR_FLAGS,
    DRM_MODE_CURSOR_MOVE, DRM_MODE_FB_DIRTY_ANNOTATE_COPY, DRM_MODE_FB_DIRTY_ANNOTATE_FILL,
    DRM_MODE_FB_DIRTY_FLAGS, DRM_MODE_PAGE_FLIP_ASYNC, DRM_MODE_PAGE_FLIP_EVENT,
    DRM_MODE_PAGE_FLIP_TARGET, DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE,
    DRM_MODE_PAGE_FLIP_TARGET_RELATIVE, MODE_GAMMA_LUT_SIZE, MODE_REFRESH_HZ,
};
use crate::{
    drm::{
        card::{CONNECTOR0_ID, CRTC0_ID, ENCODER0_ID, PLANE_TYPE_PROP_ID, PRIMARY_PLANE0_ID},
        client::{
            DRM_CAP_CURSOR_HEIGHT, DRM_CAP_DUMB_BUFFER, DRM_CAP_PRIME, DRM_CAP_TIMESTAMP_MONOTONIC,
            DRM_CLIENT_CAP_ATOMIC, DRM_CLIENT_CAP_UNIVERSAL_PLANES, DRM_EVENT_FLIP_COMPLETE,
            DRM_EVENT_VBLANK, DRM_IOCTL_PRIME_FD_TO_HANDLE, DRM_IOCTL_PRIME_HANDLE_TO_FD,
            DRM_PRIME_CAP_EXPORT, DRM_PRIME_CAP_IMPORT, DRM_VBLANK_EVENT, DRM_VBLANK_FLIP, DrmAuth,
            DrmEventVblank, DrmGetCap, DrmPrimeHandle, DrmSetClientCap, DrmUnique, DrmVersion,
            DrmWaitVblank, DrmWaitVblankRequest,
        },
        mode::{
            DRM_MODE_CONNECTED, DRM_MODE_CONNECTOR_VIRTUAL, DRM_MODE_ENCODER_VIRTUAL,
            DRM_MODE_OBJECT_FB, DRM_MODE_OBJECT_PLANE, DRM_MODE_PROP_ENUM, DRM_MODE_PROP_IMMUTABLE,
        },
        mode_types::{
            DrmModeCardRes, DrmModeCreateDumb, DrmModeCrtc, DrmModeCrtcLut, DrmModeCrtcPageFlip,
            DrmModeCursor, DrmModeCursor2, DrmModeDestroyDumb, DrmModeFbCmd, DrmModeFbCmd2,
            DrmModeFbDirtyCmd, DrmModeGetConnector, DrmModeGetEncoder, DrmModeGetPlane,
            DrmModeGetPlaneRes, DrmModeGetProperty, DrmModeListLessees, DrmModeMapDumb,
            DrmModeObjGetProperties, DrmModePropertyEnum,
        },
        object::{DRM_EVENT_QUEUE, DRM_STATE, DrmCardObject},
    },
    misc::framebuffer::FRAME_BUFFER,
    object::{
        config::ConfigurateRequest,
        error::ObjectError,
        linux_ioctl::{DMABUF_IOCTL_TYPE, ioctl_request},
        traits::Configuratable,
    },
    process::manager::get_current_process,
};

crate::test!(
    drm_mode_defaults,
    "drm fourcc and mode defaults match linux values",
    drm_fourcc_and_mode_defaults_match_linux_values
);
crate::test!(
    drm_flag_groups,
    "drm grouped flags are exact unions",
    drm_grouped_flags_are_exact_unions
);
crate::test!(
    drm_card_ioctl_semantics,
    "drm card ioctls follow linux rules",
    drm_card_ioctls_follow_linux_rules
);

fn drm_fourcc_and_mode_defaults_match_linux_values() {
    assert_eq!(DRM_FORMAT_XRGB8888, 0x3432_5258);
    assert_eq!(DRM_FORMAT_ARGB8888, 0x3432_5241);
    assert_eq!(MODE_REFRESH_HZ, 60);
    assert_eq!(MODE_GAMMA_LUT_SIZE, 256);
}

fn drm_grouped_flags_are_exact_unions() {
    assert_eq!(
        DRM_MODE_FB_DIRTY_FLAGS,
        DRM_MODE_FB_DIRTY_ANNOTATE_COPY | DRM_MODE_FB_DIRTY_ANNOTATE_FILL
    );
    assert_eq!(
        DRM_MODE_PAGE_FLIP_TARGET,
        DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE | DRM_MODE_PAGE_FLIP_TARGET_RELATIVE
    );
    assert_eq!(
        DRM_MODE_CURSOR_FLAGS,
        DRM_MODE_CURSOR_BO | DRM_MODE_CURSOR_MOVE
    );
    assert_ne!(DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_ASYNC);
}

fn drm_card_ioctls_follow_linux_rules() {
    DRM_EVENT_QUEUE.lock().clear();
    let card = DrmCardObject::default();
    let fb = FRAME_BUFFER.get().unwrap().lock().fb_info();

    let mut version = DrmVersion {
        name_len: 32,
        name: [0u8; 32].as_mut_ptr(),
        date_len: 32,
        date: [0u8; 32].as_mut_ptr(),
        desc_len: 64,
        desc: [0u8; 64].as_mut_ptr(),
        ..Default::default()
    };
    let mut name = [0u8; 32];
    let mut date = [0u8; 32];
    let mut desc = [0u8; 64];
    version.name = name.as_mut_ptr();
    version.date = date.as_mut_ptr();
    version.desc = desc.as_mut_ptr();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmVersion(&mut version))
            .unwrap(),
        0
    );
    assert_eq!(version.version_major, 1);
    assert_eq!(version.name_len, 10);
    assert_eq!(&name[..10], b"kms_swrast");

    let mut unique = DrmUnique::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmGetUnique(&mut unique))
            .unwrap(),
        0
    );
    assert_eq!(unique.unique_len, 0);

    let mut auth = DrmAuth::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmGetMagic(&mut auth))
            .unwrap(),
        0
    );
    assert_eq!(auth.magic, 1);

    for (capability, expected) in [
        (DRM_CAP_DUMB_BUFFER, 1),
        (DRM_CAP_PRIME, DRM_PRIME_CAP_EXPORT | DRM_PRIME_CAP_IMPORT),
        (DRM_CAP_TIMESTAMP_MONOTONIC, 1),
        (DRM_CAP_CURSOR_HEIGHT, 64),
    ] {
        let mut cap = DrmGetCap {
            capability,
            value: 0,
        };
        assert_eq!(
            card.configure(ConfigurateRequest::DrmGetCap(&mut cap))
                .unwrap(),
            0
        );
        assert_eq!(cap.value, expected);
    }
    let mut bad_cap = DrmGetCap {
        capability: 0xffff,
        value: 0,
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmGetCap(&mut bad_cap)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut vblank = DrmWaitVblank::default();
    vblank.request = DrmWaitVblankRequest {
        type_: DRM_VBLANK_EVENT,
        sequence: 9,
        signal: 0x1234,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmWaitVblank(&mut vblank))
            .unwrap(),
        0
    );
    let reply = unsafe { vblank.reply };
    assert_eq!(reply.sequence, 9);
    let event = pop_drm_event();
    assert_eq!(event.base.type_, DRM_EVENT_VBLANK);
    assert_eq!(event.user_data, 0x1234);
    assert_eq!(event.crtc_id, CRTC0_ID);

    vblank.request = DrmWaitVblankRequest {
        type_: DRM_VBLANK_FLIP,
        sequence: 0,
        signal: 0,
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmWaitVblank(&mut vblank)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut set_unique = DrmUnique::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmSetUnique(&mut set_unique))
            .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmAuthMagic(&mut auth))
            .unwrap(),
        0
    );

    let mut client_cap = DrmSetClientCap {
        capability: DRM_CLIENT_CAP_UNIVERSAL_PLANES,
        value: 1,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmSetClientCap(&mut client_cap))
            .unwrap(),
        0
    );
    client_cap = DrmSetClientCap {
        capability: DRM_CLIENT_CAP_ATOMIC,
        value: 1,
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmSetClientCap(&mut client_cap)),
        Err(ObjectError::Unimplemented)
    ));
    assert_eq!(card.configure(ConfigurateRequest::DrmSetMaster).unwrap(), 0);
    assert_eq!(
        card.configure(ConfigurateRequest::DrmDropMaster).unwrap(),
        0
    );

    let mut resources = DrmModeCardRes::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetResources(&mut resources))
            .unwrap(),
        0
    );
    assert_eq!(resources.count_crtcs, 1);
    assert_eq!(resources.count_connectors, 1);
    assert_eq!(resources.max_width, fb.width as u32);
    assert_eq!(resources.max_height, fb.height as u32);

    let mut resources_ids = DrmModeCardRes {
        fb_id_ptr: [0u32; 2].as_mut_ptr() as u64,
        crtc_id_ptr: [0u32; 1].as_mut_ptr() as u64,
        connector_id_ptr: [0u32; 1].as_mut_ptr() as u64,
        encoder_id_ptr: [0u32; 1].as_mut_ptr() as u64,
        count_fbs: 2,
        count_crtcs: 1,
        count_connectors: 1,
        count_encoders: 1,
        ..Default::default()
    };
    let mut fb_ids = [0u32; 2];
    let mut crtc_ids = [0u32; 1];
    let mut connector_ids = [0u32; 1];
    let mut encoder_ids = [0u32; 1];
    resources_ids.fb_id_ptr = fb_ids.as_mut_ptr() as u64;
    resources_ids.crtc_id_ptr = crtc_ids.as_mut_ptr() as u64;
    resources_ids.connector_id_ptr = connector_ids.as_mut_ptr() as u64;
    resources_ids.encoder_id_ptr = encoder_ids.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetResources(&mut resources_ids))
            .unwrap(),
        0
    );
    assert_eq!(crtc_ids[0], CRTC0_ID);
    assert_eq!(connector_ids[0], CONNECTOR0_ID);
    assert_eq!(encoder_ids[0], ENCODER0_ID);
    assert_ne!(fb_ids[0], 0);

    let mut crtc = DrmModeCrtc::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetCrtc(&mut crtc))
            .unwrap(),
        0
    );
    assert_eq!(crtc.crtc_id, CRTC0_ID);
    assert_eq!(crtc.gamma_size, MODE_GAMMA_LUT_SIZE);
    assert_eq!(crtc.mode.hdisplay, fb.width as u16);

    let mut gamma = DrmModeCrtcLut {
        red: [0u16; 256].as_mut_ptr() as u64,
        green: [0u16; 256].as_mut_ptr() as u64,
        blue: [0u16; 256].as_mut_ptr() as u64,
        gamma_size: MODE_GAMMA_LUT_SIZE,
        ..Default::default()
    };
    let mut red = [0u16; 256];
    let mut green = [0u16; 256];
    let mut blue = [0u16; 256];
    gamma.red = red.as_mut_ptr() as u64;
    gamma.green = green.as_mut_ptr() as u64;
    gamma.blue = blue.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetGamma(&mut gamma))
            .unwrap(),
        0
    );
    assert_eq!(gamma.gamma_size, MODE_GAMMA_LUT_SIZE);
    assert_eq!(red[0], 0);
    assert_eq!(red[255], u16::MAX);
    gamma.gamma_size = MODE_GAMMA_LUT_SIZE + 1;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeSetGamma(&mut gamma)),
        Err(ObjectError::InvalidArguments)
    ));
    gamma.gamma_size = MODE_GAMMA_LUT_SIZE;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeSetGamma(&mut gamma))
            .unwrap(),
        0
    );

    let mut encoder = DrmModeGetEncoder::default();
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetEncoder(&mut encoder))
            .unwrap(),
        0
    );
    assert_eq!(encoder.encoder_id, ENCODER0_ID);
    assert_eq!(encoder.encoder_type, DRM_MODE_ENCODER_VIRTUAL);

    let mut connector = DrmModeGetConnector {
        count_modes: 1,
        modes_ptr: [crate::drm::mode_types::DrmModeModeInfo::default(); 1].as_mut_ptr() as u64,
        count_encoders: 1,
        encoders_ptr: [0u32; 1].as_mut_ptr() as u64,
        ..Default::default()
    };
    let mut modes = [crate::drm::mode_types::DrmModeModeInfo::default(); 1];
    let mut encoders = [0u32; 1];
    connector.modes_ptr = modes.as_mut_ptr() as u64;
    connector.encoders_ptr = encoders.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetConnector(&mut connector))
            .unwrap(),
        0
    );
    assert_eq!(connector.connector_id, CONNECTOR0_ID);
    assert_eq!(connector.connection, DRM_MODE_CONNECTED);
    assert_eq!(connector.connector_type, DRM_MODE_CONNECTOR_VIRTUAL);
    assert_eq!(encoders[0], ENCODER0_ID);

    let mut property = DrmModeGetProperty {
        prop_id: PLANE_TYPE_PROP_ID,
        count_enum_blobs: 3,
        enum_blob_ptr: [DrmModePropertyEnum::default(); 3].as_mut_ptr() as u64,
        ..Default::default()
    };
    let mut enums = [DrmModePropertyEnum::default(); 3];
    property.enum_blob_ptr = enums.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetProperty(&mut property))
            .unwrap(),
        0
    );
    assert_eq!(property.flags, DRM_MODE_PROP_ENUM | DRM_MODE_PROP_IMMUTABLE);
    assert_eq!(property.count_enum_blobs, 3);
    assert_eq!(&property.name[..4], b"type");

    let mut obj_props = DrmModeObjGetProperties {
        obj_id: PRIMARY_PLANE0_ID,
        obj_type: DRM_MODE_OBJECT_PLANE,
        count_props: 1,
        props_ptr: [0u32; 1].as_mut_ptr() as u64,
        prop_values_ptr: [0u64; 1].as_mut_ptr() as u64,
    };
    let mut prop_ids = [0u32; 1];
    let mut prop_values = [0u64; 1];
    obj_props.props_ptr = prop_ids.as_mut_ptr() as u64;
    obj_props.prop_values_ptr = prop_values.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeObjGetProperties(&mut obj_props))
            .unwrap(),
        0
    );
    assert_eq!(obj_props.count_props, 1);
    assert_eq!(prop_ids[0], PLANE_TYPE_PROP_ID);
    assert_eq!(prop_values[0], 1);

    let mut planes = DrmModeGetPlaneRes {
        count_planes: 1,
        plane_id_ptr: [0u32; 1].as_mut_ptr() as u64,
    };
    let mut plane_ids = [0u32; 1];
    planes.plane_id_ptr = plane_ids.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetPlaneResources(&mut planes))
            .unwrap(),
        0
    );
    assert_eq!(planes.count_planes, 1);
    assert_eq!(plane_ids[0], PRIMARY_PLANE0_ID);

    let mut plane = DrmModeGetPlane {
        count_format_types: 2,
        format_type_ptr: [0u32; 2].as_mut_ptr() as u64,
        ..Default::default()
    };
    let mut formats = [0u32; 2];
    plane.format_type_ptr = formats.as_mut_ptr() as u64;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeGetPlane(&mut plane))
            .unwrap(),
        0
    );
    assert_eq!(plane.plane_id, PRIMARY_PLANE0_ID);
    assert_eq!(plane.crtc_id, CRTC0_ID);
    assert_eq!(formats, [DRM_FORMAT_XRGB8888, DRM_FORMAT_ARGB8888]);

    let mut lessees = DrmModeListLessees {
        count_lessees: 7,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeListLessees(&mut lessees))
            .unwrap(),
        0
    );
    assert_eq!(lessees.count_lessees, 0);

    let mut create = DrmModeCreateDumb {
        width: 64,
        height: 64,
        bpp: 32,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCreateDumb(&mut create))
            .unwrap(),
        0
    );
    assert_ne!(create.handle, 0);
    assert_eq!(create.pitch, 256);
    assert_eq!(create.size, 16384);

    let mut map = DrmModeMapDumb {
        handle: create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeMapDumb(&mut map))
            .unwrap(),
        0
    );
    assert_ne!(map.offset, 0);
    write_dumb_xrgb_pixel(create.handle, 0, 0, [0xab, 0xcd, 0xef, 0x01]);

    let mut addfb = DrmModeFbCmd {
        width: 64,
        height: 64,
        pitch: create.pitch,
        bpp: 32,
        depth: 24,
        handle: create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb(&mut addfb))
            .unwrap(),
        0
    );
    assert_ne!(addfb.fb_id, 0);

    let mut addfb2 = DrmModeFbCmd2 {
        width: 64,
        height: 64,
        pixel_format: DRM_FORMAT_XRGB8888,
        handles: [create.handle, 0, 0, 0],
        pitches: [create.pitch, 0, 0, 0],
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb2(&mut addfb2))
            .unwrap(),
        0
    );
    assert_ne!(addfb2.fb_id, 0);

    let mut bad_addfb2 = addfb2;
    bad_addfb2.flags = 0xffff;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeAddFb2(&mut bad_addfb2)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut set_crtc = DrmModeCrtc {
        crtc_id: CRTC0_ID,
        fb_id: addfb.fb_id,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeSetCrtc(&mut set_crtc))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0xab, 0xcd, 0xef, 0xff]);

    write_dumb_xrgb_pixel(create.handle, 0, 0, [0x44, 0x55, 0x66, 0x77]);
    let mut dirty = DrmModeFbDirtyCmd {
        fb_id: addfb.fb_id,
        flags: DRM_MODE_FB_DIRTY_ANNOTATE_COPY,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDirtyFb(&mut dirty))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0x44, 0x55, 0x66, 0xff]);
    dirty.flags = 0x8000_0000;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeDirtyFb(&mut dirty)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut flip_create = DrmModeCreateDumb {
        width: 64,
        height: 64,
        bpp: 32,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCreateDumb(&mut flip_create))
            .unwrap(),
        0
    );
    write_dumb_xrgb_pixel(flip_create.handle, 0, 0, [0x88, 0x99, 0xaa, 0xbb]);
    let mut flip_fb = DrmModeFbCmd {
        width: 64,
        height: 64,
        pitch: flip_create.pitch,
        bpp: 32,
        depth: 24,
        handle: flip_create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb(&mut flip_fb))
            .unwrap(),
        0
    );
    let mut page_flip = DrmModeCrtcPageFlip {
        crtc_id: CRTC0_ID,
        fb_id: flip_fb.fb_id,
        flags: DRM_MODE_PAGE_FLIP_EVENT,
        user_data: 0xfeed_face,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModePageFlip(&mut page_flip))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0x88, 0x99, 0xaa, 0xff]);
    let flip_event = pop_drm_event();
    assert_eq!(flip_event.base.type_, DRM_EVENT_FLIP_COMPLETE);
    assert_eq!(flip_event.user_data, 0xfeed_face);
    page_flip.flags = DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModePageFlip(&mut page_flip)),
        Err(ObjectError::Unimplemented)
    ));

    let mut fullscreen_create = DrmModeCreateDumb {
        width: fb.width as u32,
        height: fb.height as u32,
        bpp: 32,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCreateDumb(
            &mut fullscreen_create
        ))
        .unwrap(),
        0
    );
    assert!(
        !DRM_STATE
            .lock()
            .dumb_buffers
            .get(&fullscreen_create.handle)
            .unwrap()
            .scanout_backed
    );
    write_dumb_xrgb_pixel(fullscreen_create.handle, 0, 0, [0x10, 0x20, 0x30, 0x40]);
    write_dumb_xrgb_pixel(
        fullscreen_create.handle,
        fb.width - 1,
        fb.height - 1,
        [0x50, 0x60, 0x70, 0x80],
    );
    let mut fullscreen_fb = DrmModeFbCmd {
        width: fullscreen_create.width,
        height: fullscreen_create.height,
        pitch: fullscreen_create.pitch,
        bpp: 32,
        depth: 24,
        handle: fullscreen_create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb(&mut fullscreen_fb))
            .unwrap(),
        0
    );
    let mut fullscreen_set_crtc = DrmModeCrtc {
        crtc_id: CRTC0_ID,
        fb_id: fullscreen_fb.fb_id,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeSetCrtc(&mut fullscreen_set_crtc))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0x10, 0x20, 0x30, 0xff]);
    assert_visible_bgr_pixel(fb.width - 1, fb.height - 1, [0x50, 0x60, 0x70, 0xff]);

    write_dumb_xrgb_pixel(fullscreen_create.handle, 0, 0, [0x90, 0xa0, 0xb0, 0xc0]);
    let mut fullscreen_dirty = DrmModeFbDirtyCmd {
        fb_id: fullscreen_fb.fb_id,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDirtyFb(&mut fullscreen_dirty))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0x90, 0xa0, 0xb0, 0xff]);

    let mut fullscreen_flip_create = DrmModeCreateDumb {
        width: fb.width as u32,
        height: fb.height as u32,
        bpp: 32,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCreateDumb(
            &mut fullscreen_flip_create
        ))
        .unwrap(),
        0
    );
    assert!(
        !DRM_STATE
            .lock()
            .dumb_buffers
            .get(&fullscreen_flip_create.handle)
            .unwrap()
            .scanout_backed
    );
    write_dumb_xrgb_pixel(
        fullscreen_flip_create.handle,
        fb.width - 1,
        fb.height - 1,
        [0xd0, 0xe0, 0xf0, 0x12],
    );
    let mut fullscreen_flip_fb = DrmModeFbCmd {
        width: fullscreen_flip_create.width,
        height: fullscreen_flip_create.height,
        pitch: fullscreen_flip_create.pitch,
        bpp: 32,
        depth: 24,
        handle: fullscreen_flip_create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb(&mut fullscreen_flip_fb))
            .unwrap(),
        0
    );
    let mut fullscreen_page_flip = DrmModeCrtcPageFlip {
        crtc_id: CRTC0_ID,
        fb_id: fullscreen_flip_fb.fb_id,
        flags: DRM_MODE_PAGE_FLIP_EVENT,
        user_data: 0xcafe_babe,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModePageFlip(
            &mut fullscreen_page_flip
        ))
        .unwrap(),
        0
    );
    assert_visible_bgr_pixel(fb.width - 1, fb.height - 1, [0xd0, 0xe0, 0xf0, 0xff]);
    let fullscreen_flip_event = pop_drm_event();
    assert_eq!(fullscreen_flip_event.base.type_, DRM_EVENT_FLIP_COMPLETE);
    assert_eq!(fullscreen_flip_event.user_data, 0xcafe_babe);

    let mut offset_create = DrmModeCreateDumb {
        width: 66,
        height: 66,
        bpp: 32,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCreateDumb(&mut offset_create))
            .unwrap(),
        0
    );
    write_dumb_xrgb_at_offset(offset_create.handle, 4, [0x21, 0x32, 0x43, 0x54]);
    write_dumb_xrgb_at_offset(
        offset_create.handle,
        4 + (63 * offset_create.pitch as usize) + (63 * 4),
        [0x65, 0x76, 0x87, 0x98],
    );
    let mut offset_fb = DrmModeFbCmd2 {
        width: 64,
        height: 64,
        pixel_format: DRM_FORMAT_XRGB8888,
        handles: [offset_create.handle, 0, 0, 0],
        pitches: [offset_create.pitch, 0, 0, 0],
        offsets: [4, 0, 0, 0],
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeAddFb2(&mut offset_fb))
            .unwrap(),
        0
    );
    let mut offset_set_crtc = DrmModeCrtc {
        crtc_id: CRTC0_ID,
        fb_id: offset_fb.fb_id,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeSetCrtc(&mut offset_set_crtc))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(0, 0, [0x21, 0x32, 0x43, 0xff]);
    assert_visible_bgr_pixel(63, 63, [0x65, 0x76, 0x87, 0xff]);

    let mut cursor = DrmModeCursor {
        flags: DRM_MODE_CURSOR_BO,
        crtc_id: CRTC0_ID,
        width: 32,
        height: 32,
        handle: create.handle,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCursor(&mut cursor))
            .unwrap(),
        0
    );
    let mut cursor2 = DrmModeCursor2 {
        flags: DRM_MODE_CURSOR_MOVE,
        crtc_id: CRTC0_ID,
        x: 4,
        y: 5,
        hot_x: 1,
        hot_y: 2,
        ..Default::default()
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCursor2(&mut cursor2))
            .unwrap(),
        0
    );
    let current_cursor = DRM_STATE.lock().cursor.clone().unwrap();
    assert_eq!(current_cursor.x, 4);
    assert_eq!(current_cursor.y, 5);
    assert_eq!(current_cursor.hot_x, 0);
    assert_eq!(current_cursor.hot_y, 0);
    write_dumb_xrgb_pixel(flip_create.handle, 4, 5, [0x00, 0x00, 0x20, 0xff]);
    write_dumb_xrgb_pixel(flip_create.handle, 6, 7, [0x00, 0x30, 0x00, 0xff]);
    write_dumb_xrgb_pixel(create.handle, 0, 0, [0xff, 0xff, 0xff, 0x80]);
    page_flip.fb_id = flip_fb.fb_id;
    page_flip.flags = 0;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModePageFlip(&mut page_flip))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(4, 5, [0x80, 0x80, 0x8f, 0xff]);
    cursor2.x = 6;
    cursor2.y = 7;
    cursor2.hot_x = 99;
    cursor2.hot_y = 99;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeCursor2(&mut cursor2))
            .unwrap(),
        0
    );
    assert_visible_bgr_pixel(4, 5, [0x00, 0x00, 0x20, 0xff]);
    assert_visible_bgr_pixel(6, 7, [0x80, 0x97, 0x80, 0xff]);
    cursor2.hot_x = -1;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeCursor2(&mut cursor2)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut prime_export = DrmPrimeHandle {
        handle: create.handle,
        flags: 0x0008_0000,
        fd: -1,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmPrimeHandleToFd(&mut prime_export))
            .unwrap(),
        0
    );
    assert!(prime_export.fd >= 0);

    let mut prime_import = DrmPrimeHandle {
        handle: 0,
        flags: 0,
        fd: prime_export.fd,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmPrimeFdToHandle(&mut prime_import))
            .unwrap(),
        0
    );
    assert_ne!(prime_import.handle, 0);

    let mut bad_prime_import = DrmPrimeHandle {
        handle: 0,
        flags: 1,
        fd: prime_export.fd,
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmPrimeFdToHandle(
            &mut bad_prime_import
        )),
        Err(ObjectError::InvalidArguments)
    ));

    let prime_object = get_current_process()
        .lock()
        .get_object(prime_export.fd as u64)
        .unwrap();
    let prime = prime_object.as_drm_prime_buffer().unwrap();

    let mut sync = [0u64; 1];
    sync[0] = 1;
    assert_eq!(
        prime
            .configure(ConfigurateRequest::RawIoctl {
                request: ioctl_request(0, DMABUF_IOCTL_TYPE, 0, 8),
                arg: sync.as_mut_ptr() as u64,
            })
            .unwrap(),
        0
    );
    sync[0] = 0;
    assert!(matches!(
        prime.configure(ConfigurateRequest::RawIoctl {
            request: ioctl_request(0, DMABUF_IOCTL_TYPE, 0, 8),
            arg: sync.as_mut_ptr() as u64,
        }),
        Err(ObjectError::InvalidArguments)
    ));

    let mut export_sync = [1u32, u32::MAX];
    assert_eq!(
        prime
            .configure(ConfigurateRequest::RawIoctl {
                request: ioctl_request(0, DMABUF_IOCTL_TYPE, 2, 8),
                arg: export_sync.as_mut_ptr() as u64,
            })
            .unwrap(),
        0
    );
    assert_ne!(export_sync[1] as i32, -1);
    let mut import_sync = [1u32, export_sync[1]];
    assert_eq!(
        prime
            .configure(ConfigurateRequest::RawIoctl {
                request: ioctl_request(0, DMABUF_IOCTL_TYPE, 3, 8),
                arg: import_sync.as_mut_ptr() as u64,
            })
            .unwrap(),
        0
    );
    import_sync[1] = u32::MAX;
    assert!(matches!(
        prime.configure(ConfigurateRequest::RawIoctl {
            request: ioctl_request(0, DMABUF_IOCTL_TYPE, 3, 8),
            arg: import_sync.as_mut_ptr() as u64,
        }),
        Err(ObjectError::DoesNotExist)
    ));
    assert!(matches!(
        prime.configure(ConfigurateRequest::RawIoctl {
            request: ioctl_request(0, b'x', 0, 8),
            arg: sync.as_mut_ptr() as u64,
        }),
        Err(ObjectError::InvalidRequest)
    ));

    let mut gem_close = crate::drm::client::DrmGemClose {
        handle: create.handle,
        pad: 0,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmGemClose(&mut gem_close))
            .unwrap(),
        0
    );
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeMapDumb(&mut map)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut remove = addfb2.fb_id;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(&mut remove))
            .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(&mut flip_fb.fb_id))
            .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(&mut offset_fb.fb_id))
            .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(
            &mut fullscreen_fb.fb_id
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(
            &mut fullscreen_flip_fb.fb_id
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeRemoveFb(&mut addfb.fb_id))
            .unwrap(),
        0
    );
    let mut destroy = DrmModeDestroyDumb {
        handle: prime_import.handle,
    };
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy))
            .unwrap(),
        0
    );
    destroy.handle = flip_create.handle;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy))
            .unwrap(),
        0
    );
    destroy.handle = offset_create.handle;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy))
            .unwrap(),
        0
    );
    destroy.handle = fullscreen_create.handle;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy))
            .unwrap(),
        0
    );
    destroy.handle = fullscreen_flip_create.handle;
    assert_eq!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy))
            .unwrap(),
        0
    );
    destroy.handle = create.handle;
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeDestroyDumb(&mut destroy)),
        Err(ObjectError::InvalidArguments)
    ));

    let mut bad_property = DrmModeGetProperty {
        prop_id: 0xdead,
        ..Default::default()
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeGetProperty(&mut bad_property)),
        Err(ObjectError::InvalidArguments)
    ));
    let mut bad_obj_props = DrmModeObjGetProperties {
        obj_id: 0xdead,
        obj_type: DRM_MODE_OBJECT_FB,
        ..Default::default()
    };
    assert!(matches!(
        card.configure(ConfigurateRequest::DrmModeObjGetProperties(
            &mut bad_obj_props
        )),
        Err(ObjectError::InvalidArguments)
    ));
    assert!(matches!(
        card.configure(ConfigurateRequest::RawIoctl {
            request: DRM_IOCTL_PRIME_HANDLE_TO_FD,
            arg: 0,
        }),
        Err(ObjectError::InvalidRequest)
    ));
    assert!(matches!(
        card.configure(ConfigurateRequest::RawIoctl {
            request: DRM_IOCTL_PRIME_FD_TO_HANDLE,
            arg: 0,
        }),
        Err(ObjectError::InvalidRequest)
    ));
}

fn write_dumb_xrgb_pixel(handle: u32, x: usize, y: usize, bgra: [u8; 4]) {
    let (kernel_addr, pitch) = {
        let state = DRM_STATE.lock();
        let buffer = state.dumb_buffers.get(&handle).unwrap();
        (buffer.kernel_addr, buffer.width as usize * 4)
    };
    let offset = y
        .checked_mul(pitch)
        .and_then(|row| row.checked_add(x * 4))
        .unwrap();
    unsafe {
        core::ptr::copy_nonoverlapping(bgra.as_ptr(), (kernel_addr as *mut u8).add(offset), 4);
    }
}

fn write_dumb_xrgb_at_offset(handle: u32, offset: usize, bgra: [u8; 4]) {
    let kernel_addr = {
        let state = DRM_STATE.lock();
        state.dumb_buffers.get(&handle).unwrap().kernel_addr
    };
    unsafe {
        core::ptr::copy_nonoverlapping(bgra.as_ptr(), (kernel_addr as *mut u8).add(offset), 4);
    }
}

fn assert_visible_bgr_pixel(x: usize, y: usize, bgra: [u8; 4]) {
    let framebuffer = FRAME_BUFFER.get().unwrap().lock();
    assert_eq!(framebuffer.info.bytes_per_pixel, 4);
    let offset = (y * framebuffer.info.stride + x) * framebuffer.info.bytes_per_pixel;
    assert_eq!(&framebuffer.fb[offset..offset + 4], &bgra);
}

fn pop_drm_event() -> DrmEventVblank {
    let bytes: Vec<u8> = DRM_EVENT_QUEUE
        .lock()
        .drain(..core::mem::size_of::<DrmEventVblank>())
        .collect();
    assert_eq!(bytes.len(), core::mem::size_of::<DrmEventVblank>());
    unsafe { core::ptr::read(bytes.as_ptr().cast::<DrmEventVblank>()) }
}
