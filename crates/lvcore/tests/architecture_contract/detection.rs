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

#[test]
fn driver_registry_exposes_cacheable_package_candidates_without_opening() {
    let root = tempdir().unwrap();
    let package = root.path().join("NestedBook");
    fs::create_dir_all(&package).unwrap();
    write_minimal_lved_sqlite_fixture(&package);

    let candidates = DriverRegistry::default()
        .discover_package_candidates(root.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].root, package);
    assert_eq!(candidates[0].format_family, FormatFamily::LvedSqlite3);
    assert_eq!(candidates[0].format_label, "LVED_SQLITE3");
    assert!(candidates[0].title_hint.is_none());
    assert_eq!(candidates[0].root_fingerprint.len(), 64);
}

#[test]
fn package_candidate_discovery_deduplicates_same_book_fingerprint() {
    let root = tempdir().unwrap();
    let upper = root.path().join("_DCT_GEN2013");
    let mixed = root.path().join("_DCT_Gen2013");
    fs::create_dir_all(&upper).unwrap();
    fs::create_dir_all(&mixed).unwrap();
    fs::write(upper.join("Gen2013.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(mixed.join("Gen2013.IDX"), ssedinfo_fixture()).unwrap();

    let candidates = DriverRegistry::default()
        .discover_package_candidates(root.path(), PackageDiscoveryOptions::default())
        .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].format_family, FormatFamily::Ssed);
    assert_eq!(candidates[0].format_label, "SSED");
}

#[test]
fn driver_registry_does_not_treat_resource_stores_as_packages() {
    let root = tempdir().unwrap();
    let package = root.path().join("BookWithResources");
    fs::create_dir_all(&package).unwrap();
    write_minimal_lved_sqlite_fixture(&package);

    let nested_resource_store = package.join("resource");
    fs::create_dir_all(&nested_resource_store).unwrap();
    fs::write(
        nested_resource_store.join("retained.IDX"),
        ssedinfo_fixture(),
    )
    .unwrap();

    let sibling_resource_store = root.path().join("res");
    fs::create_dir_all(&sibling_resource_store).unwrap();
    fs::write(
        sibling_resource_store.join("retained.IDX"),
        ssedinfo_fixture(),
    )
    .unwrap();

    let registry = DriverRegistry::default();
    let detections = registry
        .detect_all(root.path(), PackageDiscoveryOptions::default())
        .unwrap();

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
