use alloc::{
    collections::{BTreeMap, vec_deque::VecDeque},
    sync::Arc,
    vec::Vec,
};
use core::{cmp::min, mem::size_of, slice};

use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{PageTableFlags, PhysFrame, Size4KiB, Translate, mapper::TranslateResult},
};

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    memory::{
        addrspace::{AddrSpace, mem_area::Data},
        paging::{FRAME_ALLOCATOR, MAPPER},
        protection::Protection,
        user_safe,
        utils::apply_offset,
    },
    misc::framebuffer::{FRAME_BUFFER, FramebufferPixelFormat, framebuffer_set_user_controlled},
    misc::time::Time,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        device::DEVICES,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::{copy_from_queue, read_or_block_with_flags},
        traits::{Configuratable, MemoryMappable, Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    process::misc::with_current_process,
    thread::{THREAD_MANAGER, yielding::WakeType},
};

use super::abi::{
    CARD0_RDEV, CONNECTOR0_ID, CRTC0_ID, DRIVER_DATE, DRIVER_DESC, DRIVER_NAME,
    DRM_CAP_DUMB_BUFFER, DRM_CAP_DUMB_PREFER_SHADOW, DRM_CAP_DUMB_PREFERRED_DEPTH,
    DRM_CAP_TIMESTAMP_MONOTONIC, DRM_CLIENT_CAP_ASPECT_RATIO, DRM_CLIENT_CAP_ATOMIC,
    DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT, DRM_CLIENT_CAP_STEREO_3D, DRM_CLIENT_CAP_UNIVERSAL_PLANES,
    DRM_CLIENT_CAP_WRITEBACK_CONNECTORS, DRM_EVENT_FLIP_COMPLETE, DRM_EVENT_VBLANK,
    DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, DRM_MODE_CONNECTED, DRM_MODE_CONNECTOR_VIRTUAL,
    DRM_MODE_ENCODER_VIRTUAL, DRM_MODE_FB_MODIFIERS, DRM_MODE_OBJECT_CONNECTOR,
    DRM_MODE_OBJECT_CRTC, DRM_MODE_OBJECT_ENCODER, DRM_MODE_OBJECT_FB, DRM_MODE_OBJECT_PLANE,
    DRM_MODE_PAGE_FLIP_ASYNC, DRM_MODE_PAGE_FLIP_EVENT, DRM_MODE_PAGE_FLIP_TARGET,
    DRM_MODE_PROP_ENUM, DRM_MODE_PROP_IMMUTABLE, DRM_MODE_SUBPIXEL_UNKNOWN, DRM_PLANE_TYPE_CURSOR,
    DRM_PLANE_TYPE_OVERLAY, DRM_PLANE_TYPE_PRIMARY, DRM_VBLANK_EVENT, DRM_VBLANK_FLAGS_MASK,
    DRM_VBLANK_FLIP, DRM_VBLANK_SIGNAL, DRM_VBLANK_TYPES_MASK, DrmEvent, DrmEventVblank,
    DrmGemClose, DrmModeCreateDumb, DrmModePropertyEnum, DrmWaitVblankReply, ENCODER0_ID,
    PLANE_TYPE_PROP_ID, PRIMARY_PLANE0_ID, current_framebuffer_info, current_mode_info,
};

lazy_static! {
    static ref DRM_STATE: Mutex<DrmState> = Mutex::new(DrmState::new());
    static ref DRM_EVENT_QUEUE: Mutex<VecDeque<u8>> = Mutex::new(VecDeque::new());
}

const DRM_BUFFER_OFFSET_BASE: u64 = 0x1000_0000;

#[derive(Default, Debug)]
pub struct DrmCardObject;

#[derive(Debug)]
struct DrmState {
    next_handle: u32,
    next_fb_id: u32,
    next_map_offset: u64,
    next_flip_sequence: u32,
    dumb_buffers: BTreeMap<u32, DumbBuffer>,
    framebuffers: BTreeMap<u32, RegisteredFramebuffer>,
    current_fb_id: Option<u32>,
}

#[derive(Clone, Debug)]
struct DumbBuffer {
    width: u32,
    height: u32,
    bpp: u32,
    size: u64,
    map_offset: u64,
    start_frame: PhysFrame<Size4KiB>,
    pages: usize,
    kernel_addr: u64,
    shared_flags: PageTableFlags,
    user_handle_open: bool,
    framebuffer_refs: u32,
    scanout_backed: bool,
}

#[derive(Clone, Debug)]
struct RegisteredFramebuffer {
    fb_id: u32,
    width: u32,
    height: u32,
    pitch: u32,
    offset: u32,
    pixel_format: u32,
    handle: u32,
}

impl DrmState {
    const fn new() -> Self {
        Self {
            next_handle: 1,
            next_fb_id: 1,
            next_map_offset: DRM_BUFFER_OFFSET_BASE,
            next_flip_sequence: 1,
            dumb_buffers: BTreeMap::new(),
            framebuffers: BTreeMap::new(),
            current_fb_id: None,
        }
    }

    fn create_dumb_buffer(&mut self, request: &mut DrmModeCreateDumb) -> ObjectResult<()> {
        if request.width == 0 || request.height == 0 || request.bpp == 0 || request.flags != 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let bytes_per_pixel = request.bpp.div_ceil(8);
        let pitch = request
            .width
            .checked_mul(bytes_per_pixel)
            .ok_or(ObjectError::InvalidArguments)?;
        let size = u64::from(pitch)
            .checked_mul(u64::from(request.height))
            .ok_or(ObjectError::InvalidArguments)?;
        let pages =
            usize::try_from(size.div_ceil(4096)).map_err(|_| ObjectError::InvalidArguments)?;
        if pages == 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let (start_frame, kernel_addr, shared_flags, scanout_backed) =
            if let Some((start_frame, kernel_addr, shared_flags)) = self
                .try_allocate_scanout_backing(
                    request.width,
                    request.height,
                    request.bpp,
                    pitch,
                    size,
                    pages,
                )
            {
                (start_frame, kernel_addr, shared_flags, true)
            } else {
                let start_frame = FRAME_ALLOCATOR
                    .get()
                    .unwrap()
                    .lock()
                    .allocate_contiguous(pages)
                    .ok_or(ObjectError::Other)?;
                let kernel_addr = apply_offset(start_frame.start_address().as_u64());
                unsafe {
                    core::ptr::write_bytes(kernel_addr as *mut u8, 0, pages * 4096);
                }
                (start_frame, kernel_addr, PageTableFlags::empty(), false)
            };

        let handle = self.next_handle;
        self.next_handle = self.next_handle.checked_add(1).ok_or(ObjectError::Other)?;
        let map_offset = self.next_map_offset;
        self.next_map_offset = self
            .next_map_offset
            .checked_add((pages as u64) * 4096)
            .and_then(|next| next.checked_add(4096))
            .ok_or(ObjectError::Other)?;

        self.dumb_buffers.insert(
            handle,
            DumbBuffer {
                width: request.width,
                height: request.height,
                bpp: request.bpp,
                size,
                map_offset,
                start_frame,
                pages,
                kernel_addr,
                shared_flags,
                user_handle_open: true,
                framebuffer_refs: 0,
                scanout_backed,
            },
        );

        request.handle = handle;
        request.pitch = pitch;
        request.size = size;
        Ok(())
    }

    fn register_framebuffer(&mut self, framebuffer: RegisteredFramebuffer) {
        let buffer = self
            .dumb_buffers
            .get_mut(&framebuffer.handle)
            .expect("framebuffer registration must reference an existing dumb buffer");
        buffer.framebuffer_refs = buffer
            .framebuffer_refs
            .checked_add(1)
            .expect("framebuffer refcount overflow");
        self.framebuffers.insert(framebuffer.fb_id, framebuffer);
    }

    fn next_fb_id(&mut self) -> ObjectResult<u32> {
        let fb_id = self.next_fb_id;
        self.next_fb_id = self.next_fb_id.checked_add(1).ok_or(ObjectError::Other)?;
        Ok(fb_id)
    }

    fn dumb_buffer_for_mapping(
        &self,
        offset: u64,
        pages: u64,
    ) -> ObjectResult<(usize, PhysFrame<Size4KiB>, PageTableFlags)> {
        for buffer in self.dumb_buffers.values() {
            let end_offset = buffer
                .map_offset
                .checked_add(buffer.aligned_size())
                .ok_or(ObjectError::InvalidArguments)?;
            if !(buffer.map_offset..end_offset).contains(&offset) {
                continue;
            }

            let byte_delta = offset - buffer.map_offset;
            if !byte_delta.is_multiple_of(4096) {
                return Err(ObjectError::InvalidArguments);
            }

            let page_delta =
                usize::try_from(byte_delta / 4096).map_err(|_| ObjectError::InvalidArguments)?;
            let requested_pages =
                usize::try_from(pages).map_err(|_| ObjectError::InvalidArguments)?;
            if requested_pages == 0 || page_delta + requested_pages > buffer.pages {
                return Err(ObjectError::InvalidArguments);
            }

            let start_addr =
                buffer.start_frame.start_address().as_u64() + (page_delta as u64 * 4096);
            return Ok((
                requested_pages,
                PhysFrame::containing_address(PhysAddr::new(start_addr)),
                buffer.shared_flags,
            ));
        }

        Err(ObjectError::InvalidArguments)
    }

    fn try_allocate_scanout_backing(
        &self,
        width: u32,
        height: u32,
        bpp: u32,
        pitch: u32,
        size: u64,
        pages: usize,
    ) -> Option<(PhysFrame<Size4KiB>, u64, PageTableFlags)> {
        if self
            .dumb_buffers
            .values()
            .any(|buffer| buffer.scanout_backed)
        {
            return None;
        }

        let fb_info = current_framebuffer_info();
        if bpp != 32
            || width != fb_info.width as u32
            || height != fb_info.height as u32
            || pitch != (fb_info.stride * fb_info.bytes_per_pixel) as u32
            || size > fb_info.byte_len as u64
        {
            return None;
        }

        let framebuffer = FRAME_BUFFER.get().unwrap().lock();
        let fb_addr = VirtAddr::new(framebuffer.fb.as_ptr() as u64);
        let mut shared_flags = PageTableFlags::NO_CACHE;
        let mapper = MAPPER.get().unwrap().lock();
        let phys = mapper.translate_addr(fb_addr)?;
        if phys.as_u64() & 0xfff != 0 {
            return None;
        }
        if let TranslateResult::Mapped { flags, .. } = mapper.translate(fb_addr) {
            shared_flags |= flags & (PageTableFlags::WRITE_THROUGH | PageTableFlags::NO_CACHE);
        }
        if (pages as u64) * 4096 > (fb_info.byte_len as u64).div_ceil(4096) * 4096 {
            return None;
        }

        Some((
            PhysFrame::containing_address(phys),
            apply_offset(phys.as_u64()),
            shared_flags,
        ))
    }

    fn get_user_handle(&self, handle: u32) -> ObjectResult<&DumbBuffer> {
        let buffer = self
            .dumb_buffers
            .get(&handle)
            .ok_or(ObjectError::InvalidArguments)?;
        if !buffer.user_handle_open {
            return Err(ObjectError::InvalidArguments);
        }
        Ok(buffer)
    }

    fn close_dumb_handle(&mut self, handle: u32) -> ObjectResult<()> {
        let buffer = self
            .dumb_buffers
            .get_mut(&handle)
            .ok_or(ObjectError::InvalidArguments)?;
        if !buffer.user_handle_open {
            return Err(ObjectError::InvalidArguments);
        }
        buffer.user_handle_open = false;
        Ok(())
    }

    fn remove_framebuffer(&mut self, fb_id: u32) -> ObjectResult<()> {
        let framebuffer = self
            .framebuffers
            .remove(&fb_id)
            .ok_or(ObjectError::InvalidArguments)?;
        let buffer = self
            .dumb_buffers
            .get_mut(&framebuffer.handle)
            .ok_or(ObjectError::InvalidArguments)?;
        buffer.framebuffer_refs = buffer
            .framebuffer_refs
            .checked_sub(1)
            .ok_or(ObjectError::InvalidArguments)?;
        if self.current_fb_id == Some(fb_id) {
            self.current_fb_id = None;
        }
        Ok(())
    }
}

impl DumbBuffer {
    fn aligned_size(&self) -> u64 {
        self.size.div_ceil(4096) * 4096
    }

    fn contains_scanout_range(&self, offset: u32, pitch: u32, width: u32, height: u32) -> bool {
        if width > self.width || height > self.height || self.bpp < 32 {
            return false;
        }

        let bytes_per_pixel = self.bpp.div_ceil(8);
        if pitch < width.saturating_mul(bytes_per_pixel) {
            return false;
        }

        let required = u64::from(offset)
            .saturating_add(u64::from(pitch).saturating_mul(u64::from(height.saturating_sub(1))))
            .saturating_add(u64::from(width) * u64::from(bytes_per_pixel));
        required <= self.size
    }
}

impl Object for DrmCardObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("mappable", MemoryMappable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("readable", Readable);
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
            ConfigurateRequest::DrmWaitVblank(ptr) => {
                let mut wait = read_user(ptr)?;
                let request = unsafe { wait.request };
                let flags = request.type_ & DRM_VBLANK_FLAGS_MASK;
                if request.type_ & !(DRM_VBLANK_TYPES_MASK | DRM_VBLANK_FLAGS_MASK) != 0
                    || flags & (DRM_VBLANK_SIGNAL | DRM_VBLANK_FLIP) != 0
                {
                    return Err(ObjectError::InvalidArguments);
                }

                let reply = make_vblank_reply(request.type_, request.sequence);
                if request.type_ & DRM_VBLANK_EVENT != 0 {
                    queue_vblank_event(request.signal, CRTC0_ID, DRM_EVENT_VBLANK, reply.sequence);
                }
                wait.reply = reply;
                user_safe::write(ptr, &wait).map_err(|_| ObjectError::InvalidArguments)?;
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
            ConfigurateRequest::DrmSetMaster => Ok(0),
            ConfigurateRequest::DrmDropMaster => {
                framebuffer_set_user_controlled(false);
                DRM_STATE.lock().current_fb_id = None;
                Ok(0)
            }
            ConfigurateRequest::DrmModeGetResources(ptr) => {
                let mut resources = read_user(ptr)?;
                let fb = current_framebuffer_info();
                let framebuffer_ids: Vec<u32> =
                    DRM_STATE.lock().framebuffers.keys().copied().collect();
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
            ConfigurateRequest::DrmModeGetCrtc(ptr) => {
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
            ConfigurateRequest::DrmModeSetCrtc(ptr) => {
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
                    framebuffer_set_user_controlled(false);
                } else {
                    scanout_framebuffer_id(crtc.fb_id)?;
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
            ConfigurateRequest::DrmModeGetProperty(ptr) => {
                let mut property = read_user(ptr)?;
                match property.prop_id {
                    PLANE_TYPE_PROP_ID => {
                        let enums = [
                            make_property_enum(DRM_PLANE_TYPE_OVERLAY, "Overlay"),
                            make_property_enum(DRM_PLANE_TYPE_PRIMARY, "Primary"),
                            make_property_enum(DRM_PLANE_TYPE_CURSOR, "Cursor"),
                        ];
                        maybe_write_struct_slice(
                            property.enum_blob_ptr,
                            property.count_enum_blobs,
                            &enums,
                        )?;
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
            ConfigurateRequest::DrmModeObjGetProperties(ptr) => {
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
                        user_safe::write(ptr, &properties)
                            .map_err(|_| ObjectError::InvalidArguments)?;
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
            ConfigurateRequest::DrmModeGetPlaneResources(ptr) => {
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
            ConfigurateRequest::DrmModeGetPlane(ptr) => {
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
            ConfigurateRequest::DrmModeAddFb(ptr) => {
                let mut fb = read_user(ptr)?;
                let pixel_format = match (fb.bpp, fb.depth) {
                    (32, 24) => DRM_FORMAT_XRGB8888,
                    (32, 32) => DRM_FORMAT_ARGB8888,
                    _ => return Err(ObjectError::InvalidArguments),
                };
                let registered = build_framebuffer(
                    &DRM_STATE.lock(),
                    fb.handle,
                    fb.width,
                    fb.height,
                    fb.pitch,
                    0,
                    pixel_format,
                )?;
                let mut state = DRM_STATE.lock();
                let fb_id = state.next_fb_id()?;
                let mut registered = registered;
                registered.fb_id = fb_id;
                state.register_framebuffer(registered);
                fb.fb_id = fb_id;
                user_safe::write(ptr, &fb).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeAddFb2(ptr) => {
                let mut fb = read_user(ptr)?;
                if fb.flags & !DRM_MODE_FB_MODIFIERS != 0 {
                    return Err(ObjectError::InvalidArguments);
                }
                if !matches!(fb.pixel_format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888) {
                    return Err(ObjectError::InvalidArguments);
                }
                if fb.handles[1..].iter().any(|&handle| handle != 0)
                    || fb.pitches[1..].iter().any(|&pitch| pitch != 0)
                    || fb.offsets[1..].iter().any(|&offset| offset != 0)
                    || fb.modifier[1..].iter().any(|&modifier| modifier != 0)
                {
                    return Err(ObjectError::InvalidArguments);
                }
                if fb.flags & DRM_MODE_FB_MODIFIERS != 0 && fb.modifier[0] != 0 {
                    return Err(ObjectError::InvalidArguments);
                }

                let registered = build_framebuffer(
                    &DRM_STATE.lock(),
                    fb.handles[0],
                    fb.width,
                    fb.height,
                    fb.pitches[0],
                    fb.offsets[0],
                    fb.pixel_format,
                )?;
                let mut state = DRM_STATE.lock();
                let fb_id = state.next_fb_id()?;
                let mut registered = registered;
                registered.fb_id = fb_id;
                state.register_framebuffer(registered);
                fb.fb_id = fb_id;
                user_safe::write(ptr, &fb).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeRemoveFb(ptr) => {
                let fb_id = read_user(ptr)?;
                let mut state = DRM_STATE.lock();
                state.remove_framebuffer(fb_id)?;
                if state.current_fb_id.is_none() {
                    framebuffer_set_user_controlled(false);
                }
                Ok(0)
            }
            ConfigurateRequest::DrmModePageFlip(ptr) => {
                let flip = read_user(ptr)?;
                if flip.crtc_id != CRTC0_ID || flip.reserved != 0 {
                    return Err(ObjectError::InvalidArguments);
                }
                if flip.flags & DRM_MODE_PAGE_FLIP_ASYNC != 0
                    || flip.flags & DRM_MODE_PAGE_FLIP_TARGET != 0
                {
                    return Err(ObjectError::Unimplemented);
                }
                if flip.flags & !DRM_MODE_PAGE_FLIP_EVENT != 0 {
                    return Err(ObjectError::InvalidArguments);
                }

                scanout_framebuffer_id(flip.fb_id)?;
                if flip.flags & DRM_MODE_PAGE_FLIP_EVENT != 0 {
                    queue_page_flip_event(flip.user_data, flip.crtc_id);
                }
                Ok(0)
            }
            ConfigurateRequest::DrmModeCreateDumb(ptr) => {
                let mut request = read_user(ptr)?;
                DRM_STATE.lock().create_dumb_buffer(&mut request)?;
                user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeMapDumb(ptr) => {
                let mut request = read_user(ptr)?;
                let state = DRM_STATE.lock();
                let buffer = state.get_user_handle(request.handle)?;
                request.offset = buffer.map_offset;
                user_safe::write(ptr, &request).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::DrmModeDestroyDumb(ptr) => {
                let request = read_user(ptr)?;
                DRM_STATE.lock().close_dumb_handle(request.handle)?;
                Ok(0)
            }
            ConfigurateRequest::DrmGemClose(ptr) => {
                let request: DrmGemClose = read_user(ptr)?;
                DRM_STATE.lock().close_dumb_handle(request.handle)?;
                Ok(0)
            }
            ConfigurateRequest::RawIoctl { .. } => Err(ObjectError::InvalidArguments),
            _ => Err(ObjectError::InvalidArguments),
        }
    }
}

impl MemoryMappable for DrmCardObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> ObjectResult<VirtAddr> {
        let (page_count, start_frame, shared_flags) =
            DRM_STATE.lock().dumb_buffer_for_mapping(offset, pages)?;
        let mut frames = Vec::with_capacity(page_count);
        for page_index in 0..page_count {
            let frame_addr = start_frame.start_address().as_u64() + (page_index as u64 * 4096);
            frames.push(PhysFrame::containing_address(PhysAddr::new(frame_addr)));
        }

        Ok(with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                protection,
                Data::Shared {
                    frames: Arc::<[PhysFrame]>::from(frames),
                    flags: shared_flags,
                },
            )
        }))
    }
}

impl Readable for DrmCardObject {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, FileFlags::empty())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::IO, try_read_drm_events)
    }
}

impl Pollable for DrmCardObject {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeRead) && !DRM_EVENT_QUEUE.lock().is_empty()
    }
}

impl Statable for DrmCardObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o660, CARD0_RDEV)
    }
}

fn build_framebuffer(
    state: &DrmState,
    handle: u32,
    width: u32,
    height: u32,
    pitch: u32,
    offset: u32,
    pixel_format: u32,
) -> ObjectResult<RegisteredFramebuffer> {
    let buffer = state.get_user_handle(handle)?;
    if !buffer.contains_scanout_range(offset, pitch, width, height) {
        return Err(ObjectError::InvalidArguments);
    }
    if !matches!(pixel_format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888) {
        return Err(ObjectError::InvalidArguments);
    }

    Ok(RegisteredFramebuffer {
        fb_id: 0,
        width,
        height,
        pitch,
        offset,
        pixel_format,
        handle,
    })
}

fn scanout_framebuffer_id(fb_id: u32) -> ObjectResult<()> {
    let (framebuffer, dumb_buffer) = {
        let mut state = DRM_STATE.lock();
        let framebuffer = state
            .framebuffers
            .get(&fb_id)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        let dumb_buffer = state
            .dumb_buffers
            .get(&framebuffer.handle)
            .cloned()
            .ok_or(ObjectError::InvalidArguments)?;
        state.current_fb_id = Some(fb_id);
        (framebuffer, dumb_buffer)
    };

    if !dumb_buffer.scanout_backed {
        // TODO: This is still a legacy compatibility bridge over the Limine
        // framebuffer, not a real KMS scanout implementation.
        blit_dumb_buffer_to_scanout(&dumb_buffer, &framebuffer)?;
    }
    framebuffer_set_user_controlled(true);
    Ok(())
}

fn blit_dumb_buffer_to_scanout(
    dumb_buffer: &DumbBuffer,
    framebuffer: &RegisteredFramebuffer,
) -> ObjectResult<()> {
    let src_start = dumb_buffer
        .kernel_addr
        .checked_add(u64::from(framebuffer.offset))
        .ok_or(ObjectError::InvalidArguments)?;
    let src_bytes = usize::try_from(
        dumb_buffer
            .size
            .checked_sub(u64::from(framebuffer.offset))
            .ok_or(ObjectError::InvalidArguments)?,
    )
    .map_err(|_| ObjectError::InvalidArguments)?;
    let src = unsafe { slice::from_raw_parts(src_start as *const u8, src_bytes) };

    let mut canvas = FRAME_BUFFER.get().unwrap().lock();
    let width = min(framebuffer.width as usize, canvas.info.width);
    let height = min(framebuffer.height as usize, canvas.info.height);
    let dst_bytes_per_pixel = canvas.info.bytes_per_pixel;
    let dst_stride_bytes = canvas.info.stride * dst_bytes_per_pixel;
    let dst_pixel_format = canvas.info.pixel_format;
    let src_pitch = framebuffer.pitch as usize;

    if dst_bytes_per_pixel < 3 || src_pitch < width * 4 {
        return Err(ObjectError::InvalidArguments);
    }

    canvas.fb.fill(0);

    for y in 0..height {
        let src_row_start = y
            .checked_mul(src_pitch)
            .ok_or(ObjectError::InvalidArguments)?;
        let src_row_end = src_row_start
            .checked_add(width * 4)
            .ok_or(ObjectError::InvalidArguments)?;
        if src_row_end > src.len() {
            return Err(ObjectError::InvalidArguments);
        }

        let dst_row_start = y
            .checked_mul(dst_stride_bytes)
            .ok_or(ObjectError::InvalidArguments)?;
        let dst_row_end = dst_row_start
            .checked_add(width * dst_bytes_per_pixel)
            .ok_or(ObjectError::InvalidArguments)?;
        if dst_row_end > canvas.fb.len() {
            return Err(ObjectError::InvalidArguments);
        }

        let src_row = &src[src_row_start..src_row_end];
        let dst_row = &mut canvas.fb[dst_row_start..dst_row_end];

        for x in 0..width {
            let src_px = &src_row[x * 4..x * 4 + 4];
            let dst_px = &mut dst_row[x * dst_bytes_per_pixel..(x + 1) * dst_bytes_per_pixel];

            let blue = src_px[0];
            let green = src_px[1];
            let red = src_px[2];
            let alpha = if framebuffer.pixel_format == DRM_FORMAT_ARGB8888 {
                src_px[3]
            } else {
                0xff
            };

            match dst_pixel_format {
                FramebufferPixelFormat::Rgb => {
                    dst_px[0] = red;
                    dst_px[1] = green;
                    dst_px[2] = blue;
                }
                FramebufferPixelFormat::Bgr => {
                    dst_px[0] = blue;
                    dst_px[1] = green;
                    dst_px[2] = red;
                }
            }

            if dst_bytes_per_pixel >= 4 {
                dst_px[3] = alpha;
            }
        }
    }

    Ok(())
}

fn queue_page_flip_event(user_data: u64, crtc_id: u32) {
    let sequence = next_vblank_sequence();
    queue_vblank_event(user_data, crtc_id, DRM_EVENT_FLIP_COMPLETE, sequence);
}

fn next_vblank_sequence() -> u32 {
    let mut state = DRM_STATE.lock();
    let sequence = state.next_flip_sequence;
    state.next_flip_sequence = state.next_flip_sequence.wrapping_add(1);
    sequence
}

fn make_vblank_reply(type_: u32, sequence: u32) -> DrmWaitVblankReply {
    let now = Time::since_boot();
    DrmWaitVblankReply {
        type_,
        sequence,
        tv_sec: now.as_seconds().min(i64::MAX as u64) as i64,
        tv_usec: now.subsec_microseconds() as i64,
    }
}

fn queue_vblank_event(user_data: u64, crtc_id: u32, event_type: u32, sequence: u32) {
    let reply = make_vblank_reply(0, sequence);
    let event = DrmEventVblank {
        base: DrmEvent {
            type_: event_type,
            length: size_of::<DrmEventVblank>() as u32,
        },
        user_data,
        tv_sec: reply.tv_sec.clamp(0, i64::from(u32::MAX)) as u32,
        tv_usec: reply.tv_usec.clamp(0, i64::from(u32::MAX)) as u32,
        sequence: reply.sequence,
        crtc_id,
    };
    let bytes = unsafe {
        slice::from_raw_parts(
            (&event as *const DrmEventVblank).cast::<u8>(),
            size_of::<DrmEventVblank>(),
        )
    };
    DRM_EVENT_QUEUE.lock().extend(bytes.iter().copied());
    wake_drm_readable();
}

fn wake_drm_readable() {
    let drm_object = DEVICES.get("drm-card0").cloned().unwrap();
    let mut manager = THREAD_MANAGER.get().unwrap().lock();
    manager.wake_io();
    manager.wake_poller(drm_object, PollableEvent::CanBeRead);
}

fn try_read_drm_events(buffer: &mut [u8]) -> Option<usize> {
    let event_size = size_of::<DrmEventVblank>();
    if buffer.len() < event_size {
        return Some(0);
    }

    let mut queue = DRM_EVENT_QUEUE.lock();
    if queue.len() < event_size {
        return None;
    }

    let events_to_copy = min(buffer.len() / event_size, queue.len() / event_size);
    let bytes_to_copy = events_to_copy * event_size;
    Some(copy_from_queue(&mut queue, &mut buffer[..bytes_to_copy]))
}

fn read_user<T: Copy>(ptr: *mut T) -> ObjectResult<T> {
    user_safe::read(ptr.cast_const()).map_err(|_| ObjectError::InvalidArguments)
}

fn maybe_write_u32_slice(ptr: u64, capacity: u32, values: &[u32]) -> ObjectResult<()> {
    maybe_write_struct_slice(ptr, capacity, values)
}

fn maybe_write_u64_slice(ptr: u64, capacity: u32, values: &[u64]) -> ObjectResult<()> {
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

fn copy_property_name(dst: &mut [u8; 32], value: &str) {
    for (slot, byte) in dst.iter_mut().zip(value.bytes()) {
        *slot = byte;
    }
}

fn make_property_enum(value: u64, name: &str) -> DrmModePropertyEnum {
    let mut item = DrmModePropertyEnum {
        value,
        name: [0; 32],
    };
    copy_property_name(&mut item.name, name);
    item
}
