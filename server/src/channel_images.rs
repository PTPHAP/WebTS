//! Channel pictures use the current identity's native file permission, never an HTTP proxy.
use crate::app::{App, Error};
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
pub fn description_image(description: &str, url: &str) -> bool {
    let lower = description.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(start) = lower[offset..].find("[img]") {
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
    ) -> Result<()> {
        if id.len() > 64 || self.pending.len() + self.tasks.len() >= 2 {
            bail!("图片传输正忙，请稍后重试");
        }
        if self.rate.0.elapsed() > Duration::from_secs(10) {
            self.rate = (Instant::now(), 0);
        }
        self.rate.1 = self.rate.1.saturating_add(1);
        if self.rate.1 > 8 {
            bail!("图片读取过于频繁，请稍后重试");
        }
        let state = conn.get_state()?;
        let description = state
            .channels
            .get(&ChannelId(source))
            .and_then(|c| c.optional_data.as_ref())
            .context("频道介绍不可见")?;
        if !description_image(&description.description, &url) {
            bail!("图片未出现在当前可见频道介绍中");
        }
        let target = image_path(&url)?;
        if !state.channels.contains_key(&ChannelId(target.channel)) {
            bail!("图片频道不可见");
        }
        if let Some(uid) = target.server_uid.as_ref()
            && *uid != state.server.public_key.get_uid()
        {
            bail!("图片属于其他服务器");
        }
        let handle = conn.download_file(ChannelId(target.channel), &target.path, None, None)?;
        self.pending
            .insert(handle.0, (Request { id, url, source }, Instant::now()));
        Ok(())
    }
    pub fn downloaded(&mut self, handle: u16, result: FileDownloadResult, app: Arc<App>) {
        if let Some((request, _)) = self.pending.remove(&handle) {
            self.tasks.spawn(async move {
                let read = async {
                    if result.size == 0 || result.size > MAX_DOWNLOAD as u64 {
                        bail!("频道图片最大2MiB");
                    }
                    let mut bytes = vec![0; result.size as usize];
                    let mut stream = result.stream;
                    stream.read_exact(&mut bytes).await?;
                    app.work(move |_| {
                        sanitize(&bytes).map_err(|_| Error::bad("图片格式、尺寸或内容无效"))
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
pub fn event(conn: &Connection, completed: Completed) -> serde_json::Value {
    let request = completed.request;
    let visible = conn
        .get_state()
        .ok()
        .and_then(|s| s.channels.get(&ChannelId(request.source)))
        .and_then(|c| c.optional_data.as_ref())
        .is_some_and(|d| description_image(&d.description, &request.url))
        && image_path(&request.url).ok().is_some_and(|target| {
            conn.get_state()
                .is_ok_and(|state| state.channels.contains_key(&ChannelId(target.channel)))
        });
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
    if bytes.is_empty() || bytes.len() > MAX_DOWNLOAD {
        bail!("invalid image size");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif | ImageFormat::WebP)
    ) {
        bail!("unsupported image");
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(24 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    for size in [1280, 960, 640, 320] {
        let mut output = Cursor::new(Vec::new());
        image
            .thumbnail(size, size)
            .write_to(&mut output, ImageFormat::Png)?;
        if output.get_ref().len() <= 192 * 1024 {
            return Ok(output.into_inner());
        }
    }
    bail!("image too complex")
}
#[cfg(test)]
mod tests {
    use super::*;
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
            "ts3image://x?channel=1"
        ));
        assert!(!description_image(
            "[url]ts3image://x?channel=1[/url]",
            "ts3image://x?channel=1"
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
