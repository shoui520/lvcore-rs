use super::common::*;

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
fn ssed_gaiji_resolution_honors_policy_and_keeps_fallbacks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    fs::write(dir.path().join("GA16HALF"), ga16_fixture(0xA121, 8)).unwrap();
    fs::write(dir.path().join("GA16FULL"), ga16_fixture(0xB121, 3)).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let default = package.resolve_gaiji("B123", &GaijiPolicy::default());
    assert_eq!(default.identity, "B123");
    assert_eq!(
        default.preferred_source,
        Some(GaijiSourcePreference::Unicode)
    );
    assert_eq!(default.unicode.as_deref(), Some("一"));
    assert_eq!(
        default.resource.as_ref().unwrap().kind,
        ResourceKind::Template
    );

    let image_first = package.resolve_gaiji(
        "<zB123>",
        &GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
                GaijiSourcePreference::Ga16Bitmap,
            ],
        },
    );
    assert_eq!(
        image_first.preferred_source,
        Some(GaijiSourcePreference::ExternalResource)
    );
    assert_eq!(image_first.unicode.as_deref(), Some("一"));
    assert_eq!(
        image_first.resource.as_ref().unwrap().label.as_deref(),
        Some("B123.SVG")
    );
    let bitmap_first = package.resolve_gaiji(
        "B123",
        &GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::ExternalResource,
                GaijiSourcePreference::Unicode,
            ],
        },
    );
    assert_eq!(
        bitmap_first.preferred_source,
        Some(GaijiSourcePreference::Ga16Bitmap)
    );
    assert_eq!(
        bitmap_first.resource.as_ref().unwrap().label.as_deref(),
        Some("GA16FULL:B123")
    );
    assert_eq!(
        bitmap_first.resource.as_ref().unwrap().mime_type.as_deref(),
        Some("image/png")
    );
    let bitmap_first_png = package
        .read_resource(&bitmap_first.resource.as_ref().unwrap().token)
        .unwrap();
    assert!(bitmap_first_png.starts_with(b"\x89PNG\r\n\x1a\n"));

    let bitmap = package.resolve_gaiji("A128", &GaijiPolicy::default());
    assert_eq!(
        bitmap.preferred_source,
        Some(GaijiSourcePreference::Ga16Bitmap)
    );
    assert!(bitmap.unicode.is_none());
    assert_eq!(
        bitmap.resource.as_ref().unwrap().label.as_deref(),
        Some("GA16HALF:A128")
    );
    assert_eq!(
        bitmap.resource.as_ref().unwrap().mime_type.as_deref(),
        Some("image/png")
    );
    let bitmap_png = package
        .read_resource(&bitmap.resource.as_ref().unwrap().token)
        .unwrap();
    assert!(bitmap_png.starts_with(b"\x89PNG\r\n\x1a\n"));

    let unresolved = package.resolve_gaiji("B999", &GaijiPolicy::default());
    assert_eq!(
        unresolved.preferred_source,
        Some(GaijiSourcePreference::Unresolved)
    );
    assert!(unresolved.resource.is_none());
    assert!(unresolved.unicode.is_none());
    assert!(
        unresolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "gaiji_unresolved")
    );
}

#[test]
fn ssed_template_resources_can_live_in_package_adjacent_templates_directory() {
    let root = tempdir().unwrap();
    let package_root = root.path().join("IWKOKU7N");
    let sibling_templates_root = root.path().join("IWKOKU7N_Templates");
    fs::create_dir(&package_root).unwrap();
    fs::create_dir(&sibling_templates_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(sibling_templates_root.join("B123.SVG"), b"<svg/>").unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let resolved = package.resolve_gaiji("B123", &GaijiPolicy::default());
    assert_eq!(
        resolved.preferred_source,
        Some(GaijiSourcePreference::ExternalResource)
    );
    let resource = resolved.resource.unwrap();
    assert_eq!(resource.kind, ResourceKind::Template);
    assert!(resource.href.is_some());

    let token = ResourceToken::new(&InternalResource::PackageFile {
        path: "templates/b123.svg".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();
    assert_eq!(package.read_resource(&token).unwrap(), b"<svg/>");
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

#[test]
fn package_file_resources_resolve_and_read_with_preserved_casing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = ResourceToken::new(&InternalResource::PackageFile {
        path: "templates/b123.svg".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert_eq!(resource.kind, ResourceKind::Template);
    assert_eq!(resource.label.as_deref(), Some("B123.SVG"));
    assert!(
        resource
            .href
            .as_deref()
            .unwrap_or_default()
            .starts_with("lvcore://resource/")
    );
    assert!(resource.diagnostics.is_empty());
    assert_eq!(package.read_resource(&token).unwrap(), b"<svg/>");
}

#[cfg(unix)]
#[test]
fn package_file_resource_symlink_escape_is_not_resolvable() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    let outside = dir.path().with_file_name("outside-lvcore-resource.svg");
    fs::write(&outside, b"<svg/>").unwrap();
    symlink(&outside, dir.path().join("Templates/Escape.SVG")).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = ResourceToken::new(&InternalResource::PackageFile {
        path: "Templates/Escape.SVG".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert!(resource.href.is_none());
    assert!(!resource.diagnostics.is_empty());
    assert!(package.read_resource(&token).is_err());

    fs::remove_file(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn adjacent_templates_resource_symlink_escape_is_not_resolvable() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let package_root = root.path().join("Book");
    let templates_root = root.path().join("Book_Templates");
    fs::create_dir_all(&package_root).unwrap();
    fs::create_dir_all(&templates_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let outside = root.path().join("outside-lvcore-adjacent-resource.svg");
    fs::write(&outside, b"<svg/>").unwrap();
    symlink(&outside, templates_root.join("B123.SVG")).unwrap();
    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let token = ResourceToken::new(&InternalResource::PackageFile {
        path: "Templates/B123.SVG".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();

    let resource = package.resolve_resource(&token).unwrap();
    assert!(resource.href.is_none());
    assert!(!resource.diagnostics.is_empty());
    assert!(package.read_resource(&token).is_err());
}

#[cfg(unix)]
#[test]
fn ga16_gaiji_symlink_escape_is_not_resolved() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let outside = dir.path().with_file_name("outside-lvcore-ga16");
    fs::write(&outside, ga16_fixture(0xB121, 1)).unwrap();
    symlink(&outside, dir.path().join("GA16FULL")).unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let resolved = package.resolve_gaiji(
        "B121",
        &GaijiPolicy {
            priority: vec![
                GaijiSourcePreference::Ga16Bitmap,
                GaijiSourcePreference::Unresolved,
            ],
        },
    );

    assert_eq!(
        resolved.preferred_source,
        Some(GaijiSourcePreference::Unresolved)
    );
    assert!(resolved.resource.is_none());

    fs::remove_file(outside).unwrap();
}

#[cfg(unix)]
#[test]
fn adjacent_panel_bin_symlink_escape_is_not_decoded() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let package_root = root.path().join("Book");
    let panel_root = root.path().join("Book_Panel");
    fs::create_dir_all(&package_root).unwrap();
    fs::create_dir_all(&panel_root).unwrap();
    fs::write(package_root.join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        package_root.join("Panels.xml"),
        r#"<panels>
  <panel index="01010000" paneltype="contents">
    <title>あ</title>
    <data type="bin" filename="Panel\All-A.bin" />
  </panel>
</panels>"#,
    )
    .unwrap();
    let outside = root.path().join("outside-panel-bin");
    fs::write(&outside, panel_bin_fixture(10, 2)).unwrap();
    symlink(&outside, panel_root.join("All-A.bin")).unwrap();

    let package = DriverRegistry::default().open_best(&package_root).unwrap();
    let surface = package.open_surface("panels:01010000").unwrap();

    let NavigationSurface::Deferred { diagnostics, .. } = surface else {
        panic!("escaped sibling Panel BIN must not decode into a panel surface");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "ssed_panel_bin_missing" || diagnostic.code == "ssed_panels_empty"
    }));

    fs::remove_file(outside).unwrap();
}

#[test]
fn resource_targets_render_as_media_resource_views() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::create_dir(dir.path().join("Templates")).unwrap();
    fs::write(dir.path().join("Templates/B123.SVG"), b"<svg/>").unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let resource = ResourceToken::new(&InternalResource::PackageFile {
        path: "Templates/B123.SVG".to_owned(),
        resource_kind: ResourceKind::Template,
    })
    .unwrap();
    let target = TargetToken::new(&InternalTarget::Resource {
        resource,
        anchor: None,
    })
    .unwrap();

    let view = package
        .render_target(&target, &RenderOptions::default())
        .unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::MediaResource);
    assert_eq!(view.resources.len(), 1);
    assert_eq!(view.resources[0].kind, ResourceKind::Template);
    assert!(view.resources[0].href.is_some());
}
