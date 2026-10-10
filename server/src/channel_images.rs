//! Native pictures use the real identity; external server banners are separately restricted.
use crate::{
    app::{App, Error},
    settings::ImageLimits,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use std::{
    collections::HashMap,
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, task::JoinSet};
use tsclientlib::{Connection, FileDownloadResult};
use tsproto_types::ChannelId;

#[cfg(test)]
const MAX_DOWNLOAD: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePath {
    pub channel: u64,
    pub path: String,
    pub server_uid: Option<String>,
}
pub fn image_path(value: &str) -> Result<ImagePath> {
    if value.len() > 2048 || value.chars().any(|c| c.is_control()) {
        bail!("图片地址无效");
    }
    let value = value
        .strip_prefix("ts3image://")
        .context("只支持TS频道图片")?;
    let (authority, query) = value.split_once('?').context("图片缺少频道参数")?;
    let mut fields = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            bail!("图片参数重复");
        }
    }
    let channel = fields
        .get("channel")
        .context("图片缺少频道")?
        .parse::<u64>()?;
    if channel == 0 {
        bail!("频道图片不能访问服务器根目录");
    }
    let filename = match fields.get("filename") {
        Some(name) => name.clone(),
        None => percent_encoding::percent_decode_str(authority)
            .decode_utf8()?
            .into_owned(),
    };
    let directory = fields.get("path").map(String::as_str).unwrap_or("/");
    let valid = |part: &str| {
        !part.is_empty()
            && !matches!(part, "." | "..")
            && !part
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '?' | '#' | '%'))
    };
    if !valid(&filename)
        || !directory.starts_with('/')
        || directory.len() > 512
        || filename.len() > 255
        || directory
            .split('/')
            .filter(|s| !s.is_empty())
            .any(|s| !valid(s))
    {
        bail!("图片文件路径无效");
    }
    let path = format!("{}/{}", directory.trim_end_matches('/'), filename);
    Ok(ImagePath {
        channel,
        path,
        server_uid: fields.get("serverUID").cloned(),
    })
}
pub fn description_image(description: &str, url: &str, max_images: u32) -> bool {
    let lower = description.to_ascii_lowercase();
    let mut offset = 0;
    let mut count = 0;
    while let Some(start) = lower[offset..].find("[img]") {
        count += 1;
        if count > max_images {
            return false;
        }
        let start = offset + start + 5;
        let Some(end) = lower[start..].find("[/img]") else {
            break;
        };
        if description[start..start + end].trim() == url {
            return true;
        }
        offset = start + end + 6;
    }
    false
}
#[derive(Clone)]
struct Request {
    id: String,
    url: String,
    source: u64,
    limits: ImageLimits,
}
pub struct Completed {
    request: Request,
    result: Result<Vec<u8>>,
}
pub struct Images {
    pending: HashMap<u16, (Request, Instant)>,
    pub tasks: JoinSet<Completed>,
    rate: (Instant, u8),
}
impl Default for Images {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            tasks: JoinSet::new(),
            rate: (Instant::now(), 0),
        }
    }
}
impl Images {
    pub fn contains(&self, handle: u16) -> bool {
        self.pending.contains_key(&handle)
    }
    pub fn download(
        &mut self,
        conn: &mut Connection,
        id: String,
        source: u64,
        url: String,
        app: Arc<App>,
        limits: ImageLimits,
    ) -> Result<()> {
        if id.len() > 64 || self.pending.len() + self.tasks.len() >= 2 {
            bail!("图片传输正忙，请稍后重试");
        }
        if self.rate.0.elapsed() > Duration::from_secs(10) {
            self.rate = (Instant::now(), 0);
        }
        self.rate.1 = self.rate.1.saturating_add(1);
        if u32::from(self.rate.1) > limits.channel_requests {
            bail!("图片读取过于频繁，请稍后重试");
        }
        let state = conn.get_state()?;
        let request = Request {
            id,
            url,
            source,
            limits,
        };
        if !visible(conn, &request) {
            bail!("图片不属于当前可见的服务器或频道介绍");
        }
        if let Some(banner) = request.url.strip_prefix("tsserver:banner:") {
            let banner = banner.to_owned();
            self.tasks.spawn(async move {
                let result = async {
                    let max = request.limits.channel_download_kib as usize * 1024;
                    let permit = app
                        .image_bytes
                        .clone()
                        .try_acquire_many_owned(max.div_ceil(1024) as u32)
                        .context("图片读取内存预算正忙，请稍后重试")?;
                    let bytes = crate::server_banner::download(&banner, max).await?;
                    let limits = request.limits;
                    app.expensive(move |_| {
                        let _permit = permit;
                        sanitize_with_limits(&bytes, limits)
                            .map_err(|_| Error::bad("图片格式、尺寸或内容无效"))
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!(e.1))
                }
                .await;
                Completed { request, result }
            });
            return Ok(());
        }
        let handle = if request.url.starts_with("tsserver:icon:") {
            conn.download_file(
                ChannelId(0),
                &format!("/icon_{}", state.server.icon.0),
                None,
                None,
            )?
        } else {
            let target = image_path(&request.url)?;
            conn.download_file(ChannelId(target.channel), &target.path, None, None)?
        };
        self.pending.insert(handle.0, (request, Instant::now()));
        Ok(())
    }
    pub fn downloaded(&mut self, handle: u16, result: FileDownloadResult, app: Arc<App>) {
        if let Some((request, _)) = self.pending.remove(&handle) {
            self.tasks.spawn(async move {
                let read = async {
                    if result.size == 0
                        || result.size > u64::from(request.limits.channel_download_kib) * 1024
                    {
                        bail!(
                            "频道图片超过站点限制（{}KiB）",
                            request.limits.channel_download_kib
                        );
                    }
                    // Units are KiB; hold the global buffer budget through decoding.
                    let permit = app
                        .image_bytes
                        .clone()
                        .try_acquire_many_owned(result.size.div_ceil(1024) as u32)
                        .context("图片读取内存预算正忙，请稍后重试")?;
                    let mut bytes = vec![0; result.size as usize];
                    let mut stream = result.stream;
                    stream.read_exact(&mut bytes).await?;
                    let limits = request.limits;
                    app.expensive(move |_| {
                        let _permit = permit;
                        sanitize_with_limits(&bytes, limits)
                            .map_err(|_| Error::bad("图片格式、尺寸或内容无效"))
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!(e.1))
                };
                let result = tokio::time::timeout(Duration::from_secs(8), read)
                    .await
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("频道图片读取超时")));
                Completed { request, result }
            });
        }
    }
    pub fn failed(&mut self, handle: u16) -> Option<Completed> {
        self.pending.remove(&handle).map(|(request, _)| Completed {
            request,
            result: Err(anyhow::anyhow!(
                "TS拒绝图片读取，请检查本人文件下载权限、频道密码及传输端口"
            )),
        })
    }
    pub fn expire(&mut self) -> Vec<Completed> {
        let handles: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (_, time))| time.elapsed() > Duration::from_secs(10))
            .map(|(id, _)| *id)
            .collect();
        handles
            .into_iter()
            .filter_map(|id| self.failed(id))
            .collect()
    }
}
fn visible(conn: &Connection, request: &Request) -> bool {
    let Ok(state) = conn.get_state() else {
        return false;
    };
    if request.source == 0 {
        if request.url == format!("tsserver:icon:{}", state.server.icon.0) {
            return state.server.icon.0 != 0;
        }
        if let Some(banner) = request.url.strip_prefix("tsserver:banner:") {
            return !banner.is_empty() && banner == state.server.hostbanner_gfx_url;
        }
    }
    let description = if request.source == 0 {
        format!(
            "{}\n{}",
            state.server.welcome_message, state.server.hostmessage
        )
    } else {
        let Some(data) = state
            .channels
            .get(&ChannelId(request.source))
            .and_then(|c| c.optional_data.as_ref())
        else {
            return false;
        };
        data.description.clone()
    };
    description_image(&description, &request.url, request.limits.channel_images)
        && image_path(&request.url).ok().is_some_and(|target| {
            state.channels.contains_key(&ChannelId(target.channel))
                && target
                    .server_uid
                    .as_ref()
                    .is_none_or(|uid| *uid == state.server.public_key.get_uid())
        })
}
pub fn event(conn: &Connection, completed: Completed) -> serde_json::Value {
    let request = completed.request;
    let visible = visible(conn, &request);
    let (data, error) = match completed.result {
        Ok(bytes) if visible => (
            Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes))),
            None,
        ),
        Ok(_) => (None, Some("频道介绍已变化，请重新读取".into())),
        Err(e) => (None, Some(e.to_string())),
    };
    serde_json::json!({"type":"ts_image","id":request.id,"url":request.url,"source_channel":request.source,"data":data,"error":error})
}
pub fn sanitize(bytes: &[u8]) -> Result<Vec<u8>> {
    sanitize_with_limits(bytes, ImageLimits::default())
}
pub fn sanitize_with_limits(bytes: &[u8], limits: ImageLimits) -> Result<Vec<u8>> {
    limits.validate()?;
    if bytes.is_empty() || bytes.len() > limits.channel_download_kib as usize * 1024 {
        bail!("invalid image size");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)
    ) {
        bail!("unsupported image");
    }
    let mut decode_limits = Limits::default();
    decode_limits.max_image_width = Some(limits.channel_dimension);
    decode_limits.max_image_height = Some(limits.channel_dimension);
    decode_limits.max_alloc = Some(80 * 1024 * 1024);
    reader.limits(decode_limits);
    let image = reader.decode()?;
    for size in [1280, 960, 640, 320] {
        let mut output = Cursor::new(Vec::new());
        image
            .thumbnail(
                size.min(image.width().max(image.height())),
                size.min(image.width().max(image.height())),
            )
            .write_to(&mut output, ImageFormat::Png)?;
        if output.get_ref().len() <= limits.channel_output_kib as usize * 1024 {
            return Ok(output.into_inner());
        }
    }
    bail!("image too complex")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admin_limits_change_decoding_size_and_description_image_budget() {
        let image = image::RgbImage::from_pixel(2500, 1, image::Rgb([50, 80, 130]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(sanitize(encoded.get_ref()).is_err());
        let limits = ImageLimits {
            channel_dimension: 4096,
            ..Default::default()
        };
        assert!(sanitize_with_limits(encoded.get_ref(), limits).is_ok());
        let text = "[img]ts3image://a?channel=1[/img][img]ts3image://b?channel=1[/img]";
        assert!(!description_image(text, "ts3image://b?channel=1", 1));
        assert!(description_image(text, "ts3image://b?channel=1", 2));
        assert!(
            sanitize_with_limits(
                &vec![0; 65537],
                ImageLimits {
                    channel_download_kib: 64,
                    ..limits
                }
            )
            .is_err()
        );
    }
    #[test]
    fn paths_bind_to_channel_and_reject_traversal_and_ambiguous_parameters() {
        assert_eq!(
            image_path("ts3image://weixin.jpg?channel=1&path=/")
                .unwrap()
                .path,
            "/weixin.jpg"
        );
        assert_eq!(
            image_path(
                "ts3image://my-server?channel=2&path=/images/&filename=%E5%BE%AE%E4%BF%A1.webp"
            )
            .unwrap()
            .path,
            "/images/微信.webp"
        );
        for value in [
            "https://evil.test/x",
            "ts3image://x?channel=0",
            "ts3image://x?channel=1&path=/../",
            "ts3image://%2e%2e?channel=1",
            "ts3image://x?channel=1&channel=2",
            "ts3image://x?channel=1&filename=a%2Fb",
            "ts3image://x?channel=1&filename=%252e%252e",
            "ts3image://x?channel=1&path=C:/",
        ] {
            assert!(image_path(value).is_err(), "{value}");
        }
        assert!(description_image(
            "[center][IMG]ts3image://x?channel=1[/IMG][/center]",
            "ts3image://x?channel=1",
            8
        ));
        assert!(!description_image(
            "[url]ts3image://x?channel=1[/url]",
            "ts3image://x?channel=1",
            8
        ));
    }
    #[test]
    fn images_are_reencoded_and_active_or_oversized_content_rejected() {
        assert!(sanitize(b"<svg onload='alert(1)'/>").is_err());
        assert!(sanitize(&vec![0; MAX_DOWNLOAD + 1]).is_err());
        let image = image::RgbaImage::from_pixel(640, 360, image::Rgba([40, 80, 120, 255]));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let result = sanitize(bytes.get_ref()).unwrap();
        assert!(result.starts_with(b"\x89PNG"));
        assert!(result.len() <= 192 * 1024);
    }
}
