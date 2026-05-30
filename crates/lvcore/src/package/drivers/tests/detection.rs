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
