pub fn sender(mut from: lettre::message::Mailbox, site: &str) -> lettre::message::Mailbox {
    from.name = Some(site.to_owned());
    from
}

pub fn content(purpose: &str, origin: &str, token: &str, site: &str) -> (String, String, String) {
    let (title, description, action, note) = if purpose == "verify" {
        (
            format!("验证你的 {site} 邮箱"),
            format!("只差一步，就能带上你的身份，与伙伴相聚。请确认这是你用于注册 {site} 的邮箱。"),
            "验证邮箱",
            "验证后即可登录，创建或导入 TeamSpeak 身份。",
        )
    } else {
        (
            format!("重置你的 {site} 密码"),
            "收到了你的密码找回请求。点击下方按钮，为你的账号设置一个新密码。".to_owned(),
            "设置新密码",
            "重置密码后，所有设备会退出登录，活动语音连接将断开；绑定的 TeamSpeak 身份会保留。",
        )
    };
    let link = format!("{origin}/#{purpose}={token}");
    let plain = format!(
        "{title}\n\n{description}\n\n{action}：\n{link}\n\n链接15分钟内有效，仅可使用一次。\n{note}\n\n如果不是你发起的操作，请忽略此邮件。请勿转发链接。\n来自 {site} · {origin}"
    );
    let mut html = include_str!("email.html").to_owned();
    for (key, value) in [
        ("title", title.as_str()),
        ("description", description.as_str()),
        ("action", action),
        ("note", note),
        ("link", &link),
        ("origin", origin),
        ("site", site),
    ] {
        let safe = value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
            .replace('{', "&#123;");
        html = html.replace(&format!("{{{{{key}}}}}"), &safe);
    }
    (title, plain, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_email_keeps_plaintext_fallback_and_escapes_html_links() {
        let (title, plain, html) = content("reset", "https://webts.example", "a\"&<b>", "WebTS");
        assert!(title.contains("重置"));
        assert!(plain.contains("https://webts.example/#reset=a\"&<b>"));
        assert!(html.contains("href=\"https://webts.example/#reset=a&quot;&amp;&lt;b&gt;\""));
        assert!(html.contains("活动语音连接将断开"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn account_emails_use_site_brand_and_escape_it_without_template_expansion() {
        for purpose in ["verify", "reset"] {
            let site = "社区<&\"{{link}}";
            let (subject, plain, html) =
                content(purpose, "https://webts.example", "test-token", site);
            assert!(subject.contains(site));
            assert!(plain.contains(site));
            assert!(html.contains("社区&lt;&amp;&quot;&#123;&#123;link}}"));
            assert!(!html.contains("社区<&"));
            assert!(html.contains("WebTS 是独立开源项目"));
            assert!(html.contains("href=\"https://webts.example/#"));
        }
        let from = sender(
            "Old brand <sender@example.invalid>".parse().unwrap(),
            "新的社区",
        );
        assert_eq!(from.name.as_deref(), Some("新的社区"));
        assert_eq!(from.email.to_string(), "sender@example.invalid");
    }
}
