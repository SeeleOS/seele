use spin::Mutex;

static HOSTNAME: Mutex<Option<[u8; 65]>> = Mutex::new(None);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}

impl Default for UtsName {
    fn default() -> Self {
        Self {
            sysname: [0; 65],
            nodename: [0; 65],
            release: [0; 65],
            version: [0; 65],
            machine: [0; 65],
            domainname: [0; 65],
        }
    }
}

impl UtsName {
    pub fn new(sysname: &str, release: &str, version: &str, machine: &str) -> Self {
        let mut uts = Self::default();
        write_c_field(&mut uts.sysname, sysname.as_bytes());
        write_c_field(&mut uts.release, release.as_bytes());
        write_c_field(&mut uts.version, version.as_bytes());
        write_c_field(&mut uts.machine, machine.as_bytes());
        uts
    }
}

pub fn current_hostname(default: &str) -> [u8; 65] {
    let hostname = HOSTNAME.lock();
    if let Some(hostname) = *hostname {
        hostname
    } else {
        let mut field = [0; 65];
        write_c_field(&mut field, default.as_bytes());
        field
    }
}

pub fn set_hostname(hostname: &[u8]) -> Result<(), ()> {
    if hostname.len() > 64 || hostname.contains(&0) {
        return Err(());
    }

    let mut field = [0; 65];
    write_c_field(&mut field, hostname);
    *HOSTNAME.lock() = Some(field);
    Ok(())
}

fn write_c_field(dst: &mut [u8], src: &[u8]) {
    let len = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let len = len.min(dst.len().saturating_sub(1));
    dst[..len].copy_from_slice(&src[..len]);
}
