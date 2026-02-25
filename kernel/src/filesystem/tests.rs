
use crate::test;

test!("VFS Basic", || {
    let a_txt = Path::new("/test/vfs_create.txt");
    VirtualFS.lock().create_file(a_txt.clone()).unwrap();
    VirtualFS
        .lock()
        .write_file(
            a_txt.clone(),
            FileData {
                content: "abc".to_string(),
            },
        )
        .unwrap();
    let content = VirtualFS.lock().read_file(a_txt.clone()).unwrap().content;

    assert_eq!(content, "abc");
});

test!("VFS Create Dir", || {
    let a_txt = Path::new("/test/vfs_dir");
    VirtualFS.lock().create_dir(a_txt.clone()).unwrap();
});

test!("VFS List Contents", || {
    let a = Path::new("/tests/dir/a.txt");
    let b = Path::new("/tests/dir/b.txt");
    let c = Path::new("/tests/dir/c.txt");
    VirtualFS.lock().create_file(a.clone()).unwrap();
    VirtualFS.lock().create_file(b.clone()).unwrap();
    VirtualFS.lock().create_file(c.clone()).unwrap();
    VirtualFS.lock().write_file(
        a,
        FileData {
            content: "gwergwegf".to_string(),
        },
    );

    let content = VirtualFS
        .lock()
        .list_contents(Path::new("/tests/dir"))
        .unwrap();

    assert!({
        content.contains(&"a.txt".to_string())
            && content.contains(&"b.txt".to_string())
            && content.contains(&"c.txt".to_string())
    });
});
