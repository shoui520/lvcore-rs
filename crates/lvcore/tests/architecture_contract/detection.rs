use super::common::*;

#[test]
fn driver_registry_detects_first_class_families() {
    let lved = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(lved.path());

    let multiview = tempdir().unwrap();
    fs::write(multiview.path().join("menuData.xml"), b"<menu/>").unwrap();
    fs::write(multiview.path().join("blvdat"), b"payload").unwrap();

    let hourei = tempdir().unwrap();
    fs::create_dir_all(hourei.path().join("_DataBase")).unwrap();
    fs::write(hourei.path().join("_DataBase/hore_base.db"), b"").unwrap();
    fs::write(hourei.path().join("_DataBase/hore_search_a.db"), b"").unwrap();
    fs::write(hourei.path().join("_DataBase/horejo_base.db"), b"").unwrap();

    let registry = DriverRegistry::default();
    assert_eq!(
        registry.detect(lved.path()).unwrap()[0].format_family,
        FormatFamily::LvedSqlite3
    );
    assert_eq!(
        registry.detect(multiview.path()).unwrap()[0].format_family,
        FormatFamily::LvlMultiView
    );
    assert_eq!(
        registry.detect(hourei.path()).unwrap()[0].format_family,
        FormatFamily::Hourei
    );
}

#[test]
fn driver_registry_discovers_packages_from_library_roots() {
    let root = tempdir().unwrap();
    let package = root.path().join("NestedBook");
    fs::create_dir_all(&package).unwrap();
    write_minimal_lved_sqlite_fixture(&package);

    let registry = DriverRegistry::default();
    let roots = registry
        .discover_roots(root.path(), PackageDiscoveryOptions::default())
        .unwrap();
    let detections = registry
        .detect_all(root.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert_eq!(roots, vec![package.clone()]);
    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0].root, package);
    assert_eq!(detections[0].format_family, FormatFamily::LvedSqlite3);
}

#[cfg(unix)]
#[test]
fn driver_registry_discovery_skips_symlink_cycles() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let package = root.path().join("NestedBook");
    fs::create_dir_all(&package).unwrap();
    write_minimal_lved_sqlite_fixture(&package);
    symlink(root.path(), root.path().join("Loop")).unwrap();

    let registry = DriverRegistry::default();
    let detections = registry
        .detect_all(root.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0].root, package);
    assert_eq!(detections[0].format_family, FormatFamily::LvedSqlite3);
}

#[test]
fn multiview_container_wins_over_retained_ssed_facade() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("menuData.xml"), b"<menu/>").unwrap();
    fs::write(dir.path().join("blvdat"), b"payload").unwrap();

    let registry = DriverRegistry::default();
    assert_eq!(
        registry.detect(dir.path()).unwrap()[0].format_family,
        FormatFamily::LvlMultiView
    );
    assert_eq!(
        registry
            .open_best(dir.path())
            .unwrap()
            .metadata()
            .format_family,
        FormatFamily::LvlMultiView
    );
}

#[test]
fn ssed_detection_uses_actual_idx_magic() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("fake.idx"), b"not-ssed").unwrap();
    assert!(
        DriverRegistry::default()
            .detect(dir.path())
            .unwrap()
            .is_empty()
    );

    fs::write(dir.path().join("real.IDX"), ssedinfo_fixture()).unwrap();
    assert_eq!(
        DriverRegistry::default().detect(dir.path()).unwrap()[0].format_family,
        FormatFamily::Ssed
    );
}

#[test]
fn payload_file_detection_opens_parent_package() {
    let dir = tempdir().unwrap();
    write_minimal_lved_sqlite_fixture(dir.path());
    let payload = dir.path().join("main.data");

    let package = DriverRegistry::default().open_best(&payload).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert_eq!(package.root(), dir.path());
}
