//! Output filename templating: `{title}-{id}.{ext}` style.
//!
//! Substituted values are sanitized to a single safe path component; literal
//! `/` typed in the template is preserved (directories are intentional).

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Lit(String),
    Field(Field),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Id,
    Ext,
    Height,
    Itag,
    Uploader,
    Index,
}

/// A parsed `-o` template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

impl std::str::FromStr for Template {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = Vec::new();
        let mut rest = s;
        while let Some(open) = rest.find('{') {
            if open > 0 {
                parts.push(Part::Lit(rest[..open].to_string()));
            }
            let after = &rest[open + 1..];
            let close = after
                .find('}')
                .ok_or_else(|| format!("unterminated '{{' in output template {s:?}"))?;
            let field = match &after[..close] {
                "title" => Field::Title,
                "id" => Field::Id,
                "ext" => Field::Ext,
                "height" => Field::Height,
                "itag" => Field::Itag,
                "uploader" => Field::Uploader,
                "index" => Field::Index,
                other => {
                    return Err(format!(
                        "unknown placeholder {{{other}}}: expected title, id, ext, height, itag, uploader, or index"
                    ))
                }
            };
            parts.push(Part::Field(field));
            rest = &after[close + 1..];
        }
        if !rest.is_empty() {
            parts.push(Part::Lit(rest.to_string()));
        }
        Ok(Template { parts })
    }
}

/// Values substituted into a [`Template`].
#[derive(Debug, Default)]
pub struct RenderCtx<'a> {
    /// Video title.
    pub title: &'a str,
    /// Video id.
    pub id: &'a str,
    /// File extension (no dot).
    pub ext: &'a str,
    /// Selected video height, if any.
    pub height: Option<u32>,
    /// Selected itag, if any.
    pub itag: Option<u32>,
    /// Uploader display name, if known.
    pub uploader: Option<&'a str>,
    /// 1-based position within a collection; `None` for single videos.
    pub index: Option<usize>,
}

impl Template {
    /// Substitute and sanitize `ctx` into an output path.
    pub fn render(&self, ctx: &RenderCtx<'_>) -> PathBuf {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Lit(l) => out.push_str(l),
                Part::Field(f) => {
                    let v = match f {
                        Field::Title => ctx.title.to_string(),
                        Field::Id => ctx.id.to_string(),
                        Field::Ext => ctx.ext.to_string(),
                        Field::Height => ctx.height.map(|h| h.to_string()).unwrap_or_default(),
                        Field::Itag => ctx.itag.map(|i| i.to_string()).unwrap_or_default(),
                        Field::Uploader => ctx.uploader.unwrap_or_default().to_string(),
                        Field::Index => ctx.index.map(|i| i.to_string()).unwrap_or_default(),
                    };
                    out.push_str(&sanitize(&v));
                }
            }
        }
        PathBuf::from(out)
    }
}

/// Make a substituted value safe as a single path component.
fn sanitize(v: &str) -> String {
    let mut s: String = v
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Strip trailing dots, spaces, and underscores (which may come from substituted bad chars).
    while s.ends_with('.') || s.ends_with(' ') || s.ends_with('_') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>() -> RenderCtx<'a> {
        RenderCtx {
            title: "A Video",
            id: "abc123",
            ext: "mp4",
            height: Some(1080),
            itag: Some(137),
            uploader: Some("Some One"),
            index: None,
        }
    }

    #[test]
    fn renders_default_template() {
        let t: Template = "{title}.{ext}".parse().unwrap();
        assert_eq!(t.render(&ctx()), PathBuf::from("A Video.mp4"));
    }

    #[test]
    fn sanitizes_substituted_values_but_keeps_literal_dirs() {
        let t: Template = "out/{title}.{ext}".parse().unwrap();
        let mut c = ctx();
        c.title = "a/b: c?";
        assert_eq!(t.render(&c), PathBuf::from("out/a_b_ c.mp4"));
    }

    #[test]
    fn empty_optionals_render_empty() {
        let t: Template = "{index}{uploader}x".parse().unwrap();
        let c = RenderCtx {
            title: "t",
            ..Default::default()
        };
        assert_eq!(t.render(&c), PathBuf::from("x"));
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        let err = "{nope}.mp4".parse::<Template>().unwrap_err();
        assert!(err.contains("unknown placeholder"));
    }

    #[test]
    fn unterminated_brace_is_an_error() {
        let err = "{title.mp4".parse::<Template>().unwrap_err();
        assert!(err.contains("unterminated"));
    }
}
