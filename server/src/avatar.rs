use crate::{app::App, settings::ImageLimits};
use anyhow::{Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
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
#[cfg(test)]
const MAX_DOWNLOAD: usize = 128 * 1024;
#[derive(Clone)]
pub struct Target {
    pub client: u16,
    pub uid: String,
    pub hash: String,
}
enum Pending {
    Download { target: Target, limits: ImageLimits },
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
    pub fn download(
        &mut self,
        conn: &mut Connection,
        client: u16,
        limits: ImageLimits,
    ) -> Result<Target> {
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
            (
                Pending::Download {
                    target: target.clone(),
                    limits,
                },
                Instant::now(),
            ),
        );
        Ok(target)
    }
    pub fn prepare_upload(&mut self, app: Arc<App>, request: String, data: &str) -> Result<()> {
        self.available()?;
        let limits = app.runtime.read().unwrap().settings.image_limits;
        let max_upload = limits.avatar_upload_kib as usize * 1024;
        if data.len() > max_upload.div_ceil(3) * 4 {
            bail!("头像超过站点限制（{}KiB）", limits.avatar_upload_kib);
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| anyhow::anyhow!("头像编码无效"))?;
        if bytes.len() > max_upload {
            bail!("头像超过站点限制（{}KiB）", limits.avatar_upload_kib);
        }
        self.tasks.spawn(async move {
            Completed::UploadReady(request, normalize(&app, bytes, limits, false).await)
        });
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
        if let Some((Pending::Download { target, limits }, _)) = self.pending.remove(&handle) {
            self.tasks.spawn(async move {
                let data = async {
                    if result.size == 0
                        || result.size > u64::from(limits.avatar_download_kib) * 1024
                    {
                        bail!("服务器头像过大或为空");
                    }
                    let mut stream = result.stream;
                    let mut bytes = vec![0; result.size as usize];
                    stream.read_exact(&mut bytes).await?;
                    if !format!("{:x}", Md5::digest(&bytes)).eq_ignore_ascii_case(&target.hash) {
                        bail!("头像版本已变化，请重新加载");
                    }
                    normalize(&app, bytes, limits, true).await
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
        Pending::Download { target, .. } => Completed::Download(target, error),
        Pending::Upload { request, .. } => {
            Completed::Uploaded(request, Err(anyhow::anyhow!(message)))
        }
    }
}
async fn normalize(
    app: &Arc<App>,
    bytes: Vec<u8>,
    limits: ImageLimits,
    native: bool,
) -> Result<Vec<u8>> {
    app.expensive(move |_| Ok(sanitize_image(&bytes, limits, native)))
        .await
        .map_err(|e| anyhow::anyhow!(e.1))?
}
pub fn sanitize(bytes: &[u8]) -> Result<Vec<u8>> {
    sanitize_with_limits(bytes, ImageLimits::default())
}
pub fn sanitize_with_limits(bytes: &[u8], limits: ImageLimits) -> Result<Vec<u8>> {
    sanitize_image(bytes, limits, false)
}
fn sanitize_image(bytes: &[u8], limits: ImageLimits, native: bool) -> Result<Vec<u8>> {
    limits.validate()?;
    if bytes.is_empty()
        || bytes.len() > limits.avatar_download_kib.max(limits.avatar_upload_kib) as usize * 1024
    {
        bail!("头像为空或超过站点文件体积限制");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)
    ) {
        bail!("头像格式不支持，仅接受 PNG、JPEG、GIF、WebP");
    }
    let mut decode_limits = Limits::default();
    let source_dimension = if native {
        4096
    } else {
        limits.avatar_dimension
    };
    let allocation = if native {
        32 * 1024 * 1024
    } else {
        16 * 1024 * 1024
    };
    decode_limits.max_image_width = Some(source_dimension);
    decode_limits.max_image_height = Some(source_dimension);
    decode_limits.max_alloc = Some(allocation);
    reader.limits(decode_limits.clone());
    let mut decoder = reader.into_decoder().map_err(avatar_decode_error)?;
    let (width, height) = decoder.dimensions();
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > 8_000_000
        || decoder.total_bytes() > allocation
    {
        bail!("头像解码资源超限：原图最多 800 万像素，需使用较小图片");
    }
    // Match ImageReader::decode: reserve the output buffer before assigning decoder scratch space.
    decode_limits
        .reserve(decoder.total_bytes())
        .map_err(avatar_decode_error)?;
    decoder
        .set_limits(decode_limits)
        .map_err(avatar_decode_error)?;
    let image = DynamicImage::from_decoder(decoder).map_err(avatar_decode_error)?;
    for size in [256, 128, 96, 64, 48, 32] {
        let size = size
            .min(limits.avatar_dimension)
            .min(image.width().max(image.height()));
        let mut output = Cursor::new(Vec::new());
        image
            .thumbnail(size, size)
            .write_to(&mut output, ImageFormat::Png)
            .map_err(|_| anyhow::anyhow!("头像转换失败，请使用其他图片"))?;
        let output = output.into_inner();
        if output.len() <= limits.avatar_upload_kib as usize * 1024 {
            return Ok(output);
        }
    }
    bail!("头像压缩后仍超过站点保存体积限制")
}
fn avatar_decode_error(error: image::ImageError) -> anyhow::Error {
    if matches!(error, image::ImageError::Limits(_)) {
        anyhow::anyhow!("头像解码尺寸或内存超限，请使用较小图片")
    } else {
        anyhow::anyhow!("头像无法解码：图片损坏或格式不受支持")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn in_flight_avatar_uses_request_policy_after_hot_reload() {
        for limits in [
            ImageLimits {
                avatar_dimension: 1024,
                ..Default::default()
            },
            ImageLimits::default(),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let key = directory.path().join("master.key");
            std::fs::write(&key, hex::encode([42; 32])).unwrap();
            let config: crate::config::Config =
                toml::from_str(include_str!("../../config.example.toml")).unwrap();
            let app = App::new(crate::config::Config {
                database: directory.path().join("db").to_string_lossy().into_owned(),
                master_key_file: key.to_string_lossy().into_owned(),
                ..config
            })
            .unwrap();
            let image = image::RgbImage::from_pixel(1024, 1024, image::Rgb([30, 70, 120]));
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, ImageFormat::Png).unwrap();
            let bytes = encoded.into_inner();
            let mut avatars = Avatars::default();
            avatars.pending.insert(
                1,
                (
                    Pending::Download {
                        target: Target {
                            client: 1,
                            uid: "fixture".into(),
                            hash: format!("{:x}", Md5::digest(&bytes)),
                        },
                        limits,
                    },
                    Instant::now(),
                ),
            );
            app.runtime.write().unwrap().settings.image_limits = ImageLimits::default();
            assert!(
                sanitize_with_limits(&bytes, app.runtime.read().unwrap().settings.image_limits)
                    .is_err()
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let sender = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (receiver, _) = listener.accept().await.unwrap();
            let size = bytes.len() as u64;
            let writer = tokio::spawn(async move {
                let mut sender = sender;
                sender.write_all(&bytes).await.unwrap();
            });
            avatars.downloaded(
                1,
                FileDownloadResult {
                    size,
                    stream: receiver,
                },
                app,
            );
            match avatars.tasks.join_next().await.unwrap().unwrap() {
                Completed::Download(_, result) => {
                    let png = result.expect(
                        "a native avatar must be safely resized before applying display dimensions",
                    );
                    let decoded = image::load_from_memory(&png).unwrap();
                    assert_eq!((decoded.width(), decoded.height()), (256, 256));
                    assert!(png.len() <= limits.avatar_upload_kib as usize * 1024);
                }
                _ => panic!("expected avatar download"),
            }
            writer.await.unwrap();
        }
    }
    #[test]
    fn configurable_avatar_dimensions_preserve_reencoding_and_hard_caps() {
        let image = image::RgbImage::from_pixel(700, 1, image::Rgb([30, 70, 120]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(sanitize(encoded.get_ref()).is_err());
        let limits = ImageLimits {
            avatar_dimension: 1024,
            avatar_upload_kib: 128,
            ..Default::default()
        };
        assert!(
            sanitize_with_limits(encoded.get_ref(), limits)
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        assert!(
            sanitize_with_limits(
                encoded.get_ref(),
                ImageLimits {
                    avatar_dimension: 100000,
                    ..limits
                }
            )
            .is_err()
        );
    }
    #[test]
    fn native_webp_bot_avatar_is_reencoded_as_bounded_png() {
        let image = image::RgbaImage::from_pixel(32, 32, image::Rgba([50, 80, 130, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::WebP).unwrap();
        let output = sanitize(encoded.get_ref()).unwrap();
        assert!(output.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(output.len() <= MAX_UPLOAD);
        assert_eq!(image::load_from_memory(&output).unwrap().width(), 32);
    }

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
    #[test]
    fn native_large_formats_and_small_output_budget_remain_bounded() {
        let image = image::RgbImage::from_pixel(1024, 1024, image::Rgb([30, 70, 120]));
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::WebP,
        ] {
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, format).unwrap();
            let png = sanitize_image(encoded.get_ref(), ImageLimits::default(), true).unwrap();
            let decoded = image::load_from_memory(&png).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (256, 256));
            assert!(png.len() <= MAX_UPLOAD);
        }
        let noise = image::RgbImage::from_fn(128, 128, |x, y| {
            let n = x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663);
            image::Rgb([(n >> 16) as u8, (n >> 8) as u8, n as u8])
        });
        let mut encoded = Cursor::new(Vec::new());
        noise.write_to(&mut encoded, ImageFormat::Png).unwrap();
        let png = sanitize_image(
            encoded.get_ref(),
            ImageLimits {
                avatar_upload_kib: 8,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        assert!(png.len() <= 8192);
    }
    #[test]
    fn native_compatibility_does_not_bypass_resource_or_content_limits() {
        use image::ImageEncoder;
        let limits = ImageLimits::default();
        assert!(sanitize_image(b"<svg onload='alert(1)'/>", limits, true).is_err());
        assert!(sanitize_image(&vec![0; MAX_DOWNLOAD + 1], limits, true).is_err());
        assert!(sanitize_image(b"\x89PNG\r\n\x1a\ninvalid", limits, true).is_err());
        for (width, height) in [(4097, 1), (2900, 2900)] {
            let image = image::RgbImage::from_pixel(width, height, image::Rgb([30, 70, 120]));
            let mut encoded = Cursor::new(Vec::new());
            image::codecs::png::PngEncoder::new_with_quality(
                &mut encoded,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            )
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
            assert!(encoded.get_ref().len() <= MAX_DOWNLOAD);
            let error = sanitize_image(encoded.get_ref(), limits, true)
                .unwrap_err()
                .to_string();
            assert!(error.contains("解码"), "unexpected rejection: {error}");
        }
        let image = image::RgbImage::from_pixel(1024, 1024, image::Rgb([30, 70, 120]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(sanitize_with_limits(encoded.get_ref(), limits).is_err());
    }
}
