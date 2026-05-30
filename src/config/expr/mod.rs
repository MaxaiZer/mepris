use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use pest::{Parser, iterators::Pair};
use pest_derive::Parser;

pub mod os;
pub mod tags;

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
