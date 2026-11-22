use chrono::NaiveDateTime;

pub const DEFAULT_TEMPLATE: &str = "IMG_{date:YYYYMMDD_HHmmss}_{name}";
const DEFAULT_DATE_FORMAT: &str = "YYYYMMDD_HHmmss";
const DEFAULT_SEQ_WIDTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Literal(String),
    Date(String),
    Name,
    Seq(usize),
    Camera,
}

#[derive(Clone, Debug)]
pub struct Template {
    source: String,
    tokens: Vec<Token>,
}

pub struct RenderContext<'a> {
    pub taken_at: Option<NaiveDateTime>,
    pub name: &'a str,
    pub camera: Option<&'a str>,
    pub seq: usize,
}

enum Segment {
    Literal(String),
    Digits(usize),
    Any,
}

impl Template {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let mut chars = source.chars().peekable();

        while let Some(character) = chars.next() {
            match character {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    literal.push('}');
                }
                '{' => {
                    let mut body = String::new();
                    let mut closed = false;
                    for character in chars.by_ref() {
                        if character == '}' {
                            closed = true;
                            break;
                        }
                        body.push(character);
                    }
                    if !closed {
                        return Err("「{」に対応する「}」がありません".to_owned());
                    }
                    if !literal.is_empty() {
                        tokens.push(Token::Literal(std::mem::take(&mut literal)));
                    }
                    tokens.push(parse_token(body.trim())?);
                }
                '}' => return Err("対応する「{」のない「}」があります".to_owned()),
                _ => literal.push(character),
            }
        }

        if !literal.is_empty() {
            tokens.push(Token::Literal(literal));
        }
        if tokens.is_empty() {
            return Err("テンプレートが空です".to_owned());
        }

        Ok(Self {
            source: source.to_owned(),
            tokens,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn uses_date(&self) -> bool {
        self.tokens
            .iter()
            .any(|token| matches!(token, Token::Date(_)))
    }

    pub fn render(&self, context: &RenderContext<'_>) -> Option<String> {
        let mut output = String::new();
        for token in &self.tokens {
            match token {
                Token::Literal(text) => output.push_str(text),
                Token::Date(format) => {
                    output.push_str(&context.taken_at?.format(format).to_string())
                }
                Token::Name => output.push_str(context.name),
                Token::Seq(width) => output.push_str(&format!("{:0width$}", context.seq)),
                Token::Camera => output.push_str(&camera_text(context.camera)),
            }
        }
        Some(output)
    }

    /// Returns true when `stem` already looks like the output of this template.
    pub fn matches_existing(&self, stem: &str, context: &RenderContext<'_>) -> bool {
        let mut segments = Vec::with_capacity(self.tokens.len());
        for token in &self.tokens {
            let segment = match token {
                Token::Literal(text) => Segment::Literal(text.clone()),
                Token::Date(format) => match context.taken_at {
                    Some(taken_at) => Segment::Literal(taken_at.format(format).to_string()),
                    None => return false,
                },
                Token::Name => Segment::Any,
                Token::Seq(width) => Segment::Digits(*width),
                Token::Camera => Segment::Literal(camera_text(context.camera)),
            };
            segments.push(segment);
        }

        if matches_segments(stem, &segments) {
            return true;
        }
        strip_numeric_suffix(stem).is_some_and(|base| matches_segments(base, &segments))
    }
}

fn parse_token(body: &str) -> Result<Token, String> {
    let (name, argument) = match body.split_once(':') {
        Some((name, argument)) => (name.trim(), Some(argument)),
        None => (body, None),
    };

    match (name, argument) {
        ("name", None) => Ok(Token::Name),
        ("camera", None) => Ok(Token::Camera),
        ("date", None) => convert_date_format(DEFAULT_DATE_FORMAT).map(Token::Date),
        ("date", Some(format)) => convert_date_format(format).map(Token::Date),
        ("seq", None) => Ok(Token::Seq(DEFAULT_SEQ_WIDTH)),
        ("seq", Some(width)) => match width.trim().parse::<usize>() {
            Ok(width @ 1..=9) => Ok(Token::Seq(width)),
            _ => Err(format!(
                "{{seq:N}} の桁数は 1〜9 で指定してください: {width}"
            )),
        },
        _ => Err(format!("不明なトークンです: {{{body}}}")),
    }
}

fn convert_date_format(format: &str) -> Result<String, String> {
    const PATTERNS: [(&str, &str); 7] = [
        ("YYYY", "%Y"),
        ("YY", "%y"),
        ("MM", "%m"),
        ("DD", "%d"),
        ("HH", "%H"),
        ("mm", "%M"),
        ("ss", "%S"),
    ];

    if format.is_empty() {
        return Err("{date:FORMAT} の FORMAT が空です".to_owned());
    }

    let mut output = String::new();
    let mut rest = format;
    'outer: while !rest.is_empty() {
        for (pattern, replacement) in PATTERNS {
            if let Some(remaining) = rest.strip_prefix(pattern) {
                output.push_str(replacement);
                rest = remaining;
                continue 'outer;
            }
        }
        let character = rest.chars().next().expect("rest is not empty");
        if character == '%' {
            output.push_str("%%");
        } else {
            output.push(character);
        }
        rest = &rest[character.len_utf8()..];
    }

    Ok(output)
}

fn camera_text(camera: Option<&str>) -> String {
    let camera = camera.map(str::trim).filter(|camera| !camera.is_empty());
    camera
        .unwrap_or("Unknown")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

fn strip_numeric_suffix(stem: &str) -> Option<&str> {
    let (base, digits) = stem.rsplit_once('_')?;
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(base)
}

fn matches_segments(text: &str, segments: &[Segment]) -> bool {
    let Some((segment, rest)) = segments.split_first() else {
        return text.is_empty();
    };

    match segment {
        Segment::Literal(literal) => text
            .strip_prefix(literal.as_str())
            .is_some_and(|remaining| matches_segments(remaining, rest)),
        Segment::Digits(width) => {
            text.len() >= *width
                && text.as_bytes()[..*width]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
                && matches_segments(&text[*width..], rest)
        }
        Segment::Any => text
            .char_indices()
            .map(|(index, _)| index)
            .skip(1)
            .chain(std::iter::once(text.len()))
            .filter(|index| *index > 0)
            .any(|index| matches_segments(&text[index..], rest)),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{RenderContext, Template, DEFAULT_TEMPLATE};

    fn context(name: &str) -> RenderContext<'_> {
        RenderContext {
            taken_at: NaiveDate::from_ymd_opt(2023, 2, 23)
                .unwrap()
                .and_hms_opt(12, 53, 12),
            name,
            camera: Some("ILCE 7M4"),
            seq: 7,
        }
    }

    #[test]
    fn default_template_matches_the_python_script_output() {
        let template = Template::parse(DEFAULT_TEMPLATE).unwrap();

        assert_eq!(
            template.render(&context("7M400001")).unwrap(),
            "IMG_20230223_125312_7M400001"
        );
    }

    #[test]
    fn renders_date_formats_sequence_camera_and_literals() {
        let template = Template::parse("{date:YYYY-MM-DD}_{seq:3}_{camera}_{{x}}_{name}").unwrap();

        assert_eq!(
            template.render(&context("a.b")).unwrap(),
            "2023-02-23_007_ILCE-7M4_{x}_a.b"
        );
    }

    #[test]
    fn missing_date_cannot_be_rendered() {
        let template = Template::parse(DEFAULT_TEMPLATE).unwrap();
        let mut context = context("x");
        context.taken_at = None;

        assert!(template.uses_date());
        assert_eq!(template.render(&context), None);
    }

    #[test]
    fn rejects_invalid_templates() {
        for source in [
            "",
            "IMG_{date",
            "IMG_}",
            "{unknown}",
            "{seq:0}",
            "{seq:x}",
            "{date:}",
        ] {
            assert!(
                Template::parse(source).is_err(),
                "{source} should be rejected"
            );
        }
    }

    #[test]
    fn recognizes_names_already_produced_by_the_template() {
        let template = Template::parse(DEFAULT_TEMPLATE).unwrap();
        let context = context("unused");

        assert!(template.matches_existing("IMG_20230223_125312_7M400001", &context));
        assert!(template.matches_existing("IMG_20230223_125312_7M400001_001", &context));
        assert!(!template.matches_existing("7M400001", &context));
        assert!(!template.matches_existing("IMG_20230223_125312_", &context));

        let sequence = Template::parse("{date:YYYYMMDD}_{seq:4}").unwrap();
        assert!(sequence.matches_existing("20230223_0042", &context));
        assert!(!sequence.matches_existing("20230223_42", &context));
    }
}
