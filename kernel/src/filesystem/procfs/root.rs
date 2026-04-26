use alloc::{collections::BTreeMap, format, string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

use crate::{
    filesystem::{
        info::DirectoryContentInfo,
        vfs::{FileSystemRef, VirtualFS},
        vfs_traits::{DirectoryContentType, MountFlags},
    },
    misc::time::Time,
    process::manager::MANAGER,
};

pub(super) const PROC_ROOT_INODE: u64 = 0x3000;
pub(super) const PROC_CMDLINE_INODE: u64 = 0x3001;
pub(super) const PROC_SELF_INODE: u64 = 0x3002;
pub(super) const PROC_MOUNTS_INODE: u64 = 0x3003;
pub(super) const PROC_SYS_INODE: u64 = 0x3004;
pub(super) const PROC_MEMINFO_INODE: u64 = 0x3005;
pub(super) const PROC_DEVICES_INODE: u64 = 0x3006;
pub(super) const PROC_STAT_INODE: u64 = 0x3007;
pub(super) const PROC_UPTIME_INODE: u64 = 0x3008;
pub(super) const PROC_PRESSURE_INODE: u64 = 0x3009;
pub(super) const PROC_PRESSURE_CPU_INODE: u64 = 0x300a;
pub(super) const PROC_PRESSURE_IO_INODE: u64 = 0x300b;
pub(super) const PROC_PRESSURE_MEMORY_INODE: u64 = 0x300c;
pub(super) const PROC_SYS_FS_INODE: u64 = 0x300d;
pub(super) const PROC_SYS_FS_FILE_MAX_INODE: u64 = 0x300e;
pub(super) const PROC_SYS_FS_NR_OPEN_INODE: u64 = 0x300f;
pub(super) const PROC_SYS_KERNEL_INODE: u64 = 0x3010;
pub(super) const PROC_SYS_KERNEL_RANDOM_INODE: u64 = 0x3011;
pub(super) const PROC_SYS_KERNEL_RANDOM_BOOT_ID_INODE: u64 = 0x3012;
pub(super) const PROC_SYS_KERNEL_HOSTNAME_INODE: u64 = 0x3013;
pub(super) const PROC_SYS_KERNEL_DOMAINNAME_INODE: u64 = 0x3014;
pub(super) const PROC_SYS_KERNEL_OSRELEASE_INODE: u64 = 0x3015;
pub(super) const PROC_SYS_KERNEL_RANDOM_UUID_INODE: u64 = 0x3016;

static PROC_UUID_COUNTER: AtomicU64 = AtomicU64::new(0);

lazy_static! {
    static ref PROC_BOOT_ID: String = generate_boot_id();
}

pub(super) fn proc_root_entries() -> Vec<DirectoryContentInfo> {
    let mut entries = vec![
        DirectoryContentInfo::new("cmdline".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("devices".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("meminfo".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("mounts".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("pressure".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("stat".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("self".into(), DirectoryContentType::Symlink),
        DirectoryContentInfo::new("sys".into(), DirectoryContentType::Directory),
        DirectoryContentInfo::new("uptime".into(), DirectoryContentType::File),
    ];

    for pid in MANAGER.lock().processes.keys() {
        entries.push(DirectoryContentInfo::new(
            format!("{}", pid.0),
            DirectoryContentType::Directory,
        ));
    }

    entries
}

pub(super) fn proc_kernel_cmdline_bytes() -> Vec<u8> {
    Vec::new()
}

pub(super) fn proc_devices_bytes() -> Vec<u8> {
    concat!(
        "Character devices:\n",
        "  1 mem\n",
        "  4 tty\n",
        "  5 /dev/tty\n",
        " 10 misc\n",
        " 13 input\n",
        " 29 fb\n",
        "136 pts\n",
        "248 rtc\n",
        "\n",
        "Block devices:\n",
        "  7 loop\n",
    )
    .as_bytes()
    .to_vec()
}

pub(super) fn proc_stat_bytes() -> Vec<u8> {
    let cpu_count = crate::smp::topology::processors().len().max(1) as u64;
    let idle_ticks = Time::since_boot().as_nanoseconds() / 10_000_000;
    let total_idle_ticks = idle_ticks.saturating_mul(cpu_count);
    let boot_time =
        crate::misc::time::unix_timestamp_seconds().saturating_sub(Time::since_boot().as_seconds());
    let process_count = MANAGER.lock().processes.len();

    let mut out = format!(
        concat!(
            "cpu  0 0 0 {} 0 0 0 0 0 0\n",
            "intr 0\n",
            "ctxt 0\n",
            "btime {}\n",
            "processes {}\n",
            "procs_running 1\n",
            "procs_blocked 0\n",
            "softirq 0 0 0 0 0 0 0 0 0 0 0\n",
        ),
        total_idle_ticks, boot_time, process_count,
    );

    for cpu_index in 0..cpu_count {
        out.push_str(&format!("cpu{cpu_index} 0 0 0 {idle_ticks} 0 0 0 0 0 0\n"));
    }

    out.into_bytes()
}

pub(super) fn proc_uptime_bytes() -> Vec<u8> {
    let uptime = Time::since_boot();
    format!(
        "{}.{:02} {}.{:02}\n",
        uptime.as_seconds(),
        uptime.subsec_milliseconds() / 10,
        uptime.as_seconds(),
        uptime.subsec_milliseconds() / 10,
    )
    .into_bytes()
}

pub(super) fn proc_kernel_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("hostname".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("domainname".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("osrelease".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("random".into(), DirectoryContentType::Directory),
    ]
}

pub(super) fn proc_kernel_random_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("boot_id".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("uuid".into(), DirectoryContentType::File),
    ]
}

pub(super) fn proc_boot_id_bytes() -> Vec<u8> {
    format!("{}\n", PROC_BOOT_ID.as_str()).into_bytes()
}

pub(super) fn proc_random_uuid_bytes() -> Vec<u8> {
    let counter = PROC_UUID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = Time::current().as_nanoseconds()
        ^ Time::since_boot().as_nanoseconds().rotate_left(19)
        ^ counter.rotate_left(7)
        ^ 0xbb67_ae85_84ca_a73b;
    format!("{}\n", generate_uuid(seed)).into_bytes()
}

pub(super) fn proc_pressure_entries() -> Vec<DirectoryContentInfo> {
    vec![
        DirectoryContentInfo::new("cpu".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("io".into(), DirectoryContentType::File),
        DirectoryContentInfo::new("memory".into(), DirectoryContentType::File),
    ]
}

fn generate_boot_id() -> String {
    generate_uuid(
        Time::current().as_nanoseconds()
            ^ Time::since_boot().as_nanoseconds().rotate_left(19)
            ^ 0x6a09_e667_f3bc_c908,
    )
}

fn generate_uuid(mut state: u64) -> String {
    let mut bytes = [0u8; 16];

    for byte in &mut bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

fn sorted_mounts() -> Vec<(String, FileSystemRef, String, MountFlags)> {
    let mut mounts = VirtualFS
        .lock()
        .mount_snapshots()
        .into_iter()
        .map(|(path, fs, source_path, flags)| {
            (path.as_string(), fs, source_path.as_string(), flags)
        })
        .collect::<Vec<_>>();
    mounts.sort_by_key(|(path, _, _, _)| (path.matches('/').count(), path.len()));
    mounts
}

pub(super) fn proc_mounts_bytes() -> Vec<u8> {
    let mut out = String::new();
    for (path, fs, _, flags) in sorted_mounts() {
        let fs = fs.lock();
        out.push_str(fs.mount_source());
        out.push(' ');
        out.push_str(&path);
        out.push(' ');
        out.push_str(fs.name());
        out.push(' ');
        out.push_str(&flags.proc_options());
        out.push_str(" 0 0\n");
    }
    out.into_bytes()
}

pub(super) fn proc_mountinfo_bytes() -> Vec<u8> {
    let mounts = sorted_mounts();
    let mut ids = BTreeMap::new();
    let mut dev_ids = BTreeMap::new();
    let mut next_dev_id = 1u64;

    for (index, (path, fs, _, _)) in mounts.iter().enumerate() {
        ids.insert(path.clone(), index as u64 + 1);

        let fs_key = format!("{:p}", alloc::sync::Arc::as_ptr(fs));
        dev_ids.entry(fs_key).or_insert_with(|| {
            let dev_id = next_dev_id;
            next_dev_id += 1;
            dev_id
        });
    }

    let mut out = String::new();
    for (path, fs, source_path, flags) in mounts {
        let id = *ids.get(&path).unwrap_or(&1);
        let parent_id = if path == "/" {
            0
        } else {
            ids.keys()
                .filter(|candidate| {
                    candidate.as_str() != path
                        && (path == format!("{}/", candidate.trim_end_matches('/'))
                            || path.starts_with(&format!("{}/", candidate.trim_end_matches('/'))))
                })
                .max_by_key(|candidate| candidate.len())
                .and_then(|candidate| ids.get(candidate))
                .copied()
                .unwrap_or(1)
        };
        let fs_key = format!("{:p}", alloc::sync::Arc::as_ptr(&fs));
        let fs = fs.lock();
        let dev_id = *dev_ids.get(&fs_key).unwrap_or(&1);
        let options = flags.proc_options();
        out.push_str(&format!(
            "{id} {parent_id} 0:{dev_id} {source_path} {path} {options} - {} {} {options}\n",
            fs.name(),
            fs.mount_source()
        ));
    }
    out.into_bytes()
}
