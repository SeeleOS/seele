use alloc::{string::String, sync::Arc};

use crate::{
    filesystem::{path::Path, vfs::VirtualFS},
    object::misc::ObjectRef,
    process::manager::get_current_process,
};

fn open_as_object(path: Path) -> Option<ObjectRef> {
    VirtualFS
        .lock()
        .open(path)
        .ok()
        .map(Arc::new)
        .map(|f| f as ObjectRef)
}

pub fn smart_navigate(
    path: String,
    object: ObjectRef,
    // Start the path with the current directory
    start_from_current_dir: bool,
    // Just navigate to the object without doing anything else
    use_object: bool,
) -> Option<ObjectRef> {
    match use_object {
        false => {
            if path.starts_with('/') {
                // Just use the path and ignore other stuff
                // if its a absolute path
                let path = Path::new(&path);
                open_as_object(path)
            } else if start_from_current_dir {
                let mut cur_path = get_current_process().lock().current_directory.clone();
                cur_path.push_path_str(&path);
                open_as_object(cur_path.as_normal())
            } else {
                None
            }
        }
        true => Some(object),
    }
}
