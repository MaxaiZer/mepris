use std::{collections::HashSet, io::Write, path::Path};

use crate::{
    cli::ListStepsArgs,
    commands::utils::filter_by_tags,
    config::{Step, expr::eval_os_expr},
    os_info::OS_INFO,
    parser,
};
use anyhow::Result;
use comfy_table::{
    ContentArrangement, Table,
    modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS},
    presets::UTF8_FULL,
};

use super::utils::{check_unique_id, filter_by_os};

pub fn handle(args: ListStepsArgs, out: &mut impl Write) -> Result<()> {
    let steps = parser::parse(&args.file)?;
    check_unique_id(&steps)?;

    let mut steps = steps.iter().collect::<Vec<&Step>>();
    if !args.all {
        steps = filter_by_os(&steps, &OS_INFO).map(|res| res.matching)?;
    }

    if let Some(expr) = args.tags_expr {
        steps = filter_by_tags(&steps, &expr).map(|res| res.matching)?;
    } else if !args.plain {
        let mut all_tags: Vec<&str> = steps
            .iter()
            .flat_map(|step| step.tags.iter().map(|t| t.as_str()))
            .collect();
        all_tags.sort();
        all_tags.dedup();

        writeln!(out, "all tags: {}", all_tags.join(", "))?;
    }

    if args.plain {
        steps
            .iter()
            .for_each(|s| writeln!(out, "{}", s.id).unwrap());
        return Ok(());
    }

    let sources = steps
        .iter()
        .map(|step| step.source_file.as_str())
        .collect::<HashSet<_>>();

    let print_source = sources.len() > 1 || sources.iter().next().unwrap() != &args.file;

    let mut headers = vec!["id", "tags"];
    if args.all {
        headers.push("os");
    }
    if print_source {
        headers.push("file");
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .apply_modifier(UTF8_SOLID_INNER_BORDERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(headers);

    for step in steps.iter() {
        let mut row = vec![step.id.clone(), step.tags.join(", ")];

        if args.all {
            let os_status: String = if let Some(expr) = step.os.as_ref() {
                if eval_os_expr(expr, &OS_INFO) {
                    "✅".to_string()
                } else {
                    "❌".to_string()
                }
            } else {
                "✅".to_string()
            };

            row.push(os_status);
        }
        if print_source {
            let source = Path::new(&step.source_file)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            row.push(source);
        }

        table.add_row(row);
    }
    writeln!(out, "{table}")?;
    Ok(())
}
