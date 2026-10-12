use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, EndTag, StartTag, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
        states::RawKind,
    },
};
use std::cell::{Cell, RefCell};
#[derive(Default)]
struct Metadata {
    title: RefCell<String>,
    description: RefCell<String>,
    image: RefCell<String>,
    fallback: RefCell<String>,
    in_title: Cell<bool>,
    body: Cell<bool>,
}
fn clean(s: &str, max: usize) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}
impl TokenSink for Metadata {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(t) if t.kind == StartTag => match t.name.as_ref() {
                "body" => self.body.set(true),
                "title" => {
                    self.in_title.set(true);
                    return TokenSinkResult::RawData(RawKind::Rcdata);
                }
                "script" => return TokenSinkResult::RawData(RawKind::ScriptData),
                "style" | "iframe" | "xmp" | "noscript" => {
                    return TokenSinkResult::RawData(RawKind::Rawtext);
                }
                "textarea" => return TokenSinkResult::RawData(RawKind::Rcdata),
                "meta" if !self.body.get() => {
                    let attr = |name: &str| {
                        t.attrs
                            .iter()
                            .find(|a| &*a.name.local == name)
                            .map(|a| a.value.as_ref())
                            .unwrap_or("")
                    };
                    let property = if attr("property").is_empty() {
                        attr("name")
                    } else {
                        attr("property")
                    };
                    let target = match property.to_ascii_lowercase().as_str() {
                        "og:title" => Some((&self.title, 120)),
                        "og:description" | "description" => Some((&self.description, 1200)),
                        "og:image" => Some((&self.image, 2048)),
                        _ => None,
                    };
                    if let Some((target, max)) = target
                        && target.borrow().is_empty()
                    {
                        *target.borrow_mut() = clean(attr("content"), max);
                    }
                }
                _ => {}
            },
            Token::TagToken(t) if t.kind == EndTag && &*t.name == "title" => {
                self.in_title.set(false)
            }
            Token::CharacterTokens(s) if self.in_title.get() => {
                let mut title = self.fallback.borrow_mut();
                if title.len() < 4096 {
                    title.push_str(&s);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
pub fn parse(html: &str) -> (String, String, String) {
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(html));
    let tok = Tokenizer::new(Metadata::default(), TokenizerOpts::default());
    let _ = tok.feed(&input);
    tok.end();
    let title = if tok.sink.title.borrow().is_empty() {
        clean(&tok.sink.fallback.borrow(), 120)
    } else {
        tok.sink.title.borrow().clone()
    };
    (
        title,
        tok.sink.description.borrow().clone(),
        tok.sink.image.borrow().clone(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_decodes_entities_and_ignores_script_fake_tags() {
        assert_eq!(
            parse(
                r#"<head><script>var x='<meta property="og:title" content="FAKE">';</script><meta property="og:title" content="CS2 &amp; Friends"><meta name="description" content="hello &lt;b&gt;world&lt;/b&gt;"></head>"#
            ),
            (
                "CS2 & Friends".into(),
                "hello <b>world</b>".into(),
                "".into()
            )
        );
    }
    #[test]
    fn title_fallback_is_bounded_and_body_metadata_is_ignored() {
        assert_eq!(
            parse("<title> Live\n Match </title><body><meta property='og:title' content='wrong'>")
                .0,
            "Live Match"
        );
        assert_eq!(
            parse(&format!("<title>{}</title>", "a".repeat(500)))
                .0
                .len(),
            120
        );
    }
}
