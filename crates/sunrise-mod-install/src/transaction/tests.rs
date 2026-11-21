use super::*;

#[test]
fn interrupted_swap_restores_old_files_and_does_not_consume_the_previous_step() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let interrupted = root.join(JOURNAL);
    fs::create_dir(&interrupted).unwrap();
    fs::create_dir(interrupted.join("old")).unwrap();
    fs::create_dir(interrupted.join("new")).unwrap();
    fs::create_dir(root.join(PREVIOUS_DIR)).unwrap();
    fs::write(
        root.join(PREVIOUS_DIR).join(CONFIG),
        b"before earlier change",
    )
    .unwrap();
    fs::write(interrupted.join("old").join(CONFIG), b"before this change").unwrap();
    fs::write(root.join(CONFIG), b"half swapped").unwrap();
    fs::write(root.join(lock::LOCK), b"never published").unwrap();
    let mut doc = Move {
        old: [false; FILES.len()],
        new: [false; FILES.len()],
        keep_undo: true,
    };
    doc.old[0] = true;
    doc.new[0] = true;
    doc.new[1] = true;
    fs::write(
        interrupted.join("move.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    assert!(read_lease(root).is_err());
    let folder = ServerFolder::open(root).unwrap();
    assert!(folder.recover().unwrap());
    assert_eq!(fs::read(root.join(CONFIG)).unwrap(), b"before this change");
    assert!(!root.join(lock::LOCK).exists());
    assert_eq!(
        fs::read(root.join(PREVIOUS_DIR).join(CONFIG)).unwrap(),
        b"before earlier change"
    );
    assert!(!folder.recover().unwrap());
}
