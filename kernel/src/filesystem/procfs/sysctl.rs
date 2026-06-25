use super::*;

const DEFAULT_FILE_MAX: u64 = 1_048_576;
const DEFAULT_INOTIFY_MAX_QUEUED_EVENTS: u64 = 16_384;
const DEFAULT_INOTIFY_MAX_USER_INSTANCES: u64 = 128;
const DEFAULT_INOTIFY_MAX_USER_WATCHES: u64 = 524_288;
const DEFAULT_NR_OPEN: u64 = 1_048_576;
const DEFAULT_PID_MAX: u64 = 4_194_304;

pub(super) static PROC_FILE_MAX: AtomicU64 = AtomicU64::new(DEFAULT_FILE_MAX);
pub(super) static PROC_INOTIFY_MAX_QUEUED_EVENTS: AtomicU64 =
    AtomicU64::new(DEFAULT_INOTIFY_MAX_QUEUED_EVENTS);
pub(super) static PROC_INOTIFY_MAX_USER_INSTANCES: AtomicU64 =
    AtomicU64::new(DEFAULT_INOTIFY_MAX_USER_INSTANCES);
pub(super) static PROC_INOTIFY_MAX_USER_WATCHES: AtomicU64 =
    AtomicU64::new(DEFAULT_INOTIFY_MAX_USER_WATCHES);
pub(crate) static PROC_NR_OPEN: AtomicU64 = AtomicU64::new(DEFAULT_NR_OPEN);
pub(super) static PROC_PID_MAX: AtomicU64 = AtomicU64::new(DEFAULT_PID_MAX);
pub(super) static PROC_NR_HUGEPAGES: AtomicU64 = AtomicU64::new(0);
pub(super) static PROC_HUGETLB_SHM_GROUP: AtomicU64 = AtomicU64::new(0);
pub(super) static PROC_OVERCOMMIT_MEMORY: AtomicU64 = AtomicU64::new(0);
static PROC_DROP_CACHES_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn proc_hostname_bytes() -> Vec<u8> {
    proc_c_string_bytes(crate::misc::utsname::current_hostname(crate::NAME))
}

pub(super) fn proc_domainname_bytes() -> Vec<u8> {
    proc_c_string_bytes(crate::misc::utsname::current_domainname("(none)"))
}

pub(super) fn proc_osrelease_bytes() -> Vec<u8> {
    format!("{}\n", crate::KERNEL_RELEASE).into_bytes()
}

pub(super) fn proc_meminfo_bytes() -> Vec<u8> {
    let total_kib = crate::memory::usable_memory_bytes() / 1024;
    format!(
        concat!(
            "MemTotal:       {:>8} kB\n",
            "MemFree:        {:>8} kB\n",
            "MemAvailable:   {:>8} kB\n",
            "Buffers:        {:>8} kB\n",
            "Cached:         {:>8} kB\n",
            "SwapCached:     {:>8} kB\n",
            "Active:         {:>8} kB\n",
            "Inactive:       {:>8} kB\n",
            "Active(anon):   {:>8} kB\n",
            "Inactive(anon): {:>8} kB\n",
            "Active(file):   {:>8} kB\n",
            "Inactive(file): {:>8} kB\n",
            "Unevictable:    {:>8} kB\n",
            "Mlocked:        {:>8} kB\n",
            "SwapTotal:      {:>8} kB\n",
            "SwapFree:       {:>8} kB\n",
            "HugePages_Total:{:>8}\n",
            "HugePages_Free: {:>8}\n",
            "HugePages_Rsvd: {:>8}\n",
            "HugePages_Surp: {:>8}\n",
            "Hugepagesize:   {:>8} kB\n",
            "Hugetlb:        {:>8} kB\n"
        ),
        total_kib, total_kib, total_kib, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2048, 0,
    )
    .into_bytes()
}

pub(super) fn proc_pressure_bytes() -> Vec<u8> {
    b"some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"
        .to_vec()
}

pub(super) fn proc_write_pressure(buffer: &[u8]) -> FSResult<usize> {
    // systemd programs PSI triggers via writes to /proc/pressure/*.
    // We do not implement real PSI accounting yet, but accepting the
    // trigger string matches the expected userspace setup flow.
    Ok(buffer.len())
}

pub(super) fn proc_write_hostname(buffer: &[u8]) -> FSResult<usize> {
    let value = proc_trim_sysctl_string(buffer)?;
    crate::misc::utsname::set_hostname(value.as_bytes()).map_err(|_| FSError::Other)?;
    Ok(buffer.len())
}

pub(super) fn proc_write_domainname(buffer: &[u8]) -> FSResult<usize> {
    let value = proc_trim_sysctl_string(buffer)?;
    crate::misc::utsname::set_domainname(value.as_bytes()).map_err(|_| FSError::Other)?;
    Ok(buffer.len())
}

pub(super) fn proc_c_string_bytes(value: [u8; 65]) -> Vec<u8> {
    let len = value
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(value.len());
    let mut bytes = value[..len].to_vec();
    bytes.push(b'\n');
    bytes
}

pub(super) fn proc_trim_sysctl_string(buffer: &[u8]) -> FSResult<&str> {
    core::str::from_utf8(buffer)
        .map(|value| value.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0'))
        .map_err(|_| FSError::Other)
}

pub(super) fn proc_fs_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("file-max".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("inotify".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("nr_open".into(), DirectoryContentType::File),
    ]
}

pub(super) fn proc_fs_inotify_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("max_queued_events".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("max_user_instances".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("max_user_watches".into(), DirectoryContentType::File),
    ]
}

pub(super) fn proc_sys_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("fs".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("kernel".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("net".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("vm".into(), DirectoryContentType::Directory),
    ]
}

pub(super) fn proc_sys_net_entries() -> Vec<DirectoryContentInfo> {
    vec![DirectoryContentInfo::new(
        "ipv4".into(),
        DirectoryContentType::Directory,
    )]
}

pub(super) fn proc_sys_net_ipv4_entries() -> Vec<DirectoryContentInfo> {
    vec![DirectoryContentInfo::new(
        "conf".into(),
        DirectoryContentType::Directory,
    )]
}

pub(super) fn proc_sys_net_ipv4_conf_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("default".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("lo".into(), DirectoryContentType::Directory),
    ]
}

pub(super) fn proc_sys_net_ipv4_conf_if_entries() -> Vec<DirectoryContentInfo> {
    vec![DirectoryContentInfo::new(
        "tag".into(),
        DirectoryContentType::File,
    )]
}

pub(super) fn proc_vm_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("drop_caches".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("nr_hugepages".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("hugetlb_shm_group".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("overcommit_memory".into(), DirectoryContentType::File),
    ]
}

pub(super) fn proc_sysctl_value_bytes(value: &AtomicU64) -> Vec<u8> {
    format!("{}\n", value.load(Ordering::Relaxed)).into_bytes()
}

pub(super) fn proc_net_ipv4_conf_lo_tag_bytes() -> Vec<u8> {
    let namespace = crate::process::manager::get_current_process()
        .lock()
        .net_namespace
        .clone();
    format!("{}\n", namespace.ipv4_conf_lo_tag()).into_bytes()
}

pub(super) fn proc_net_ipv4_conf_default_tag_bytes() -> Vec<u8> {
    let namespace = crate::process::manager::get_current_process()
        .lock()
        .net_namespace
        .clone();
    format!("{}\n", namespace.ipv4_conf_default_tag()).into_bytes()
}

fn parse_sysctl_u64(buffer: &[u8]) -> FSResult<u64> {
    let content = core::str::from_utf8(buffer).map_err(|_| FSError::Other)?;
    content
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0')
        .parse::<u64>()
        .map_err(|_| FSError::Other)
}

pub(super) fn proc_write_sysctl_u64(target: &AtomicU64, buffer: &[u8]) -> FSResult<usize> {
    let value = parse_sysctl_u64(buffer)?;
    target.store(value, Ordering::Relaxed);
    Ok(buffer.len())
}

pub(super) fn proc_drop_caches_bytes() -> Vec<u8> {
    b"0\n".to_vec()
}

pub(super) fn proc_write_drop_caches(buffer: &[u8]) -> FSResult<usize> {
    match parse_sysctl_u64(buffer)? {
        1..=3 => {
            PROC_DROP_CACHES_GENERATION.fetch_add(1, Ordering::Relaxed);
            Ok(buffer.len())
        }
        _ => Err(FSError::InvalidArguments),
    }
}

pub(crate) fn proc_drop_caches_generation() -> u64 {
    PROC_DROP_CACHES_GENERATION.load(Ordering::Relaxed)
}

pub(super) fn proc_write_net_ipv4_conf_lo_tag(buffer: &[u8]) -> FSResult<usize> {
    let value = parse_sysctl_u64(buffer)?;
    let namespace = crate::process::manager::get_current_process()
        .lock()
        .net_namespace
        .clone();
    namespace.set_ipv4_conf_lo_tag(value);
    Ok(buffer.len())
}

pub(super) fn proc_write_net_ipv4_conf_default_tag(buffer: &[u8]) -> FSResult<usize> {
    let value = parse_sysctl_u64(buffer)?;
    let namespace = crate::process::manager::get_current_process()
        .lock()
        .net_namespace
        .clone();
    namespace.set_ipv4_conf_default_tag(value);
    Ok(buffer.len())
}
