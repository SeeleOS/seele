use alloc::{string::String, sync::Arc};

use crate::{
    filesystem::{absolute_path::AbsolutePath, path::Path, vfs::VirtualFS},
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

pub fn smart_resolve_path(
    path: String,
    // Start the path with the current directory
    start_from_current_dir: bool,
) -> Option<Path> {
    let path = Path::new(&path);
    let process = get_current_process();
    let fs_context = process.lock().fs_context.lock().clone();

    if path.is_absolute() {
        Some(
            AbsolutePath::join_under_root(
                &fs_context.root_directory,
                &fs_context.current_directory,
                &path,
            )
            .as_normal(),
        )
    } else if start_from_current_dir {
        let mut cur_path = fs_context.current_directory;
        cur_path.push_path_str(&path.as_string());
        Some(cur_path.as_normal())
    } else {
        None
    }
}

pub fn smart_navigate(
    path: String,
    object: ObjectRef,
    // Start the path with the current directory
    start_from_current_dir: bool,
    // Just navigate to the object without doing anything else
    use_object: bool,
) -> Option<ObjectRef> {
    if use_object {
        Some(object)
    } else {
        smart_resolve_path(path, start_from_current_dir).and_then(open_as_object)
    }
}
