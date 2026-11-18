//! `Mounter` — port of Swift `Sources/Services/Mounter.swift`
//! (@714aafa3): the DDI mount flow behind
//! `MinimuxerCore::mount_ddi` (diff list
//! `docs/m5-install-ddi-debug-diff.md` §5).
//!
//! Error typing mirrors Swift: the Mounter's own failures are
//! `MinimuxerError`s (noDevice / invalidProductVersion /
//! downloadImage / mount), gateway failures pass through as
//! their `MmErrorInfo` (the facade's catch-all maps them),
//! and local filesystem failures behave like Swift's Cocoa
//! throws — they surface through the facade catch-all, so
//! they are carried as a foreign-domain `MmErrorInfo` whose
//! description is the error text.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mm_common::constants::{
    DDI_IMAGE_URL, DDI_MANIFEST_URL, DDI_TRUSTCACHE_URL, PRE17_VERSIONS_URL,
};
use mm_common::error::MinimuxerError;
use mm_common::{MmErrorInfo, PairingProtocol};
use mm_gateway::DeviceGateway;

use crate::core::CoreFuture;

/// Swift's `Data(contentsOf: URL)` download seam: production
/// is reqwest; tests inject fixtures.
pub trait ImageDownloader: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str) -> CoreFuture<'a, Result<Vec<u8>, String>>;
}

/// The production downloader (reqwest, rustls).
pub struct ReqwestDownloader;

impl ImageDownloader for ReqwestDownloader {
    fn fetch<'a>(&'a self, url: &'a str) -> CoreFuture<'a, Result<Vec<u8>, String>> {
        let url = url.to_string();
        Box::pin(async move {
            let response = reqwest::get(&url).await.map_err(|e| e.to_string())?;
            let response = response
                .error_for_status()
                .map_err(|e| e.to_string())?;
            response
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|e| e.to_string())
        })
    }
}

/// A local-filesystem failure, carried like a Swift Cocoa
/// throw: neither a DeviceGatewayError nor a MinimuxerError,
/// so the facade's catch-all claims it.
fn io_failure(error: &std::io::Error) -> MmErrorInfo {
    MmErrorInfo::new("MounterIOError", "ioError", &error.to_string())
}

fn minimuxer(error: MinimuxerError) -> MmErrorInfo {
    error.to_info()
}

/// Swift `Mounter`: gateway + endpoint reachability + the
/// download seam.
pub struct Mounter {
    gateway: Arc<dyn DeviceGateway>,
    endpoint_ip: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    downloader: Arc<dyn ImageDownloader>,
}

impl Mounter {
    pub fn new(
        gateway: Arc<dyn DeviceGateway>,
        endpoint_ip: Arc<dyn Fn() -> Option<String> + Send + Sync>,
        downloader: Arc<dyn ImageDownloader>,
    ) -> Self {
        Self {
            gateway,
            endpoint_ip,
            downloader,
        }
    }

    /// Swift `mount(docsPath:)` — `Ok(true)` when this call
    /// mounted the image, `Ok(false)` when it was already
    /// mounted.
    pub async fn mount(&self, docs_path: &str) -> Result<bool, MmErrorInfo> {
        let path = docs_path.strip_prefix("file://").unwrap_or(docs_path);
        let trimmed = path.strip_suffix('/').unwrap_or(path);
        let dmg_docs_path = format!("{trimmed}/DMG");
        let _ = std::fs::create_dir_all(&dmg_docs_path);

        if (self.endpoint_ip)().is_none() {
            return Err(minimuxer(MinimuxerError::NoDevice(
                "Reachable device IP not found".to_string(),
            )));
        }

        if self.gateway.is_ddi_mounted().await? {
            return Ok(false);
        }

        let mut major: i64 = 17;
        let mut version: Option<String> = None;
        if self.gateway.pairing_file_type() == PairingProtocol::Lockdown {
            let v = self
                .gateway
                .get_lockdown_value("ProductVersion")
                .await?;
            let parsed = v
                .split('.')
                .next()
                .and_then(|first| first.parse::<i64>().ok());
            match parsed {
                Some(m) => {
                    major = m;
                    version = Some(v);
                }
                None => {
                    return Err(minimuxer(MinimuxerError::InvalidProductVersion(
                        v,
                    )));
                }
            }
        }

        self.perform_mount(major, version.as_deref(), &dmg_docs_path)
            .await?;
        Ok(true)
    }

    async fn perform_mount(
        &self,
        major: i64,
        ios_version: Option<&str>,
        dmg_docs_path: &str,
    ) -> Result<(), MmErrorInfo> {
        if major < 17 {
            if let Some(version) = ios_version {
                let (dmg, sig) = self.load_pre17_image(version, dmg_docs_path).await?;
                self.gateway.mount_developer_image(&dmg, &sig).await?;
                return Ok(());
            }
        }
        let (image, trustcache, manifest) =
            self.load_post17_image(dmg_docs_path).await?;
        self.gateway
            .mount_personalized_ddi(&image, &trustcache, &manifest)
            .await
    }

    /// Swift `getOrDownload`: local file first; a failed
    /// download is `downloadImage`, local read/write failures
    /// are Cocoa-shaped.
    async fn get_or_download(
        &self,
        url: &str,
        local_path: &Path,
    ) -> Result<Vec<u8>, MmErrorInfo> {
        if local_path.exists() {
            return std::fs::read(local_path).map_err(|e| io_failure(&e));
        }
        let data = self.downloader.fetch(url).await.map_err(|_| {
            minimuxer(MinimuxerError::DownloadImage(format!(
                "Failed to download file from {url}"
            )))
        })?;
        std::fs::write(local_path, &data).map_err(|e| io_failure(&e))?;
        Ok(data)
    }
}

impl Mounter {
    /// Swift `loadPre17Image`: local `<version>.dmg` /
    /// `.dmg.signature`, else the versions-table zip flow.
    async fn load_pre17_image(
        &self,
        ios_version: &str,
        dmg_docs_path: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), MmErrorInfo> {
        let dir = PathBuf::from(dmg_docs_path);
        let dmg_path = dir.join(format!("{ios_version}.dmg"));
        let sig_path = dir.join(format!("{ios_version}.dmg.signature"));

        if !dmg_path.exists() || !sig_path.exists() {
            let lookup_failed = || {
                minimuxer(MinimuxerError::DownloadImage(format!(
                    "Failed to retrieve pre-17 versions plist or find iOS {ios_version} DMG URL"
                )))
            };
            let table_bytes = self
                .downloader
                .fetch(PRE17_VERSIONS_URL)
                .await
                .map_err(|_| lookup_failed())?;
            let table: serde_json::Value =
                serde_json::from_slice(&table_bytes).map_err(|_| lookup_failed())?;
            let dmg_url = table
                .get(ios_version)
                .and_then(|v| v.as_str())
                .ok_or_else(lookup_failed)?
                .to_string();

            let zip_path = dir.join("dmg.zip");
            let tmp_dir = dir.join("tmp");
            let result = self
                .fetch_and_unzip_pre17(&dmg_url, &zip_path, &tmp_dir, &dmg_path, &sig_path)
                .await;
            // Swift's defer: zip + tmp always removed.
            let _ = std::fs::remove_file(&zip_path);
            let _ = std::fs::remove_dir_all(&tmp_dir);
            result?;
        }

        let dmg = std::fs::read(&dmg_path);
        let sig = std::fs::read(&sig_path);
        match (dmg, sig) {
            (Ok(dmg), Ok(sig)) => Ok((dmg, sig)),
            _ => Err(minimuxer(MinimuxerError::Mount {
                protocol: PairingProtocol::Lockdown,
                reason: format!(
                    "Unable to read pre-17 image files at: {}",
                    dmg_path.display()
                ),
            })),
        }
    }

    async fn fetch_and_unzip_pre17(
        &self,
        dmg_url: &str,
        zip_path: &Path,
        tmp_dir: &Path,
        dmg_path: &Path,
        sig_path: &Path,
    ) -> Result<(), MmErrorInfo> {
        // Swift `try Data(contentsOf: dmgUrl)`: a failure here
        // is a Cocoa throw, not downloadImage.
        let zip_bytes = self
            .downloader
            .fetch(dmg_url)
            .await
            .map_err(|e| MmErrorInfo::new("MounterIOError", "ioError", &e))?;
        std::fs::write(zip_path, &zip_bytes).map_err(|e| io_failure(&e))?;
        let _ = std::fs::remove_dir_all(tmp_dir);
        std::fs::create_dir_all(tmp_dir).map_err(|e| io_failure(&e))?;
        unzip_into(zip_path, tmp_dir)?;

        let entries = std::fs::read_dir(tmp_dir).map_err(|e| io_failure(&e))?;
        let mut subdirs: Vec<PathBuf> =
            entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        subdirs.sort();
        for item in subdirs {
            let name = item
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !item.is_dir() || name.contains("__MACOSX") {
                continue;
            }
            let dmg_file = item.join("DeveloperDiskImage.dmg");
            let sig_file = item.join("DeveloperDiskImage.dmg.signature");
            if dmg_file.exists() {
                let _ = std::fs::remove_file(dmg_path);
                let _ = std::fs::remove_file(sig_path);
                std::fs::rename(&dmg_file, dmg_path).map_err(|e| io_failure(&e))?;
                std::fs::rename(&sig_file, sig_path).map_err(|e| io_failure(&e))?;
            }
        }
        Ok(())
    }

    /// Swift `loadPost17Image`.
    async fn load_post17_image(
        &self,
        dmg_docs_path: &str,
    ) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), MmErrorInfo> {
        let dir = PathBuf::from(dmg_docs_path);
        let image = self
            .get_or_download(DDI_IMAGE_URL, &dir.join("Image.dmg"))
            .await?;
        let trustcache = self
            .get_or_download(DDI_TRUSTCACHE_URL, &dir.join("Image.dmg.trustcache"))
            .await?;
        let manifest = self
            .get_or_download(DDI_MANIFEST_URL, &dir.join("BuildManifest.plist"))
            .await?;
        Ok((image, trustcache, manifest))
    }
}

/// Extracts a zip archive into `dest` (ZIPFoundation's
/// `unzipItem` in Swift). Entry paths are confined to `dest`.
pub(crate) fn unzip_into(zip_path: &Path, dest: &Path) -> Result<(), MmErrorInfo> {
    let file = std::fs::File::open(zip_path).map_err(|e| io_failure(&e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| MmErrorInfo::new("MounterIOError", "ioError", &e.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| MmErrorInfo::new("MounterIOError", "ioError", &e.to_string()))?;
        let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| io_failure(&e))?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io_failure(&e))?;
            }
            let mut writer =
                std::fs::File::create(&out).map_err(|e| io_failure(&e))?;
            std::io::copy(&mut entry, &mut writer).map_err(|e| io_failure(&e))?;
        }
    }
    Ok(())
}

/// Zips a directory's *contents* at the archive root
/// (FileManager.zipItem with shouldKeepParent: false, as in
/// the facade's dumpProfiles .zip mode).
pub(crate) fn zip_dir_contents(dir: &Path, zip_path: &Path) -> Result<(), MmErrorInfo> {
    let file = std::fs::File::create(zip_path).map_err(|e| io_failure(&e))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![dir.to_path_buf()];
    let mut files: Vec<PathBuf> = Vec::new();
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| io_failure(&e))?;
        for entry in entries {
            let path = entry.map_err(|e| io_failure(&e))?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        writer
            .start_file(rel, options)
            .map_err(|e| MmErrorInfo::new("MounterIOError", "ioError", &e.to_string()))?;
        let data = std::fs::read(&path).map_err(|e| io_failure(&e))?;
        use std::io::Write as _;
        writer.write_all(&data).map_err(|e| io_failure(&e))?;
    }
    writer
        .finish()
        .map_err(|e| MmErrorInfo::new("MounterIOError", "ioError", &e.to_string()))?;
    Ok(())
}
