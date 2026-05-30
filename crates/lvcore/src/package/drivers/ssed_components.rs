use super::*;

impl ReaderBookPackage {
    pub(super) fn ssed_component_for_role_or_name(
        &self,
        role: SsedComponentRole,
        name: &str,
    ) -> Option<&SsedComponent> {
        let catalog = self.ssed_catalog.as_ref()?;
        catalog
            .components_by_role(role)
            .next()
            .or_else(|| catalog.component_named(name))
    }

    pub(super) fn resolve_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
    ) -> Result<Option<PathBuf>> {
        let candidates = self.resolve_ssed_component_candidate_paths(component)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut unreadable = Vec::new();
        for path in candidates {
            match self.materialize_readable_ssed_component_path(component, &path)? {
                Some(readable) => return Ok(Some(readable)),
                None => unreadable.push(path),
            }
        }
        Err(Error::Driver(format!(
            "candidate file(s) were found but none decoded as plain, zipped, or encrypted SSEDDATA: {}",
            unreadable
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    pub(super) fn resolve_ssed_component_candidate_paths(
        &self,
        component: &SsedComponent,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        let mut seen = BTreeSet::new();
        if let Some(path) = self
            .storage
            .resolve_casefolded(Path::new(&component.filename))?
        {
            seen.insert(path.clone());
            paths.push(path);
        }
        for alias in ssed_component_filename_aliases(component) {
            if let Some(path) = self.storage.resolve_casefolded(Path::new(&alias))?
                && seen.insert(path.clone())
            {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    pub(super) fn ssed_component_by_name(&self, component_name: &str) -> Option<&SsedComponent> {
        self.ssed_catalog
            .as_ref()?
            .components
            .iter()
            .find(|component| {
                component.filename.eq_ignore_ascii_case(component_name)
                    || (component_name.eq_ignore_ascii_case("COLSCR.DIC")
                        && component.role == SsedComponentRole::Colscr)
            })
    }

    pub(super) fn materialize_readable_ssed_component_path(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        if SsedDataHeader::parse_file(path).is_ok() {
            return Ok(Some(path.to_path_buf()));
        }

        if file_starts_with_android_wrapped_sseddata(path)? {
            let cache_path =
                self.ssed_component_cache_path(component, path, "android_lved_wrapped", "dic")?;
            if SsedDataHeader::parse_file(&cache_path).is_ok() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            if tmp_path.exists() {
                fs::remove_file(&tmp_path)?;
            }
            normalize_android_wrapped_sseddata_file_to_path(path, &tmp_path)?;
            SsedDataHeader::parse_file(&tmp_path)?;
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }

        if looks_like_zip_file(path)?
            && let Some(extracted) = self.extract_zipped_ssed_component(component, path)?
        {
            return self.materialize_readable_ssed_component_path(component, &extracted);
        }

        if let Some(decrypted) = self.decrypt_ssed_component_if_needed(component, path)? {
            return Ok(Some(decrypted));
        }

        Ok(None)
    }

    pub(super) fn extract_zipped_ssed_component(
        &self,
        component: &SsedComponent,
        zip_path: &Path,
    ) -> Result<Option<PathBuf>> {
        let member_name = match zip_member_name_for_component(component, zip_path)? {
            Some(member_name) => member_name,
            None => return Ok(None),
        };
        for password in self.mac_honmon_zip_passwords()? {
            let file = File::open(zip_path)?;
            let mut archive = ZipArchive::new(file).map_err(zip_error)?;
            let mut member = match password.as_deref() {
                Some(password) => match archive.by_name_decrypt(&member_name, password) {
                    Ok(member) => member,
                    Err(ZipError::InvalidPassword)
                    | Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
                None => match archive.by_name(&member_name) {
                    Ok(member) if member.encrypted() => continue,
                    Ok(member) => member,
                    Err(ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)) => continue,
                    Err(err) => return Err(zip_error(err)),
                },
            };
            let size_limit =
                zipped_ssed_component_size_limit(component, &member_name, member.size())?;
            let cache_path = self.ssed_component_cache_path(
                component,
                zip_path,
                &format!(
                    "zip:{}:{}",
                    member_name,
                    password
                        .as_deref()
                        .map(hex::encode)
                        .unwrap_or_else(|| "none".to_owned())
                ),
                "bin",
            )?;
            if cache_path.exists() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            {
                let mut outfile = File::create(&tmp_path)?;
                if let Err(error) =
                    copy_zip_member_with_size_limit(&mut member, &mut outfile, size_limit)
                {
                    let _ = fs::remove_file(&tmp_path);
                    match error {
                        Error::Io(error) => {
                            if password.is_none()
                                && matches!(
                                    error.kind(),
                                    std::io::ErrorKind::Unsupported
                                        | std::io::ErrorKind::PermissionDenied
                                        | std::io::ErrorKind::InvalidData
                                )
                            {
                                continue;
                            }
                            return Err(Error::Io(error));
                        }
                        error => return Err(error),
                    }
                }
                outfile.flush()?;
            }
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    pub(super) fn decrypt_ssed_component_if_needed(
        &self,
        component: &SsedComponent,
        path: &Path,
    ) -> Result<Option<PathBuf>> {
        let mut file = File::open(path)?;
        let mut prefix = vec![0_u8; 4096];
        let read = file.read(&mut prefix)?;
        prefix.truncate(read);
        if prefix.len() < 16 {
            return Ok(None);
        }
        let attempts: [(&str, PrefixDecryptFn, FileDecryptFn); 3] = [
            (
                "android_honmon_diw",
                decrypt_android_diw_prefix,
                decrypt_android_diw_file_to_path,
            ),
            (
                "macos_logofont_cipher",
                decrypt_macos_logofont_cipher_prefix,
                decrypt_macos_logofont_cipher_file_to_path,
            ),
            (
                "logofont_cipher",
                decrypt_logofont_cipher_prefix,
                decrypt_logofont_cipher_file_to_path,
            ),
        ];
        for (name, prefix_decrypt, file_decrypt) in attempts {
            let decrypted_prefix = prefix_decrypt(&prefix, prefix.len())?;
            if !decrypted_prefix.starts_with(SSEDDATA_MAGIC) {
                continue;
            }
            let cache_path = self.ssed_component_cache_path(component, path, name, "dic")?;
            if SsedDataHeader::parse_file(&cache_path).is_ok() {
                return Ok(Some(cache_path));
            }
            let tmp_path = cache_path.with_extension("tmp");
            file_decrypt(path, &tmp_path)?;
            SsedDataHeader::parse_file(&tmp_path)?;
            fs::rename(&tmp_path, &cache_path)?;
            return Ok(Some(cache_path));
        }
        Ok(None)
    }

    pub(super) fn mac_honmon_zip_passwords(&self) -> Result<Vec<Option<Vec<u8>>>> {
        let mut passwords = vec![None];
        for path in self.storage.list_dir(Path::new(""))? {
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().map(|value| value.to_string_lossy()) else {
                continue;
            };
            let Some(extension) = path.extension().map(|value| value.to_string_lossy()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("idx") {
                continue;
            }
            let lower = stem.to_ascii_lowercase();
            if lower.len() == 8 && lower.chars().all(|ch| ch.is_ascii_hexdigit()) {
                passwords.push(Some(format!("casKet{lower}").into_bytes()));
            }
        }
        Ok(passwords)
    }

    pub(super) fn ssed_component_cache_path(
        &self,
        component: &SsedComponent,
        source: &Path,
        stage: &str,
        extension: &str,
    ) -> Result<PathBuf> {
        let metadata = fs::metadata(source)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let mut hasher = Sha256::new();
        hasher.update(self.metadata.root_fingerprint.as_bytes());
        hasher.update(b"\0");
        hasher.update(component.filename.as_bytes());
        hasher.update(b"\0");
        hasher.update(stage.as_bytes());
        hasher.update(b"\0");
        hasher.update(source.as_os_str().to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(metadata.len().to_le_bytes());
        hasher.update(modified.to_le_bytes());
        let hash = hex::encode(hasher.finalize());
        let dir = private_cache_dir("ssed-components")?;
        Ok(dir.join(format!("{hash}.{extension}")))
    }

    pub(super) fn ssed_aux_index_specs(&self) -> Result<Vec<SsedAuxIndexSpec>> {
        let relative = Path::new("EXINFO.INI");
        if !self.storage.exists(relative)? {
            return Ok(Vec::new());
        }
        let bytes = self.storage.read(relative)?;
        Ok(parse_aux_index_specs_from_exinfo(&bytes))
    }

    pub(super) fn ssed_numeric_aux_index_specs(
        &self,
        excluded_infos: &BTreeSet<String>,
    ) -> Result<Vec<SsedAuxIndexSpec>> {
        let mut specs = Vec::new();
        for path in self.storage.list_dir(Path::new(""))? {
            let Some(name) = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
            else {
                continue;
            };
            if !is_numeric_aux_index_filename(&name) {
                continue;
            }
            if excluded_infos.contains(&name.to_ascii_lowercase()) {
                continue;
            }
            if file_starts_with_ssedinfo_magic(&path)? {
                continue;
            }
            let index = specs.len();
            specs.push(SsedAuxIndexSpec {
                index,
                name: name.clone(),
                info: name,
            });
        }
        Ok(specs)
    }
}
