pub fn content(purpose: &str, origin: &str, token: &str) -> (String, String, String) {
    let (title, description, action, note) = if purpose == "verify" {
        (
            "验证你的 WebTS 邮箱",
            "只差一步，就能带上你的身份，与伙伴相聚。请确认这是你用于注册 WebTS 的邮箱。",
            "验证邮箱",
            "验证后即可登录，创建或导入 TeamSpeak 身份。",
        )
    } else {
        (
            "重置你的 WebTS 密码",
            "收到了你的密码找回请求。点击下方按钮，为你的账号设置一个新密码。",
            "设置新密码",
            "重置密码后，所有设备会退出登录，活动语音连接将断开；绑定的 TeamSpeak 身份会保留。",
        )
    };
    let link = format!("{origin}/#{purpose}={token}");
    let plain = format!(
        "{title}\n\n{description}\n\n{action}：\n{link}\n\n链接15分钟内有效，仅可使用一次。\n{note}\n\n如果不是你发起的操作，请忽略此邮件。请勿转发链接。\n来自 {origin}"
    );
    let mut html = include_str!("email.html").to_owned();
    for (key, value) in [
        ("title", title),
        ("description", description),
        ("action", action),
        ("note", note),
        ("link", &link),
        ("origin", origin),
    ] {
        let safe = value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;");
        html = html.replace(&format!("{{{{{key}}}}}"), &safe);
    }
    (title.to_owned(), plain, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_email_keeps_plaintext_fallback_and_escapes_html_links() {
        let (title, plain, html) = content("reset", "https://webts.example", "a\"&<b>");
        assert!(title.contains("重置"));
        assert!(plain.contains("https://webts.example/#reset=a\"&<b>"));
        assert!(html.contains("href=\"https://webts.example/#reset=a&quot;&amp;&lt;b&gt;\""));
        assert!(html.contains("活动语音连接将断开"));
        assert!(!html.contains("<img"));
    }
}
