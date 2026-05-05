use crate::drm::mode::{
    DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_MODE_CURSOR_BO, DRM_MODE_CURSOR_FLAGS,
    DRM_MODE_CURSOR_MOVE, DRM_MODE_FB_DIRTY_ANNOTATE_COPY, DRM_MODE_FB_DIRTY_ANNOTATE_FILL,
    DRM_MODE_FB_DIRTY_FLAGS, DRM_MODE_PAGE_FLIP_ASYNC, DRM_MODE_PAGE_FLIP_EVENT,
    DRM_MODE_PAGE_FLIP_TARGET, DRM_MODE_PAGE_FLIP_TARGET_ABSOLUTE,
    DRM_MODE_PAGE_FLIP_TARGET_RELATIVE, MODE_GAMMA_LUT_SIZE, MODE_REFRESH_HZ,
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
