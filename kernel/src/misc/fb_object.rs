use alloc::{sync::Arc, vec::Vec};
use core::ptr::read_volatile;
use spin::Mutex;
use x86_64::{VirtAddr, structures::paging::Translate};

use crate::{
    impl_cast_function,
    memory::{addrspace::mem_area::Data, paging::MAPPER, protection::Protection, user_safe},
    misc::{
        framebuffer::{
            FRAME_BUFFER, FramebufferInfo, FramebufferPixelFormat, framebuffer_set_user_controlled,
        },
        framebuffer_ioctl::{
            FB_TYPE_PACKED_PIXELS, FB_VISUAL_TRUECOLOR, FbBitfield, FbCmap, FbFixScreeninfo,
            FbVarScreeninfo,
        },
    },
    object::{
        Object,
        config::ConfigurateRequest,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Configuratable, MemoryMappable},
    },
    process::misc::with_current_process,
};

#[derive(Default, Debug)]
pub struct FramebufferObject {
    used_by_user: Mutex<bool>,
}

impl Object for FramebufferObject {
    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("mappable", MemoryMappable);
}

impl MemoryMappable for FramebufferObject {
    fn map(
        self: Arc<Self>,
        offset: u64,
        pages: u64,
        protection: Protection,
    ) -> crate::object::misc::ObjectResult<VirtAddr> {
        use alloc::vec::Vec;
        use x86_64::structures::paging::PhysFrame;
        use x86_64::structures::paging::{PageTableFlags, mapper::TranslateResult};

        let mut framebuffer = FRAME_BUFFER.get().unwrap().lock();

        let fb_ptr = framebuffer.fb.as_mut_ptr();
        let fb_len = framebuffer.info.byte_len as u64;

        if pages == 0 || offset % 4096 != 0 {
            return Err(ObjectError::InvalidArguments);
        }

        let map_offset = offset;
        let map_len = pages
            .checked_mul(4096)
            .ok_or(ObjectError::InvalidArguments)?;

        let fb_start_virt = VirtAddr::new(fb_ptr as u64);
        let fb_start_phys = MAPPER
            .get()
            .unwrap()
            .lock()
            .translate_addr(fb_start_virt)
            .ok_or(ObjectError::InvalidArguments)?;
        let fb_page_offset = fb_start_phys.as_u64() & 0xfff;
        let fb_window_len = fb_page_offset
            .checked_add(fb_len)
            .ok_or(ObjectError::InvalidArguments)?;
        let fb_window_len_aligned = fb_window_len.div_ceil(4096) * 4096;
        if map_offset
            .checked_add(map_len)
            .ok_or(ObjectError::InvalidArguments)?
            > fb_window_len_aligned
        {
            return Err(ObjectError::InvalidArguments);
        }

        let start_page_index = map_offset / 4096;
        let fb_base_virt = VirtAddr::new(fb_start_virt.as_u64() - fb_page_offset);
        let mut frames = Vec::with_capacity(pages as usize);
        let mut shared_flags = PageTableFlags::empty();

        {
            let mapper = MAPPER.get().unwrap().lock();

            if let TranslateResult::Mapped { flags, .. } = mapper.translate(fb_base_virt) {
                shared_flags = flags & (PageTableFlags::WRITE_THROUGH | PageTableFlags::NO_CACHE);
            }

            // Framebuffer memory is device memory, not normal DRAM. If the
            // bootloader left it cacheable, inheriting no cache bits would map
            // it as write-back in userspace, which can lead to corrupted or
            // stale scanout contents. Force uncached mappings for /dev/fb0.
            shared_flags |= PageTableFlags::NO_CACHE;

            for relative_page in 0..pages {
                let page_index = start_page_index + relative_page;
                let page_virt = fb_base_virt + page_index * 4096;
                let page_phys = mapper
                    .translate_addr(page_virt)
                    .ok_or(ObjectError::InvalidArguments)?;
                frames.push(PhysFrame::containing_address(page_phys));
            }
        }

        let user_addr = with_current_process(|process| {
            process.addrspace.allocate_user_lazy(
                pages,
                protection,
                Data::Shared {
                    frames: Arc::<[PhysFrame]>::from(frames),
                    flags: shared_flags,
                },
            )
        });

        framebuffer_set_user_controlled(true);

        // Xorg fbdev expects mmap(/dev/fb0) to return the page-aligned base.
        // It separately adds fix.smem_start's intra-page offset to compute
        // the first visible pixel address.
        Ok(user_addr)
    }
}

impl Configuratable for FramebufferObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::FbGetFixedScreenInfo(ptr) => {
                user_safe::write(ptr, &current_fb_fix_info())
                    .map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::FbGetVariableScreenInfo(ptr) => {
                user_safe::write(ptr, &current_fb_var_info())
                    .map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::FbPutVariableScreenInfo(ptr) => {
                if ptr.is_null() {
                    return Err(ObjectError::InvalidArguments);
                }

                let requested = unsafe { read_volatile(ptr) };
                let current = current_fb_var_info();
                if !fb_var_matches_current(&requested, &current) {
                    return Err(ObjectError::InvalidArguments);
                }
                user_safe::write(ptr, &current).map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::FbPanDisplay(ptr) => {
                if ptr.is_null() {
                    return Err(ObjectError::InvalidArguments);
                }

                let requested = unsafe { read_volatile(ptr) };
                if requested.xoffset != 0 || requested.yoffset != 0 {
                    return Err(ObjectError::InvalidArguments);
                }
                user_safe::write(ptr, &current_fb_var_info())
                    .map_err(|_| ObjectError::InvalidArguments)?;
                Ok(0)
            }
            ConfigurateRequest::FbGetColorMap(ptr) => {
                if ptr.is_null() {
                    return Err(ObjectError::InvalidArguments);
                }
                unsafe { fill_fb_cmap(&mut *ptr) };
                Ok(0)
            }
            ConfigurateRequest::FbPutColorMap(ptr) => {
                if ptr.is_null() {
                    return Err(ObjectError::InvalidArguments);
                }
                Ok(0)
            }
            ConfigurateRequest::FbBlank(_) => Ok(0),
            _ => Err(ObjectError::InvalidArguments),
        }
    }
}

fn current_fb_info() -> FramebufferInfo {
    FRAME_BUFFER.get().unwrap().lock().fb_info()
}

fn framebuffer_bitfields(
    pixel_format: FramebufferPixelFormat,
) -> (FbBitfield, FbBitfield, FbBitfield) {
    match pixel_format {
        FramebufferPixelFormat::Rgb => (
            FbBitfield {
                offset: 0,
                length: 8,
                msb_right: 0,
            },
            FbBitfield {
                offset: 8,
                length: 8,
                msb_right: 0,
            },
            FbBitfield {
                offset: 16,
                length: 8,
                msb_right: 0,
            },
        ),
        FramebufferPixelFormat::Bgr => (
            FbBitfield {
                offset: 16,
                length: 8,
                msb_right: 0,
            },
            FbBitfield {
                offset: 8,
                length: 8,
                msb_right: 0,
            },
            FbBitfield {
                offset: 0,
                length: 8,
                msb_right: 0,
            },
        ),
    }
}

fn framebuffer_transparency(bytes_per_pixel: usize) -> FbBitfield {
    if bytes_per_pixel >= 4 {
        FbBitfield {
            offset: 24,
            length: 8,
            msb_right: 0,
        }
    } else {
        FbBitfield::default()
    }
}

fn current_fb_fix_info() -> FbFixScreeninfo {
    let info = current_fb_info();
    let mut out = FbFixScreeninfo::default();
    let id = b"seelefb\0";
    for (dst, src) in out.id.iter_mut().zip(id.iter().copied()) {
        *dst = src as i8;
    }
    out.smem_start = info.phys_addr as u64;
    out.smem_len = info.byte_len as u32;
    out.type_ = FB_TYPE_PACKED_PIXELS;
    out.visual = FB_VISUAL_TRUECOLOR;
    out.line_length = (info.stride * info.bytes_per_pixel) as u32;
    out
}

fn current_fb_var_info() -> FbVarScreeninfo {
    let info = current_fb_info();
    let mut out = FbVarScreeninfo {
        xres: info.width as u32,
        yres: info.height as u32,
        xres_virtual: info.stride as u32,
        yres_virtual: info.height as u32,
        bits_per_pixel: (info.bytes_per_pixel * 8) as u32,
        height: u32::MAX,
        width: u32::MAX,
        ..FbVarScreeninfo::default()
    };
    let (red, green, blue) = framebuffer_bitfields(info.pixel_format);
    out.red = red;
    out.green = green;
    out.blue = blue;
    out.transp = framebuffer_transparency(info.bytes_per_pixel);
    out
}

fn fb_var_matches_current(requested: &FbVarScreeninfo, current: &FbVarScreeninfo) -> bool {
    requested.xres == current.xres
        && requested.yres == current.yres
        && requested.xres_virtual == current.xres_virtual
        && requested.yres_virtual == current.yres_virtual
        && requested.xoffset == current.xoffset
        && requested.yoffset == current.yoffset
        && requested.bits_per_pixel == current.bits_per_pixel
        && requested.grayscale == 0
        && requested.nonstd == 0
        && requested.rotate == 0
        && requested.red.offset == current.red.offset
        && requested.red.length == current.red.length
        && requested.red.msb_right == current.red.msb_right
        && requested.green.offset == current.green.offset
        && requested.green.length == current.green.length
        && requested.green.msb_right == current.green.msb_right
        && requested.blue.offset == current.blue.offset
        && requested.blue.length == current.blue.length
        && requested.blue.msb_right == current.blue.msb_right
        && requested.transp.offset == current.transp.offset
        && requested.transp.length == current.transp.length
        && requested.transp.msb_right == current.transp.msb_right
}

fn fill_fb_cmap(out: &mut FbCmap) {
    let len = out.len as usize;
    if len == 0 {
        return;
    }

    let zeros = Vec::from_iter(core::iter::repeat_n(0u8, len * core::mem::size_of::<u16>()));
    for ptr in [out.red, out.green, out.blue, out.transp] {
        if !ptr.is_null() {
            let _ = user_safe::write(ptr, &zeros[..]);
        }
    }
}
