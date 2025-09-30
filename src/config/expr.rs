use std::collections::HashSet;

use crate::os_info::OsInfo;
use anyhow::{Context, Result, bail};
use pest::{Parser, iterators::Pair};
use pest_derive::Parser;
use serde::{Deserialize, Deserializer};

#[derive(Parser)]
#[grammar = "config/expr.pest"]
pub struct ExprParser;

#[derive(Debug, Clone)]
pub enum Expr {
    Var(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    pub fn vars(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        self.collect_vars(&mut result);
        result
    }

    fn collect_vars(&self, set: &mut HashSet<String>) {
        match self {
            Expr::Var(name) => {
                set.insert(name.clone());
            }
            Expr::Not(inner) => {
                inner.collect_vars(set);
            }
            Expr::And(left, right) | Expr::Or(left, right) => {
                left.collect_vars(set);
                right.collect_vars(set);
            }
        }
    }
}

impl Expr {}

fn parse_term(term: &str) -> OsCond {
    let norm = term.to_ascii_lowercase();
    if let Some(rest) = norm.strip_prefix('%') {
        OsCond::IdLike(rest.to_string())
    } else {
        OsCond::Os(norm.to_string())
    }
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

pub fn parse(input: &str) -> Result<Expr> {
    let mut pairs = ExprParser::parse(Rule::expr, input).context("Parse error")?;

    build_expr(pairs.next().unwrap())
}

fn build_expr(pair: Pair<Rule>) -> Result<Expr> {
    match pair.as_rule() {
        Rule::expr => build_expr(pair.into_inner().next().unwrap()),

        Rule::or_expr => {
            let mut inner = pair.into_inner();
            let first = build_expr(inner.next().unwrap())?;
            inner.try_fold(first, |left, right_pair| {
                let right = build_expr(right_pair)?;
                Ok(Expr::Or(Box::new(left), Box::new(right)))
            })
        }

        Rule::and_expr => {
            let mut inner = pair.into_inner();
            let first = build_expr(inner.next().unwrap())?;
            inner.try_fold(first, |left, right_pair| {
                let right = build_expr(right_pair)?;
                Ok(Expr::And(Box::new(left), Box::new(right)))
            })
        }

        Rule::not_expr => {
            let inner = pair.into_inner();
            let mut not_count = 0;
            let mut last = None;

            for p in inner {
                if p.as_rule() == Rule::atom {
                    last = Some(build_expr(p)?);
                    break;
                }
                not_count += 1;
            }

            let mut expr = last.ok_or_else(|| anyhow::anyhow!("Missing atom after !"))?;
            for _ in 0..not_count {
                expr = Expr::Not(Box::new(expr));
            }

            Ok(expr)
        }

        Rule::atom => build_expr(pair.into_inner().next().unwrap()),

        Rule::word => Ok(Expr::Var(pair.as_str().to_string())),

        _ => bail!(format!("Unexpected rule: {:?}", pair.as_rule())),
    }
}

#[derive(Debug)]
enum OsCond {
    Os(String),
    IdLike(String),
}

pub fn eval_os_expr(expr: &Expr, os_info: &OsInfo) -> bool {
    match expr {
        Expr::Var(s) => match parse_term(s) {
            OsCond::Os(id) => id == os_info.platform.as_str() || Some(id) == os_info.id,
            OsCond::IdLike(id) => os_info.id_like.contains(&id),
        },
        Expr::Not(e) => !eval_os_expr(e, os_info),
        Expr::And(a, b) => eval_os_expr(a, os_info) && eval_os_expr(b, os_info),
        Expr::Or(a, b) => eval_os_expr(a, os_info) || eval_os_expr(b, os_info),
    }
}

pub fn eval_tags_expr(expr: &Expr, tags: &[String]) -> bool {
    match expr {
        Expr::Var(s) => tags.contains(s),
        Expr::Not(e) => !eval_tags_expr(e, tags),
        Expr::And(a, b) => eval_tags_expr(a, tags) && eval_tags_expr(b, tags),
        Expr::Or(a, b) => eval_tags_expr(a, tags) || eval_tags_expr(b, tags),
    }
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
        platform: crate::os_info::Platform::Linux,
        id: Some("ubuntu".to_string()),
        id_like: vec!["debian".to_string()],
    };

    for (str, expected) in &inputs {
        let parsed = parse(str).unwrap();
        dbg!(str, &parsed);
        assert_eq!(
            eval_os_expr(&parsed, &os_info),
            expected.clone(),
            "testing {str}"
        );
    }
}
