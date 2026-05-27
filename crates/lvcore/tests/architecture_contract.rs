use std::fs;
use std::path::Path;

use lvcore::{
    Capability, DriverRegistry, FormatFamily, GaijiPolicy, GaijiSourcePreference, InternalTarget,
    NavigationStatus, NavigationSurfaceKind, RenderOptions, SSEDINFO_MAGIC, StorageBackend,
    TargetToken, VisualBody,
};
use tempfile::tempdir;

#[test]
fn driver_registry_detects_first_class_families() {
    let lved = tempdir().unwrap();
    fs::write(lved.path().join("main.data"), b"encrypted").unwrap();
    fs::write(lved.path().join("dict.key"), b"key").unwrap();

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
fn target_tokens_are_frontend_safe_and_round_trippable() {
    let target = InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 42,
        anchor: Some("main".to_owned()),
    };
    let token = TargetToken::new(&target).unwrap();
    assert_eq!(token.decode().unwrap(), target);
}

#[test]
fn ssed_home_surfaces_are_capability_based() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("MENU.DIC"), b"").unwrap();
    fs::write(dir.path().join("Panels.xml"), b"").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let metadata = package.metadata();
    assert_eq!(metadata.format_family, FormatFamily::Ssed);
    assert!(metadata.capabilities.contains(&Capability::HcRenderInput));

    let surfaces = package.home_surfaces().unwrap();
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Menu && surface.status == NavigationStatus::Available
    }));
    assert!(surfaces.iter().any(|surface| {
        surface.kind == NavigationSurfaceKind::Panel
            && surface.status == NavigationStatus::Available
    }));
}

#[test]
fn dense_honmon_targets_do_not_render_as_raw_numeric_anchors() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedDenseAnchor {
        anchor: "00100050".to_owned(),
        resolver_hint: Some("vlpljbl".to_owned()),
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    assert!(matches!(body, VisualBody::Unsupported { .. }));
    assert!(!serde_json::to_string(&body).unwrap().contains("00100050"));

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(view.display_html.is_none());
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "dense_honmon_deferred")
    );
}

#[test]
fn continuous_view_api_returns_typed_deferred_window() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let window = package
        .resolve_target_window(&token, None, 2, 3, &RenderOptions::default())
        .unwrap();
    assert!(window.before.is_empty());
    assert!(window.after.is_empty());
    assert!(
        window
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "sequence_deferred")
    );
}

#[test]
fn gaiji_policy_is_backend_owned_and_reorderable() {
    let policy = GaijiPolicy {
        priority: vec![
            GaijiSourcePreference::ExternalResource,
            GaijiSourcePreference::Unicode,
            GaijiSourcePreference::Ga16Bitmap,
        ],
    };
    assert_eq!(policy.priority[0], GaijiSourcePreference::ExternalResource);
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
    let payload = dir.path().join("main.data");
    fs::write(&payload, b"encrypted").unwrap();
    fs::write(dir.path().join("dict.key"), b"key").unwrap();

    let package = DriverRegistry::default().open_best(&payload).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::LvedSqlite3);
    assert_eq!(package.root(), dir.path());
}

#[test]
fn casefolded_paths_find_real_casing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("HONMON.DIN"), b"body").unwrap();
    let storage = lvcore::DirectoryStorage::new(dir.path());
    let resolved = storage
        .resolve_casefolded(Path::new("honmon.din"))
        .unwrap()
        .unwrap();
    assert_eq!(resolved.file_name().unwrap(), "HONMON.DIN");
}

fn ssedinfo_fixture() -> Vec<u8> {
    let record_start = 0x80;
    let mut data = vec![0u8; record_start + 5 * 0x30];
    data[..8].copy_from_slice(SSEDINFO_MAGIC);
    let title = b"Fixture Book";
    data[0x0c] = title.len() as u8;
    data[0x0d..0x0d + title.len()].copy_from_slice(title);
    data[0x4d] = 5;
    write_record(
        &mut data[record_start..record_start + 0x30],
        0x00,
        1,
        10,
        "HONMON.DIC",
    );
    write_record(
        &mut data[record_start + 0x30..record_start + 0x60],
        0x01,
        11,
        12,
        "MENU.DIC",
    );
    write_record(
        &mut data[record_start + 0x60..record_start + 0x90],
        0x05,
        13,
        14,
        "FHTITLE.DIC",
    );
    write_record(
        &mut data[record_start + 0x90..record_start + 0xc0],
        0x91,
        15,
        16,
        "FHINDEX.DIC",
    );
    write_record(
        &mut data[record_start + 0xc0..record_start + 0xf0],
        0xf2,
        17,
        18,
        "GA16HALF",
    );
    data
}

fn write_record(rec: &mut [u8], component_type: u8, start: u32, end: u32, filename: &str) {
    rec[3] = component_type;
    rec[4..8].copy_from_slice(&start.to_be_bytes());
    rec[8..12].copy_from_slice(&end.to_be_bytes());
    rec[0x10] = filename.len() as u8;
    rec[0x11..0x11 + filename.len()].copy_from_slice(filename.as_bytes());
}
