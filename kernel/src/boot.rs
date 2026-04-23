use limine::{
    BaseRevision,
    framebuffer::Framebuffer,
    memory_map::Entry,
    request::{
        FramebufferRequest, HhdmRequest, MemoryMapRequest, MpRequest, RequestsEndMarker,
        RequestsStartMarker, RsdpRequest, StackSizeRequest,
    },
    response::MpResponse,
};

const KERNEL_STACK_SIZE: u64 = 2 * 1024 * 1024;

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(KERNEL_STACK_SIZE);

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MP_REQUEST: MpRequest = MpRequest::new();

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

pub fn assert_supported() {
    assert!(BASE_REVISION.is_supported());
    let _ = STACK_SIZE_REQUEST.get_response();
}

pub fn physical_memory_offset() -> u64 {
    HHDM_REQUEST
        .get_response()
        .expect("limine hhdm response missing")
        .offset()
}

pub fn memory_map() -> &'static [&'static Entry] {
    MEMORY_MAP_REQUEST
        .get_response()
        .expect("limine memory map response missing")
        .entries()
}

pub fn framebuffer() -> Framebuffer<'static> {
    FRAMEBUFFER_REQUEST
        .get_response()
        .expect("limine framebuffer response missing")
        .framebuffers()
        .next()
        .expect("limine framebuffer missing")
}

pub fn rsdp_address() -> u64 {
    RSDP_REQUEST
        .get_response()
        .expect("limine rsdp response missing")
        .address() as u64
}

pub fn mp_response() -> &'static MpResponse {
    MP_REQUEST
        .get_response()
        .expect("limine mp response missing")
}
