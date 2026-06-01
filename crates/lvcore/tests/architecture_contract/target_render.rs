use super::common::*;

#[test]
fn target_tokens_are_frontend_safe_and_round_trippable() {
    let target = InternalTarget::LvedRow {
        table: "content".to_owned(),
        row_id: 42,
        anchor: Some("main".to_owned()),
        query: None,
    };
    let token = TargetToken::new(&target).unwrap();
    assert_eq!(token.decode().unwrap(), target);
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

    let input = package.renderer_input_for_target(&token).unwrap();
    assert!(matches!(input, RendererInput::Unsupported { .. }));
    assert!(!serde_json::to_string(&input).unwrap().contains("00100050"));

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();
    assert!(view.display_html.is_none());
    assert_eq!(view.kind, ResolvedTargetKind::Unsupported);
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_dense_sidecar_not_found")
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
fn ssed_address_targets_resolve_through_catalog_components() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "honmon.dic".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    assert_eq!(
        body,
        VisualBody::SsedStream {
            component: "HONMON.DIC".to_owned(),
            offset: 2,
            length: None,
        }
    );
}

#[test]
fn render_target_uses_resolved_visual_body_contract() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(b"abcdef"),
    )
    .unwrap();
    fs::write(dir.path().join("HC0158.dll"), b"").unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();
    let options = RenderOptions {
        include_debug_trace: true,
        ..RenderOptions::default()
    };

    let input = package.renderer_input_for_target(&token).unwrap();
    assert_eq!(input.target(), &token);
    assert_eq!(input.kind(), lvcore::RendererInputKind::HcSsedStream);
    let RendererInput::HcSsedStream {
        component,
        offset,
        length,
        profile_hint,
        hc_profile,
        diagnostics,
        ..
    } = input
    else {
        panic!("SSED stream must become explicit HC renderer input");
    };
    assert_eq!(component, "HONMON.DIC");
    assert_eq!(offset, 2);
    assert_eq!(length, None);
    assert_eq!(profile_hint.as_deref(), Some("HC0158"));
    let hc_profile = hc_profile.expect("HC DLL profile metadata should be present");
    assert_eq!(hc_profile.profile_id, "HC0158");
    assert_eq!(hc_profile.source, lvcore::HcRendererProfileSource::HcDll);
    assert_eq!(
        hc_profile.status,
        lvcore::HcRendererProfileStatus::InputOnly
    );
    assert_eq!(hc_profile.dll_size, Some(0));
    assert_eq!(
        hc_profile.dll_sha256.as_deref(),
        Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_renderer_input_ready")
    );

    let view = package.render_target(&token, &options).unwrap();
    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        view.display_html
            .as_deref()
            .is_some_and(|html| html.contains("lv-hc-common-html-fallback"))
    );
    assert!(
        view.basic_text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
    );
    assert!(
        view.capabilities
            .contains(&lvcore::RenderCapability::HcRenderInput)
    );
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_render_common_html_fallback")
    );
    let debug_trace = view.debug_trace.as_deref().unwrap_or_default();
    assert!(debug_trace.contains("HONMON.DIC"));
    assert!(debug_trace.contains("\"offset\":2"));
    assert!(debug_trace.contains("HC0158"));
    assert!(
        debug_trace.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    );
}

#[test]
fn basic_text_mode_decodes_hc_ssed_stream_instead_of_returning_empty_deferred_view() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    let mut honmon = body_jis("見出し");
    honmon.extend_from_slice(&[0x1f, 0x0a]);
    honmon.extend_from_slice(&[0x1f, 0x04]);
    honmon.extend_from_slice(&body_jis("ＡＢＣ"));
    honmon.extend_from_slice(&[0x1f, 0x05]);
    honmon.extend_from_slice(&[0xb1, 0x23]);
    honmon.extend_from_slice(&[0xb9, 0x99]);
    honmon.extend_from_slice(&body_jis("本文"));
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(&honmon),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 0,
    })
    .unwrap();
    let options = RenderOptions {
        mode: RenderMode::BasicText,
        include_debug_trace: true,
        ..RenderOptions::default()
    };

    let view = package.render_target(&token, &options).unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(view.display_html, None);
    assert_eq!(view.basic_text.as_deref(), Some("見出し\nABC一〓本文"));
    assert!(
        view.capabilities
            .contains(&lvcore::RenderCapability::HcRenderInput)
    );
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_basic_text_visual_incomplete")
    );
    assert!(
        view.debug_trace
            .as_deref()
            .unwrap_or_default()
            .contains("ssed_stream_basic_text")
    );
}

#[test]
fn hc_renderer_profile_suppresses_known_nonliteral_gaiji_markers() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("HC013A.dll"), b"").unwrap();
    let mut honmon = body_jis("前");
    honmon.extend_from_slice(&[0xb2, 0x61]);
    honmon.extend_from_slice(&body_jis("後"));
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(&honmon),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 0,
    })
    .unwrap();

    let view = package
        .render_target(
            &token,
            &RenderOptions {
                include_debug_trace: true,
                ..RenderOptions::default()
            },
        )
        .unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert_eq!(view.basic_text.as_deref(), Some("前後"));
    assert!(
        view.display_html
            .as_deref()
            .is_some_and(|html| html.contains("前後") && !html.contains('〓'))
    );
    assert!(
        view.diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "hc_basic_text_gaiji_placeholders")
    );
    assert!(
        view.debug_trace
            .as_deref()
            .unwrap_or_default()
            .contains("\"suppressed_gaiji_pairs\":1")
    );
}

#[test]
fn native_hc_common_html_fallback_exposes_ssed_address_links_as_targets() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let mut honmon = body_jis("参照");
    honmon.extend_from_slice(&[
        0x1f, 0x44, 0xaa, 0xbb, 0xcc, 0xdd, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    ]);
    honmon.extend_from_slice(&body_jis("本文"));
    honmon.extend_from_slice(&[0x1f, 0x64, 0, 0, 0, 0, 0, 0]);
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(&honmon),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 0,
    })
    .unwrap();

    let view = package
        .render_target(&token, &RenderOptions::default())
        .unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::EntryBody);
    assert!(
        view.display_html
            .as_deref()
            .is_some_and(|html| html.contains("href=\"lvcore://target/"))
    );
    assert!(
        view.display_html
            .as_deref()
            .is_some_and(|html| !html.contains("href=\"lvaddr://00000001/0000\""))
    );
    assert_eq!(view.links.len(), 1);
    assert_eq!(view.links[0].kind, lvcore::TargetKind::SsedAddress);
    assert_eq!(
        view.links[0].attributes.get("href").map(String::as_str),
        Some("lvaddr://00000001/0000")
    );
    assert_eq!(
        view.links[0].token.decode().unwrap(),
        InternalTarget::SsedAddress {
            component: "HONMON.DIC".to_owned(),
            block: 1,
            offset: 0,
        }
    );
    assert_eq!(
        view.links[0].attributes.get("control").map(String::as_str),
        Some("1f44")
    );
}

#[test]
fn debug_mode_decodes_hc_ssed_stream_without_claiming_visual_rendering() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    fs::write(dir.path().join("DICT.uni"), uni_fixture()).unwrap();
    let mut honmon = body_jis("見出し");
    honmon.extend_from_slice(&[0x1f, 0x0a]);
    honmon.extend_from_slice(&body_jis("本文"));
    honmon.extend_from_slice(&[0xb1, 0x23]);
    fs::write(
        dir.path().join("HONMON.DIC"),
        sseddata_literal_fixture(&honmon),
    )
    .unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 0,
    })
    .unwrap();

    let view = package
        .render_target(
            &token,
            &RenderOptions {
                mode: RenderMode::Debug,
                ..RenderOptions::default()
            },
        )
        .unwrap();

    assert_eq!(view.kind, ResolvedTargetKind::Deferred);
    assert_eq!(view.display_html, None);
    assert_eq!(view.basic_text.as_deref(), Some("見出し\n本文一"));
    assert!(
        view.capabilities
            .contains(&lvcore::RenderCapability::HcRenderInput)
    );
    assert!(
        view.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "hc_debug_visual_incomplete")
    );
    assert!(
        view.debug_trace
            .as_deref()
            .unwrap_or_default()
            .contains("ssed_stream_debug_basic_text")
    );
}

#[test]
fn ssed_honmon_targets_accept_mac_extensionless_payload_alias() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("DICT.IDX"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIN".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        component, offset, ..
    } = input
    else {
        panic!("extensionless Mac HONMON should still produce HC SSED renderer input");
    };
    assert_eq!(component, "HONMON.DIN");
    assert_eq!(offset, 2);
}

#[test]
fn ssed_android_lvedinfo_honmon_diw_uses_shared_ssed_driver() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("ANDROID.IDX"),
        ssedinfo_fixture_with_magic_and_honmon(ANDROID_LVEDINFO_MAGIC, "HONMON.DIW"),
    )
    .unwrap();
    fs::write(
        dir.path().join("HONMON.DIW"),
        sseddata_literal_fixture(b"0123456789"),
    )
    .unwrap();

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    assert_eq!(package.metadata().format_family, FormatFamily::Ssed);
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIW".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        component, offset, ..
    } = input
    else {
        panic!("Android HONMON.DIW should produce HC SSED renderer input");
    };
    assert_eq!(component, "HONMON.DIW");
    assert_eq!(offset, 2);
}

#[test]
fn ssed_honmon_targets_accept_mac_zipcrypto_payload_wrapper() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("0000012e.idx"),
        ssedinfo_fixture_with_honmon("HONMON.DIN"),
    )
    .unwrap();
    write_zipcrypto_honmon_wrapper(
        &dir.path().join("HONMON"),
        "HONMON.DIN",
        b"casKet0000012e",
        &sseddata_literal_fixture(b"0123456789"),
    );

    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let target = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIN".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let input = package.renderer_input_for_target(&target).unwrap();
    let RendererInput::HcSsedStream {
        component, offset, ..
    } = input
    else {
        panic!("Mac HONMON ZipCrypto wrapper should produce HC SSED renderer input");
    };
    assert_eq!(component, "HONMON.DIN");
    assert_eq!(offset, 2);
}

#[test]
fn ssed_address_targets_report_missing_declared_components() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("DICT.IDX"), ssedinfo_fixture()).unwrap();
    let package = DriverRegistry::default().open_best(dir.path()).unwrap();
    let token = TargetToken::new(&InternalTarget::SsedAddress {
        component: "HONMON.DIC".to_owned(),
        block: 1,
        offset: 2,
    })
    .unwrap();

    let body = package.visual_body_for_target(&token).unwrap();
    let VisualBody::Unsupported { diagnostics, .. } = body else {
        panic!("missing component must not produce renderable body");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ssed_component_file_missing")
    );
}
