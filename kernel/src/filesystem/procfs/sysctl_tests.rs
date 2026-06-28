use super::sysctl::{
    PROC_PID_MAX, proc_c_string_bytes, proc_drop_caches_bytes, proc_fs_entries,
    proc_fs_inotify_entries, proc_pressure_bytes, proc_sys_entries, proc_sys_net_entries,
    proc_sys_net_ipv4_conf_entries, proc_sys_net_ipv4_conf_if_entries, proc_sys_net_ipv4_entries,
    proc_sysctl_value_bytes, proc_trim_sysctl_string, proc_user_entries, proc_vm_entries,
    proc_write_domainname, proc_write_drop_caches, proc_write_hostname, proc_write_pressure,
    proc_write_sysctl_u64,
};
use crate::filesystem::errors::FSError;
use crate::misc::utsname::{current_domainname, current_hostname, set_domainname, set_hostname};
use core::sync::atomic::AtomicU64;

crate::test!(
    procfs_string_helpers,
    "procfs string helpers trim sysctl values and preserve c-string bytes",
    procfs_string_helpers_trim_sysctl_values_and_preserve_c_string_bytes
);
crate::test!(
    procfs_static_entry_sets,
    "procfs static entry builders expose stable names",
    procfs_static_entry_builders_expose_stable_names
);
crate::test!(
    procfs_pressure_and_sysctl_bytes,
    "procfs pressure and sysctl rendering stay stable",
    procfs_pressure_and_sysctl_rendering_stays_stable
);
crate::test!(
    procfs_write_helpers,
    "procfs write helpers trim values update state and reject invalid inputs",
    procfs_write_helpers_trim_values_update_state_and_reject_invalid_inputs
);

fn procfs_string_helpers_trim_sysctl_values_and_preserve_c_string_bytes() {
    assert_eq!(proc_trim_sysctl_string(b" host \n\0").unwrap(), "host");
    assert!(matches!(
        proc_trim_sysctl_string(&[0xff]),
        Err(FSError::Other)
    ));

    let mut field = [0u8; 65];
    field[..4].copy_from_slice(b"host");
    assert_eq!(proc_c_string_bytes(field), b"host\n");
}

fn procfs_static_entry_builders_expose_stable_names() {
    let fs_entries = proc_fs_entries();
    assert_eq!(fs_entries.len(), 5);
    assert_eq!(fs_entries[0].name, "file-max");
    assert_eq!(fs_entries[1].name, "inotify");
    assert_eq!(fs_entries[2].name, "nr_open");
    assert_eq!(fs_entries[3].name, "pipe-max-size");
    assert_eq!(fs_entries[4].name, "pipe-user-pages-soft");

    let inotify_entries = proc_fs_inotify_entries();
    assert_eq!(inotify_entries.len(), 3);
    assert_eq!(inotify_entries[0].name, "max_queued_events");
    assert_eq!(inotify_entries[1].name, "max_user_instances");
    assert_eq!(inotify_entries[2].name, "max_user_watches");

    let sys_entries = proc_sys_entries();
    assert_eq!(sys_entries.len(), 5);
    assert_eq!(sys_entries[0].name, "fs");
    assert_eq!(sys_entries[1].name, "kernel");
    assert_eq!(sys_entries[2].name, "net");
    assert_eq!(sys_entries[3].name, "user");
    assert_eq!(sys_entries[4].name, "vm");

    let user_entries = proc_user_entries();
    assert_eq!(user_entries.len(), 1);
    assert_eq!(user_entries[0].name, "max_user_namespaces");

    let net_entries = proc_sys_net_entries();
    assert_eq!(net_entries.len(), 1);
    assert_eq!(net_entries[0].name, "ipv4");

    let ipv4_entries = proc_sys_net_ipv4_entries();
    assert_eq!(ipv4_entries.len(), 1);
    assert_eq!(ipv4_entries[0].name, "conf");

    let conf_entries = proc_sys_net_ipv4_conf_entries();
    assert_eq!(conf_entries.len(), 2);
    assert_eq!(conf_entries[0].name, "default");
    assert_eq!(conf_entries[1].name, "lo");

    let if_entries = proc_sys_net_ipv4_conf_if_entries();
    assert_eq!(if_entries.len(), 1);
    assert_eq!(if_entries[0].name, "tag");

    let vm_entries = proc_vm_entries();
    assert_eq!(vm_entries.len(), 5);
    assert_eq!(vm_entries[0].name, "compact_memory");
    assert_eq!(vm_entries[1].name, "drop_caches");
    assert_eq!(vm_entries[2].name, "nr_hugepages");
    assert_eq!(vm_entries[3].name, "hugetlb_shm_group");
    assert_eq!(vm_entries[4].name, "overcommit_memory");

    assert_eq!(proc_sysctl_value_bytes(&PROC_PID_MAX), b"4194304\n");
}

fn procfs_pressure_and_sysctl_rendering_stays_stable() {
    let rendered = proc_pressure_bytes();
    assert!(
        core::str::from_utf8(&rendered)
            .unwrap()
            .contains("some avg10=0.00")
    );
    let value = AtomicU64::new(1234);
    assert_eq!(proc_sysctl_value_bytes(&value), b"1234\n");
}

fn procfs_write_helpers_trim_values_update_state_and_reject_invalid_inputs() {
    let hostname_before = current_hostname(crate::NAME);
    let domain_before = current_domainname("(none)");

    assert_eq!(proc_write_hostname(b" proc-host \n").unwrap(), 12);
    assert_eq!(
        proc_c_string_bytes(current_hostname(crate::NAME)),
        b"proc-host\n"
    );
    assert!(matches!(proc_write_hostname(&[0xff]), Err(FSError::Other)));
    set_hostname(
        proc_trim_sysctl_string(&proc_c_string_bytes(hostname_before))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();

    assert_eq!(proc_write_domainname(b" domain.test \n").unwrap(), 14);
    assert_eq!(
        proc_c_string_bytes(current_domainname("(none)")),
        b"domain.test\n"
    );
    assert!(matches!(
        proc_write_domainname(&[0xff]),
        Err(FSError::Other)
    ));
    set_domainname(
        proc_trim_sysctl_string(&proc_c_string_bytes(domain_before))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();

    let value = AtomicU64::new(7);
    assert_eq!(proc_write_sysctl_u64(&value, b" 123 \0").unwrap(), 6);
    assert_eq!(proc_sysctl_value_bytes(&value), b"123\n");
    assert!(matches!(
        proc_write_sysctl_u64(&value, b"not-a-number"),
        Err(FSError::Other)
    ));

    assert_eq!(proc_write_pressure(b"some 100 1000").unwrap(), 13);
    assert_eq!(proc_drop_caches_bytes(), b"0\n");
    assert_eq!(proc_write_drop_caches(b"3\n").unwrap(), 2);
    assert!(matches!(
        proc_write_drop_caches(b"4\n"),
        Err(FSError::InvalidArguments)
    ));
}
