use miette::{LabeledSpan, NamedSource, Report};

pub struct YamlParseError(pub Report);

impl std::fmt::Debug for YamlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Display for YamlParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for YamlParseError {}

pub fn yaml_error(path: &str, content: &str, err: &serde_yaml::Error, field_path: &str) -> Report {
    let label = calculate_labeled_span(content, err, field_path);

    let inner_msg = err
        .to_string()
        .split(" at line ")
        .next()
        .unwrap_or("unknown error")
        .to_string();

    match label {
        Some(label) => miette::miette!(labels = vec![label], "{inner_msg}")
            .with_source_code(NamedSource::new(path, content.to_string())),

        None => miette::miette!("failed to parse config file `{path}`: {inner_msg}"),
    }
}

fn calculate_labeled_span(
    content: &str,
    err: &serde_yaml::Error,
    field_path: &str,
) -> Option<LabeledSpan> {
    let err_msg = err.to_string();
    if err_msg.contains("found unexpected end of stream") {
        return None;
    }

    let field_error = err_msg.contains("unknown field");

    let fallback_offset = err.location().map(|loc| loc.index()).unwrap_or(0);

    let field_name = if let Some(field) = extract_unknown_field(&err_msg) {
        field
    } else {
        extract_field_name(field_path).unwrap_or("")
    };

    let pattern = format!("{field_name}:");

    let search_start = content[..fallback_offset]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    if field_path == "." || field_path.is_empty() {
        return Some(LabeledSpan::new(
            Some("here".into()),
            fallback_offset,
            remaining_line_length(content, fallback_offset),
        ));
    }

    let (offset, len) = content[search_start..]
        .lines()
        .find_map(|line| {
            let indent = line.len() - line.trim_start().len();

            if !line.trim_start().starts_with(&pattern) {
                return None;
            }

            let line_offset = content[search_start..].find(line)?;
            let absolute_line_offset = search_start + line_offset;

            let field_offset = absolute_line_offset + indent;

            let Some(colon) = line.find(':') else {
                return Some((field_offset, field_name.len()));
            };

            if field_error {
                return Some((field_offset, field_name.len()));
            }

            let value = line[colon + 1..].trim();

            if value.is_empty() {
                Some((field_offset, field_name.len()))
            } else {
                let value_offset = absolute_line_offset
                    + colon
                    + 1
                    + line[colon + 1..]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .count();

                Some((value_offset, value.len()))
            }
        })
        .unwrap_or((fallback_offset, field_name.len()));

    let max_len = content.len().saturating_sub(offset);
    Some(LabeledSpan::new(
        Some("here".into()),
        offset,
        len.min(max_len),
    ))
}

fn extract_field_name(path: &str) -> Option<&str> {
    path.rsplit('.')
        .next()
        .and_then(|part| {
            //ignore array elements, like "requires[1]"
            part.split('[').next()
        })
        .filter(|s| !s.is_empty())
}

fn extract_unknown_field(err: &str) -> Option<&str> {
    err.split("unknown field `").nth(1)?.split('`').next()
}

fn remaining_line_length(content: &str, offset: usize) -> usize {
    let end = content[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(content.len());

    end.saturating_sub(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use tempfile::tempdir;

    /// Helper: writes yaml to a temp file, deserializes into Config via
    /// serde_path_to_error, and returns (content, inner serde_yaml::Error, path_string).
    fn deserialize_err(yaml: &str) -> (String, serde_yaml::Error, String) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.yaml");
        fs::write(&path, yaml).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let de = serde_yaml::Deserializer::from_str(&content);
        let result: Result<Config, _> = serde_path_to_error::deserialize(de);
        let err = result.unwrap_err();
        let field_path = err.path().to_string();
        (content, err.into_inner(), field_path)
    }

    #[test]
    fn unknown_field_in_defaults() {
        let yaml = r#"
        defaults:
          delete_root_dir: true
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "defaults.delete_root_dir");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(
            &content[span.offset()..span.offset() + span.len()],
            "delete_root_dir"
        );
    }

    #[test]
    fn unknown_field_in_step() {
        let yaml = r#"
        steps:
          - id: test
            scrit:
              shell: pwsh
              run: echo hi
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].scrit");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "scrit");
    }

    #[test]
    fn unknown_field_in_full_script() {
        let yaml = r#"
        steps:
          - id: test
            script:
              shll: pwsh
              run: echo hi
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert!(
            !err.to_string()
                .contains("data did not match any variant of untagged enum"),
            "{:?}",
            err.to_string()
        );
        assert_eq!(path, "steps[0].script");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "shll");
    }

    #[test]
    fn unknown_field_in_full_require() {
        let yaml = r#"
        steps:
          - id: test
            requires:
              - id: something1
              - id: something2
                whn: what?
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert!(
            !err.to_string()
                .contains("data did not match any variant of untagged enum"),
            "{:?}",
            err.to_string()
        );
        assert_eq!(path, "steps[0].requires[1]");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "whn");
    }

    #[test]
    fn unknown_field_in_require_script() {
        let yaml = r#"
        steps:
          - id: test
            requires:
              - id: something1
              - id: something2
                when:
                  shl: pwsh
                  script: what
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert!(
            !err.to_string()
                .contains("data did not match any variant of untagged enum"),
            "{:?}",
            err.to_string()
        );
        assert_eq!(path, "steps[0].requires[1]");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "shl");
    }

    #[test]
    fn wrong_type_provides_integer() {
        let yaml = r#"
        steps:
          - id: test
            provides: 42
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].provides");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        println!("span: {:?}", span);
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "42");
    }

    #[test]
    fn wrong_type_requires_string() {
        let yaml = r#"
        steps:
          - id: test
            requires: "not-a-list"
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].requires");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert!(
            content[span.offset()..].lines().next().is_some(),
            "span offset {} out of content bounds",
            span.offset()
        );
        assert_eq!(
            &content[span.offset()..span.offset() + span.len()],
            "\"not-a-list\""
        );
    }

    #[test]
    fn wrong_type_tags_integer() {
        let yaml = r#"
        steps:
          - id: test
            tags: 42
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].tags");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "42");
    }

    #[test]
    fn wrong_type_env_integer() {
        let yaml = r#"
        steps:
          - id: test
            env: 12345
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].env");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "12345");
    }

    #[test]
    fn wrong_type_script_integer() {
        let yaml = r#"
        steps:
          - id: test
            script: 999
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].script");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "999");
    }

    #[test]
    fn wrong_type_check_integer() {
        let yaml = r#"
        steps:
          - id: test
            check: 999
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].check");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "999");
    }

    #[test]
    fn wrong_type_packages_string() {
        let yaml = r#"
        steps:
          - id: test
            packages: "not-a-list"
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].packages");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(
            &content[span.offset()..span.offset() + span.len()],
            "\"not-a-list\""
        );
    }

    #[test]
    fn unknown_top_level_field_with_value() {
        let yaml = r#"
        not_a_real_field: true
        steps:
          - id: test
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "not_a_real_field");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(
            &content[span.offset()..span.offset() + span.len()],
            "not_a_real_field"
        );
    }

    #[test]
    fn unknown_top_level_field_without_value() {
        let yaml = r#"
        not_a_real_field
        steps:
          - id: test
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, ".");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(
            &content[span.offset()..span.offset() + span.len()],
            "not_a_real_field"
        );
    }

    #[test]
    fn missing_colon() {
        let yaml = r#"
        steps
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, ".");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "steps")
    }

    //because error points at the end of file
    #[test]
    fn missing_colon_at_the_end() {
        let yaml = r#"
        steps:
          - id: t
            script: echo
        unknown_field
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].?");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "");
    }

    #[test]
    fn missing_colon_between_root_fields() {
        let yaml = r#"
        steps:
          - id: t
            script: echo
        unknown_field
        defaults:
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].?");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "d");
    }

    #[test]
    fn missing_colon_inside_step() {
        let yaml = r#"
        steps:
          - id: step1
            env
            script: what
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert_eq!(path, "steps[0].?");
        let span = calculate_labeled_span(&content, &err, &path).unwrap();
        assert_eq!(&content[span.offset()..span.offset() + span.len()], "s");
    }

    #[test]
    fn unclosed_yaml_string_has_no_label() {
        let yaml = r#"
        steps:
          - id: t
            script: "
        "#;
        let (content, err, path) = deserialize_err(yaml);
        assert!(err.to_string().contains("found unexpected end of stream"));
        let span = calculate_labeled_span(&content, &err, &path);
        assert!(span.is_none());
    }
}
