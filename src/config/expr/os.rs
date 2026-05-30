use super::{Expr, parse};
use crate::system::os_info::Platform::{Linux, MacOS, Windows};
use crate::system::os_info::{OsInfo, Platform};
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

#[derive(Debug)]
enum OsCond {
    Os(String),
    IdLike(String),
}

pub fn parse_os_expr<'de, D>(deserializer: D) -> Result<Option<Expr>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<&str> = Option::deserialize(deserializer)?;
    match s {
        Some(inner) => parse(inner).map(Some).map_err(|e| {
            serde::de::Error::custom(format!("Failed to parse OS expr '{inner}': {e}"))
        }),
        None => Ok(None),
    }
}

pub fn eval_os_expr(expr: &Expr, os_info: &OsInfo) -> bool {
    match expr {
        Expr::Var(s) => match parse_term(s) {
            OsCond::Os(id) => id == os_info.platform.as_str() || Some(id) == os_info.id,
            OsCond::IdLike(id) => Some(&id) == os_info.id.as_ref() || os_info.id_like.contains(&id),
        },
        Expr::Not(e) => !eval_os_expr(e, os_info),
        Expr::And(a, b) => eval_os_expr(a, os_info) && eval_os_expr(b, os_info),
        Expr::Or(a, b) => eval_os_expr(a, os_info) || eval_os_expr(b, os_info),
    }
}

fn parse_term(term: &str) -> OsCond {
    let norm = term.to_ascii_lowercase();
    if let Some(rest) = norm.strip_prefix('%') {
        OsCond::IdLike(rest.to_string())
    } else {
        OsCond::Os(norm.to_string())
    }
}

pub fn os_expr_possible_platforms(expr: &Expr) -> HashSet<Platform> {
    let mut result = HashSet::new();

    for os in candidate_os_infos(&expr.vars()) {
        if eval_os_expr(expr, &os) {
            result.insert(os.platform);
        }
    }

    result
}

fn candidate_os_infos(vars: &HashSet<String>) -> Vec<OsInfo> {
    let mut candidates = vec![];

    candidates.push(OsInfo {
        platform: Windows,
        id: None,
        id_like: vec![],
    });

    candidates.push(OsInfo {
        platform: MacOS,
        id: None,
        id_like: vec![],
    });

    candidates.push(OsInfo {
        platform: Linux,
        id: Some("__any__".to_string()),
        id_like: vec![],
    });

    for v in vars {
        let id = match parse_term(v) {
            OsCond::Os(id) => id,
            OsCond::IdLike(id) => id,
        };

        if id != Linux.as_str() && id != Windows.as_str() && id != MacOS.as_str() {
            candidates.push(OsInfo {
                platform: Linux,
                id: Some(id.clone()),
                id_like: vec![],
            });
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hs<const N: usize>(vals: [Platform; N]) -> HashSet<Platform> {
        vals.into_iter().collect()
    }

    #[test]
    fn test_os_expr() {
        let inputs = vec![
            ("ubuntu", true),
            ("Ubuntu", true),
            ("debian", false),
            ("%debian", true),
            ("!%debian", false),
            ("!windows", true),
            ("windows", false),
            ("ubuntu || debian", true),
            ("!ubuntu || debian", false),
            ("!(arch || fedora)", true),
            ("linux && !arch && !fedora", true),
            ("linux && !arch && !fedora && !ubuntu", false),
        ];
        let os_info = OsInfo {
            platform: crate::system::os_info::Platform::Linux,
            id: Some("ubuntu".to_string()),
            id_like: vec!["debian".to_string()],
        };

        for (str, expected) in &inputs {
            let parsed = parse(str).unwrap();
            assert_eq!(
                eval_os_expr(&parsed, &os_info),
                expected.clone(),
                "testing {str}"
            );
        }
    }

    #[test]
    fn test_os_expr_empty_idlike() {
        let inputs = vec![
            ("arch", true),
            ("debian", false),
            ("%arch", true),
            ("!%arch", false),
        ];
        let os_info = OsInfo {
            platform: Linux,
            id: Some("arch".to_string()),
            id_like: vec![],
        };

        for (str, expected) in &inputs {
            let parsed = parse(str).unwrap();
            assert_eq!(
                eval_os_expr(&parsed, &os_info),
                expected.clone(),
                "testing {str}"
            );
        }
    }

    #[test]
    fn test_possible_platforms() {
        let cases = vec![
            ("windows", hs([Windows])),
            ("macos", hs([MacOS])),
            ("linux", hs([Linux])),
            ("!windows", hs([Linux, MacOS])),
            ("windows || macos", hs([Windows, MacOS])),
            ("windows && linux", hs([])),
            ("fedora", hs([Linux])),
            ("fedora || arch", hs([Linux])),
            ("my_custom_distro", hs([Linux])),
            ("windows || fedora", hs([Windows, Linux])),
            ("!(windows) && (fedora || macos)", hs([Linux, MacOS])),
            ("%debian", hs([Linux])),
            ("!%debian", hs([Linux, Windows, MacOS])),
        ];

        for (input, expected) in cases {
            let expr = parse(input).unwrap();
            let actual = os_expr_possible_platforms(&expr);

            assert_eq!(actual, expected, "testing {input}");
        }
    }
}
