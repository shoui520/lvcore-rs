use std::fs;

use super::*;

#[test]
fn detects_multiview_by_menu_and_payload() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        br#"<list><item label="Visible Title" /></list>"#,
    )
    .unwrap();
    fs::write(dir.path().join("blvdat"), b"payload").unwrap();

    let detected = LvlMultiViewDriver.detect(dir.path()).unwrap().unwrap();
    assert_eq!(detected.format_family, FormatFamily::LvlMultiView);
    assert_eq!(detected.title.as_deref(), Some("Visible Title"));
}

#[cfg(unix)]
#[test]
fn multiview_detection_ignores_symlinked_payload_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(
        dir.path().join("menuData.xml"),
        br#"<list><item label="Visible Title" /></list>"#,
    )
    .unwrap();
    fs::write(outside.path().join("blvdat"), b"payload").unwrap();
    symlink(outside.path().join("blvdat"), dir.path().join("blvdat")).unwrap();

    assert!(LvlMultiViewDriver.detect(dir.path()).unwrap().is_none());
}

#[test]
fn detects_hourei_by_core_databases() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("_DataBase")).unwrap();
    fs::write(dir.path().join("_DataBase/hore_base.db"), b"").unwrap();
    fs::write(dir.path().join("_DataBase/hore_search_a.db"), b"").unwrap();
    fs::write(dir.path().join("_DataBase/horejo_base.db"), b"").unwrap();

    let detected = HoureiDriver.detect(dir.path()).unwrap().unwrap();
    assert_eq!(detected.format_family, FormatFamily::Hourei);
}

#[cfg(unix)]
#[test]
fn hourei_detection_ignores_symlinked_core_database_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("_DataBase")).unwrap();
    fs::create_dir_all(outside.path().join("_DataBase")).unwrap();
    for name in ["hore_base.db", "hore_search_a.db", "horejo_base.db"] {
        fs::write(outside.path().join("_DataBase").join(name), b"").unwrap();
        symlink(
            outside.path().join("_DataBase").join(name),
            dir.path().join("_DataBase").join(name),
        )
        .unwrap();
    }

    assert!(HoureiDriver.detect(dir.path()).unwrap().is_none());
}
