use super::*;
use lazy_static::lazy_static;

#[derive(Clone)]
pub(super) struct CgroupDirectory {
    pub(super) inode: u64,
    pub(super) children: BTreeSet<String>,
    pub(super) subtree_control: String,
    pub(super) cpu_max: String,
    pub(super) memory_oom_group: bool,
    pub(super) memory_min: String,
    pub(super) memory_low: String,
    pub(super) memory_high: String,
    pub(super) memory_max: String,
    pub(super) memory_swap_max: String,
    pub(super) pids_max: String,
}

pub(super) struct CgroupState {
    next_inode: u64,
    pub(super) directories: BTreeMap<String, CgroupDirectory>,
    pid_paths: BTreeMap<u64, String>,
}

impl CgroupState {
    pub(super) fn new() -> Self {
        let mut directories = BTreeMap::new();
        directories.insert(
            "/".into(),
            CgroupDirectory {
                inode: ROOT_INODE,
                children: BTreeSet::new(),
                subtree_control: String::new(),
                cpu_max: String::from("max 100000"),
                memory_oom_group: false,
                memory_min: String::from("0"),
                memory_low: String::from("0"),
                memory_high: String::from("max"),
                memory_max: String::from("max"),
                memory_swap_max: String::from("max"),
                pids_max: String::from("max"),
            },
        );

        Self {
            next_inode: ROOT_INODE + 1,
            directories,
            pid_paths: BTreeMap::new(),
        }
    }

    pub(super) fn normalize_dir_path(path: &str) -> String {
        if path.is_empty() || path == "/" {
            "/".into()
        } else {
            Path::new(path).normalize().as_string()
        }
    }

    pub(super) fn child_path(parent: &str, name: &str) -> String {
        if parent == "/" {
            format!("/{name}")
        } else {
            format!("{parent}/{name}")
        }
    }

    pub(super) fn directory(&self, path: &str) -> FSResult<&CgroupDirectory> {
        self.directories.get(path).ok_or(FSError::NotFound)
    }

    pub(super) fn directory_mut(&mut self, path: &str) -> FSResult<&mut CgroupDirectory> {
        self.directories.get_mut(path).ok_or(FSError::NotFound)
    }

    pub(super) fn create_directory(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize_dir_path(parent);
        let child_path = Self::child_path(&parent, name);

        if self.directories.contains_key(&child_path) {
            return Err(FSError::AlreadyExists);
        }

        self.directory(&parent)?;

        let inode = self.next_inode;
        self.next_inode += 1;
        self.directories.insert(
            child_path,
            CgroupDirectory {
                inode,
                children: BTreeSet::new(),
                subtree_control: String::new(),
                cpu_max: String::from("max 100000"),
                memory_oom_group: false,
                memory_min: String::from("0"),
                memory_low: String::from("0"),
                memory_high: String::from("max"),
                memory_max: String::from("max"),
                memory_swap_max: String::from("max"),
                pids_max: String::from("max"),
            },
        );
        self.directory_mut(&parent)?.children.insert(name.into());
        Ok(())
    }

    pub(super) fn remove_directory(&mut self, parent: &str, name: &str) -> FSResult<()> {
        let parent = Self::normalize_dir_path(parent);
        let child_path = Self::child_path(&parent, name);
        self.prune_dead_pid_paths();
        let Some(directory) = self.directories.get(&child_path) else {
            return Err(FSError::NotFound);
        };
        if !directory.children.is_empty() {
            return Err(FSError::DirectoryNotEmpty);
        }
        if self
            .pid_paths
            .values()
            .any(|path| Self::normalize_dir_path(path) == child_path)
        {
            return Err(FSError::Busy);
        }

        self.directories.remove(&child_path);
        self.directory_mut(&parent)?.children.remove(name);
        Ok(())
    }

    pub(super) fn pid_path(&self, pid: ProcessID) -> String {
        self.pid_paths
            .get(&pid.0)
            .cloned()
            .unwrap_or_else(|| "/".into())
    }

    pub(super) fn set_pid_path(&mut self, pid: ProcessID, path: &str) -> FSResult<()> {
        let path = Self::normalize_dir_path(path);
        self.directory(&path)?;
        self.pid_paths.insert(pid.0, path);
        Ok(())
    }

    pub(super) fn remove_pid_path(&mut self, pid: ProcessID) {
        self.pid_paths.remove(&pid.0);
    }

    fn prune_dead_pid_paths(&mut self) {
        let live_pids = MANAGER
            .lock()
            .processes
            .keys()
            .map(|pid| pid.0)
            .collect::<BTreeSet<_>>();
        self.pid_paths.retain(|pid, _| live_pids.contains(pid));
    }

    pub(super) fn pids_in_path(&self, path: &str) -> Vec<ProcessID> {
        let path = Self::normalize_dir_path(path);
        let pids = MANAGER.lock().processes.keys().copied().collect::<Vec<_>>();
        pids.into_iter()
            .filter(|pid| self.pid_path(*pid) == path)
            .collect()
    }
}

lazy_static! {
    pub(super) static ref CGROUP_STATE: Mut<CgroupState> = Mut::new(CgroupState::new());
}
