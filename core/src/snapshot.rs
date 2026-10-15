use crate::{Document, GroupKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnapshotOptions {
    pub groups: Option<Vec<GroupKey>>,
    pub show_notes: bool,
    pub show_subtasks: bool,
}
impl Default for SnapshotOptions {
    fn default() -> Self {
        Self {
            groups: None,
            show_notes: false,
            show_subtasks: true,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub html: String,
    pub style_sha256: String,
}
fn escape(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&#39;"),
            _ => result.push(c),
        }
    }
    result
}
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(char::from(ABC[usize::from(a >> 2)]));
        out.push(char::from(ABC[usize::from((a & 3) << 4 | b >> 4)]));
        out.push(if chunk.len() > 1 {
            char::from(ABC[usize::from((b & 15) << 2 | c >> 6)])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ABC[usize::from(c & 63)])
        } else {
            '='
        });
    }
    out
}
pub fn snapshot_html(doc: &Document, options: &SnapshotOptions) -> Snapshot {
    let mut style = String::from(
        "*{box-sizing:border-box}body{margin:0;background:#fafafa;color:#222;font:16px/1.5 system-ui,sans-serif}main{max-width:44rem;margin:auto;padding:1.25rem}h1{font-size:1.7rem}h2{font-size:1.1rem;text-transform:capitalize;margin-top:2rem}ul{list-style:none;padding:0}li{overflow-wrap:anywhere}.task{background:white;border:1px solid #ddd;border-radius:.5rem;margin:.6rem 0;padding:.8rem}.subtasks{padding-left:1rem;margin-top:.5rem}.note{white-space:pre-wrap;color:#555}.track{height:4px;background:#e3e3e3;margin:.5rem 0}.bar{height:4px;background:#357b61}.count{color:#666;font-weight:normal}bdi{unicode-bidi:plaintext}",
    );
    for i in 0..=100 {
        let _ = write!(style, ".p{i}{{width:{i}%}}");
    }
    let style_sha256 = base64(&Sha256::digest(style.as_bytes()));
    let name = escape(&doc.0.title);
    let mut html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{name}</title><style>{style}</style></head><body><main><h1><bdi>{name}</bdi></h1>"
    );
    for group in &doc.0.groups {
        if options
            .groups
            .as_ref()
            .is_some_and(|keys| !keys.contains(&group.key))
        {
            continue;
        }
        let _ = write!(
            html,
            "<section><h2>{} <span class=\"count\">({})</span></h2><ul>",
            group.key.as_str(),
            group.tasks.len()
        );
        for task in &group.tasks {
            let _ = write!(
                html,
                "<li class=\"task\"><bdi>{}</bdi>",
                escape(&task.title)
            );
            if options.show_notes
                && let Some(note) = &task.note
            {
                let _ = write!(html, "<p class=\"note\"><bdi>{}</bdi></p>", escape(note));
            }
            if !task.subtasks.is_empty() {
                let done = task.subtasks.iter().filter(|s| s.done).count();
                let _ = write!(
                    html,
                    "<div class=\"track\"><div class=\"bar p{}\"></div></div><small>{}/{}</small>",
                    done * 100 / task.subtasks.len(),
                    done,
                    task.subtasks.len()
                );
                if options.show_subtasks {
                    html.push_str("<ul class=\"subtasks\">");
                    for sub in &task.subtasks {
                        let _ = write!(
                            html,
                            "<li>{} <bdi>{}</bdi></li>",
                            if sub.done { "✓" } else { "□" },
                            escape(&sub.title)
                        );
                    }
                    html.push_str("</ul>");
                }
            }
            html.push_str("</li>");
        }
        html.push_str("</ul></section>");
    }
    html.push_str("</main></body></html>");
    Snapshot { html, style_sha256 }
}
