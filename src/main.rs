//! gtnh-recipes-db：把 ExportRecipe 导出的 JSON 配方数据导入 SQLite。
//!
//! 用法见 `--help`。典型流程：
//! ```text
//! cargo run --release -- --input ../exportrecipe-output --db gtnh-recipes.db
//! ```
//! 导入完成后会打印库内统计，并和 index.json 的总数对账（防止静默丢数据）。

mod db;
mod importer;
mod model;

use std::path::Path;
use std::time::Instant;

use anyhow::Result;

use crate::db::Db;
use crate::importer::{import_gt, import_nei};

const USAGE: &str = "\
gtnh-recipes-db —— 把 ExportRecipe 导出的 JSON 配方数据导入 SQLite

用法:
  gtnh-recipes-db [选项]

选项:
  --input <目录>      导出数据目录（默认 exportrecipe-output）
  --db <文件>         输出数据库文件（默认 gtnh-recipes.db）
  --category <子串>   只导入类别 id 含该子串的类别（调试用）
  --limit <N>         每个类别最多导入 N 条（调试用）
  --no-gt             不导入 gt/ 数值数据集
  -h, --help          显示本帮助
";

struct Args {
    input: String,
    db: String,
    category: Option<String>,
    limit: Option<usize>,
    skip_gt: bool,
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        input: "exportrecipe-output".to_string(),
        db: "gtnh-recipes.db".to_string(),
        category: None,
        limit: None,
        skip_gt: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--input" => args.input = iter.next().unwrap_or_default(),
            "--db" => args.db = iter.next().unwrap_or_default(),
            "--category" => args.category = iter.next(),
            "--limit" => {
                args.limit = iter.next().and_then(|v| v.parse::<usize>().ok());
            }
            "--no-gt" => args.skip_gt = true,
            other => {
                eprintln!("未知参数：{other}\n\n{USAGE}");
                return Ok(None);
            }
        }
    }
    Ok(Some(args))
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else { return Ok(()) };

    println!("数据目录：{}", args.input);
    println!("数据库　：{}", args.db);
    let started = Instant::now();

    let mut db = Db::open(&args.db)?;
    let nei = import_nei(&mut db, Path::new(&args.input), args.category.as_deref(), args.limit)?;
    let gt = if args.skip_gt {
        Default::default()
    } else {
        import_gt(&mut db, Path::new(&args.input), args.category.as_deref())?
    };

    println!("\n===== 导入完成（{:.1} 秒）=====", started.elapsed().as_secs_f64());
    println!("NEI：{} 个类别 / {} 条配方", nei.categories, nei.recipes);
    println!("GT ：{} 个配方图 / {} 条配方", gt.categories, gt.recipes);

    println!("\n===== 库内统计 =====");
    for table in [
        "categories",
        "recipes",
        "items",
        "ingredient_groups",
        "group_candidates",
        "item_slots",
        "fluids",
        "fluid_slots",
    ] {
        println!("  {:<18} {}", table, db.count(table)?);
    }

    // ---- 校验：与源 JSON 声明的总数对账（只在全量导入时有意义）----
    let full_run = args.category.is_none() && args.limit.is_none();
    println!("\n===== 校验 =====");
    if full_run {
        let nei_expected = nei.expected;
        let nei_actual = db.count_recipes_by_source("nei")?;
        println!(
            "  NEI 配方：期望 {} / 实际 {} {}",
            nei_expected,
            nei_actual,
            if nei_expected == nei_actual { "✔" } else { "✘ 不一致！" }
        );
        if gt.expected > 0 {
            let gt_actual = db.count_recipes_by_source("gt")?;
            println!(
                "  GT  配方：期望 {} / 实际 {} {}",
                gt.expected,
                gt_actual,
                if gt.expected == gt_actual { "✔" } else { "✘ 不一致！" }
            );
        }
    } else {
        println!("  （使用了 --category/--limit，跳过总数对账）");
    }

    // ---- 给个直观的"大头"概览 ----
    println!("\n===== 配方最多的类别（前 10）=====");
    let mut stmt = db.conn.prepare(
        "SELECT id, recipe_count, source FROM categories ORDER BY recipe_count DESC LIMIT 10",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    })?;
    for row in rows {
        let (id, count, source) = row?;
        println!("  [{source:>3}] {count:>7}  {id}");
    }

    println!("\n数据库已就绪：{}", args.db);
    Ok(())
}
