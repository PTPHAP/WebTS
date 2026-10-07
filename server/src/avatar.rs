use crate::app::{App, Error};
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use md5::{Digest, Md5};
use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinSet,
};
use tsclientlib::{Connection, FileDownloadResult, FileUploadResult};
use tsproto_types::{ChannelId, ClientId};

pub const MAX_UPLOAD: usize = 64 * 1024;
const MAX_DOWNLOAD: usize = 128 * 1024;
#[derive(Clone)]
pub struct Target {
    pub client: u16,
    pub uid: String,
    pub hash: String,
}
enum Pending {
    Download(Target),
    Upload { request: String, bytes: Vec<u8> },
}
pub enum Completed {
    Download(Target, Result<Vec<u8>>),
    UploadReady(String, Result<Vec<u8>>),
    Uploaded(String, Result<String>),
}
pub struct Avatars {
    pending: HashMap<u16, (Pending, Instant)>,
    pub tasks: JoinSet<Completed>,
}
impl Default for Avatars {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            tasks: JoinSet::new(),
        }
    }
}
impl Avatars {
    fn available(&self) -> Result<()> {
        if self.pending.len() + self.tasks.len() >= 2 {
            bail!("头像传输正忙，请稍后重试");
        }
        Ok(())
    }
    pub fn download(&mut self, conn: &mut Connection, client: u16) -> Result<Target> {
        self.available()?;
        let state = conn.get_state()?;
        let member = state
            .clients
            .get(&ClientId(client))
            .ok_or_else(|| anyhow::anyhow!("成员不可见"))?;
        let uid = member
            .uid
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("服务器未提供身份"))?;
        if member.avatar_hash.len() != 32
            || !member.avatar_hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("成员未设置可用头像");
        }
        let target = Target {
            client,
            uid: uid.to_string(),
            hash: member.avatar_hash.clone(),
        };
        let path = format!("/avatar_{}", uid.as_avatar());
        let handle = conn.download_file(ChannelId(0), &path, None, None)?;
        self.pending.insert(
            handle.0,
            (Pending::Download(target.clone()), Instant::now()),
        );
        Ok(target)
    }
    pub fn prepare_upload(&mut self, app: Arc<App>, request: String, data: &str) -> Result<()> {
        self.available()?;
        if data.len() > MAX_UPLOAD.div_ceil(3) * 4 {
            bail!("头像最大64KiB");
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| anyhow::anyhow!("头像编码无效"))?;
        if bytes.len() > MAX_UPLOAD {
            bail!("头像最大64KiB");
        }
        self.tasks
            .spawn(async move { Completed::UploadReady(request, normalize(&app, bytes).await) });
        Ok(())
    }
    pub fn upload(&mut self, conn: &mut Connection, request: String, bytes: Vec<u8>) -> Result<()> {
        self.available()?;
        let handle = conn.upload_file(
            ChannelId(0),
            "/avatar",
            None,
            bytes.len() as u64,
            true,
            false,
        )?;
        self.pending.insert(
            handle.0,
            (Pending::Upload { request, bytes }, Instant::now()),
        );
        Ok(())
    }
    pub fn downloaded(&mut self, handle: u16, result: FileDownloadResult, app: Arc<App>) {
        if let Some((Pending::Download(target), _)) = self.pending.remove(&handle) {
            self.tasks.spawn(async move {
                let data = async {
                    if result.size == 0 || result.size > MAX_DOWNLOAD as u64 {
                        bail!("服务器头像过大或为空");
                    }
                    let mut stream = result.stream;
                    let mut bytes = vec![0; result.size as usize];
                    stream.read_exact(&mut bytes).await?;
                    if !format!("{:x}", Md5::digest(&bytes)).eq_ignore_ascii_case(&target.hash) {
                        bail!("头像版本已变化，请重新加载");
                    }
                    normalize(&app, bytes).await
                };
                let result = tokio::time::timeout(Duration::from_secs(5), data)
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("头像下载超时")));
                Completed::Download(target, result)
            });
        }
    }
    pub fn uploaded(&mut self, handle: u16, result: FileUploadResult) {
        if let Some((Pending::Upload { request, bytes }, _)) = self.pending.remove(&handle) {
            self.tasks.spawn(async move {
                let write = async {
                    if result.seek_position != 0 {
                        bail!("服务器头像上传偏移无效");
                    }
                    let mut stream = result.stream;
                    stream.write_all(&bytes).await?;
                    stream.shutdown().await?;
                    Ok(format!("{:x}", Md5::digest(&bytes)))
                };
                let result = tokio::time::timeout(Duration::from_secs(5), write)
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("头像上传超时")));
                Completed::Uploaded(request, result)
            });
        }
    }
    pub fn failed(&mut self, handle: u16) -> Option<Completed> {
        self.pending
            .remove(&handle)
            .map(|(pending, _)| failure(pending, "TS拒绝头像传输，请检查本人权限和文件传输端口"))
    }
    pub fn expire(&mut self) -> Vec<Completed> {
        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (_, time))| time.elapsed() > Duration::from_secs(10))
            .map(|(handle, _)| *handle)
            .collect();
        expired
            .into_iter()
            .filter_map(|handle| {
                self.pending
                    .remove(&handle)
                    .map(|(pending, _)| failure(pending, "头像传输响应超时"))
            })
            .collect()
    }
}
fn failure(pending: Pending, message: &'static str) -> Completed {
    let error = Err(anyhow::anyhow!(message));
    match pending {
        Pending::Download(target) => Completed::Download(target, error),
        Pending::Upload { request, .. } => {
            Completed::Uploaded(request, Err(anyhow::anyhow!(message)))
        }
    }
}
async fn normalize(app: &Arc<App>, bytes: Vec<u8>) -> Result<Vec<u8>> {
    app.work(move |_| {
        sanitize(&bytes).map_err(|_| Error::bad("头像必须是有效PNG/JPEG/GIF，最大512×512及128KiB"))
    })
    .await
    .map_err(|e| anyhow::anyhow!(e.1))
}
pub fn sanitize(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_DOWNLOAD {
        bail!("invalid avatar size");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif)
    ) {
        bail!("unsupported avatar format");
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(512);
    limits.max_image_height = Some(512);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    for size in [256, 128, 96] {
        let size = size.min(image.width().max(image.height()));
        let mut output = Cursor::new(Vec::new());
        image
            .thumbnail(size, size)
            .write_to(&mut output, ImageFormat::Png)?;
        let output = output.into_inner();
        if output.len() <= MAX_UPLOAD {
            return Ok(output);
        }
    }
    bail!("avatar cannot fit size limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_gif_avatar_is_shown_as_safe_static_png() {
        let image = image::RgbaImage::from_pixel(32, 32, image::Rgba([50, 80, 130, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Gif).unwrap();
        let output = sanitize(encoded.get_ref()).unwrap();
        assert!(output.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(image::load_from_memory(&output).unwrap().width(), 32);
    }
    #[test]
    fn avatar_rejects_active_content_oversize_and_decoding_bombs() {
        assert!(sanitize(b"<svg onload='alert(1)'/>").is_err());
        assert!(sanitize(&vec![0; MAX_DOWNLOAD + 1]).is_err());
        let image = image::RgbaImage::new(513, 1);
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(sanitize(encoded.get_ref()).is_err());
    }
    #[test]
    fn avatar_normalization_is_bounded_and_interoperable_png() {
        let image = image::RgbaImage::from_pixel(300, 300, image::Rgba([30, 70, 120, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        let normalized = sanitize(encoded.get_ref()).unwrap();
        assert!(normalized.len() <= MAX_UPLOAD);
        let decoded = image::load_from_memory(&normalized).unwrap();
        assert_eq!(decoded.width(), 256);
        assert_eq!(decoded.height(), 256);
    }
}
