use super::*;

impl ReaderBookPackage {
    pub(super) fn resolve_package_file_resource(
        &self,
        token: &ResourceToken,
        path: &str,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        let relative = Path::new(path);
        let resolved = self.resolve_package_file_path(path)?;
        let mut diagnostics = Vec::new();
        let href = if resolved.is_some() {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{path} was not found in the package"),
            ));
            None
        };
        let label = resolved
            .as_ref()
            .and_then(|path| path.file_name())
            .or_else(|| relative.file_name())
            .map(|value| value.to_string_lossy().to_string());
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label,
            href,
            mime_type: resource_mime_type(resource_kind, Some(path)).map(str::to_owned),
            byte_len: resolved
                .as_ref()
                .and_then(|path| path.metadata().ok())
                .map(|metadata| metadata.len()),
            diagnostics,
        })
    }

    pub(super) fn resolve_chm_file_resource(
        &self,
        token: &ResourceToken,
        chm_path: &str,
        entry_path: &str,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        let chm_relative = Path::new(chm_path);
        let exists = self.storage.exists(chm_relative)?;
        let mut diagnostics = Vec::new();
        let href = if exists {
            Some(format!("lvcore://resource/{}", token.as_str()))
        } else {
            diagnostics.push(Diagnostic::warning(
                "resource_missing",
                format!("{chm_path} was not found in the package"),
            ));
            None
        };
        let label = Path::new(entry_path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .or_else(|| Some(entry_path.to_owned()));
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label,
            href,
            mime_type: resource_mime_type(resource_kind, Some(entry_path)).map(str::to_owned),
            byte_len: None,
            diagnostics,
        })
    }

    pub(super) fn read_chm_file_resource(
        &self,
        chm_path: &str,
        entry_path: &str,
    ) -> Result<Vec<u8>> {
        let relative = Path::new(chm_path);
        let Some(resolved) = self.storage.resolve_casefolded(relative)? else {
            return Err(Error::Driver(format!("resource not found: {chm_path}")));
        };
        if !path_stays_inside_root(&self.root, &resolved)? {
            return Err(Error::Driver(format!(
                "CHM resource path is outside the package: {chm_path}"
            )));
        }
        read_chm_entry(&resolved, entry_path)
    }

    pub(super) fn resolve_media_blob_resource(
        &self,
        token: &ResourceToken,
        store: &str,
        key: &str,
        resource_kind: ResourceKind,
    ) -> Result<ResourceRef> {
        let byte_len = self
            .lved_store
            .as_ref()
            .and_then(|lved_store| lved_store.media_blob_len(store, key).ok().flatten());
        Ok(ResourceRef {
            token: token.clone(),
            kind: resource_kind,
            label: Some(key.to_owned()),
            href: self
                .lved_store
                .is_some()
                .then(|| format!("lvcore://resource/{}", token.as_str())),
            mime_type: resource_mime_type(resource_kind, Some(key)).map(str::to_owned),
            byte_len,
            diagnostics: if self.lved_store.is_some() {
                Vec::new()
            } else {
                vec![Diagnostic::info(
                    "resource_deferred",
                    "media blob resource resolution is not implemented yet for this package",
                )]
            },
        })
    }

    pub(super) fn read_media_blob_resource(&self, store: &str, key: &str) -> Result<Vec<u8>> {
        let Some(lved_store) = &self.lved_store else {
            return Err(Error::Driver(
                "media blob resource reading is not implemented yet for this package".to_owned(),
            ));
        };
        let Some(bytes) = lved_store.media_blob(store, key)? else {
            return Err(Error::Driver(format!(
                "media blob not found: {store}:{key}"
            )));
        };
        Ok(bytes)
    }

    pub(super) fn resolve_unsupported_resource(
        &self,
        token: &ResourceToken,
        reason: String,
    ) -> Result<ResourceRef> {
        Ok(ResourceRef {
            token: token.clone(),
            kind: ResourceKind::Other,
            label: None,
            href: None,
            mime_type: None,
            byte_len: None,
            diagnostics: vec![Diagnostic::warning("resource_unsupported", reason)],
        })
    }
}
