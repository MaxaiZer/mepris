use super::Expr;

pub fn eval_tags_expr(expr: &Expr, tags: &[String]) -> bool {
    match expr {
        Expr::Var(s) => tags.contains(s),
        Expr::Not(e) => !eval_tags_expr(e, tags),
        Expr::And(a, b) => eval_tags_expr(a, tags) && eval_tags_expr(b, tags),
        Expr::Or(a, b) => eval_tags_expr(a, tags) || eval_tags_expr(b, tags),
    }
}
