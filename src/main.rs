//! gtnh-recipes-db：把 ExportRecipe 导出的 JSON 配方数据导入 SQLite，并提供常用查询。
//!
//! 子命令一览（`--help` 有完整说明）：
//! - `import`  导入 JSON → SQLite（默认子命令）
//! - `item`    按名称片段查物品（拿 id）
//! - `reverse` 什么东西能做出它（带 EU/t、时长、概率）
//! - `forward` 它被哪些机器/类别消耗
//! - `chain`   生产链递归展开（需要什么才能做出来）
//! - `csv`     把某个视图导出成 CSV
//! - `views`   列出数据库里的视图

mod db;
mod importer;
mod model;
mod query;

use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Result};

use crate::db::Db;
use crate::importer::{import_gt, import_nei};
use crate::query as q;

const USAGE: &str = "\
gtnh-recipes-db —— ExportRecipe 的 JSON 导出 → SQLite 数据库，并提供常用查询

用法:
  gtnh-recipes-db [子命令] [参数] [选项]

子命令:
  import                      把 JSON 导入数据库（缺省子命令）
  item <名称片段>             查物品（拿 id；支持 #123 直接按 id）
  reverse <名称|#id>          反向：什么东西能做出它（GT 数据带 EU/t、时长、概率）
  forward <名称|#id>          正向：它被哪些机器/类别消耗
  chain <名称|#id>            生产链：递归展开\"需要什么才能做出来\"
  csv <视图名>                把视图/表导出成 CSV（默认写 stdout，--out 写文件）
  views                       列出数据库里的视图

选项:
  --db <文件>         数据库文件（默认 gtnh-recipes.db）
  --input <目录>      导出数据目录，仅 import 用（默认 exportrecipe-output）
  --category <子串>   仅 import：只导入类别 id 含该子串的类别
  --import-limit <N>  仅 import：每个类别最多导入 N 条（调试用）
  --no-gt             仅 import：不导入 gt/ 数值数据集
  --source <来源>     any | nei | gt（查询用，默认 any）
  --limit <N>         查询结果条数上限（默认 20）
  --depth <N>         chain 的递归深度（默认 3）
  --out <文件>        csv 的输出文件（缺省写 stdout）
  -h, --help          显示本帮助

示例:
  # 导入（740MB JSON，约 25 秒）
  gtnh-recipes-db import --input /path/to/exportrecipe-output --db gtnh-recipes.db

  # 查物品 id
  gtnh-recipes-db item 铁锭

  # 铁锭怎么来的 / 能用来做什么（只看 GT 数值数据）
  gtnh-recipes-db reverse 铁锭 --source gt
  gtnh-recipes-db forward 铁锭

  # 三层生产链
  gtnh-recipes-db chain 铁锭 --depth 3 --limit 50

  # 导出 GT 配方表为 CSV
  gtnh-recipes-db csv v_gt_recipes --out gt_recipes.csv
";

struct Opts {
    db: String,
    input: String,
    category: Option<String>,
    import_limit: Option<usize>,
    skip_gt: bool,
    source: String,
    limit: usize,
    depth: i64,
    out: Option<String>,
}

fn main() -> Result<()> {
    let mut opts = Opts {
        db: "gtnh-recipes.db".to_string(),
        input: "exportrecipe-output".to_string(),
        category: None,
        import_limit: None,
        skip_gt: false,
        source: "any".to_string(),
        limit: 20,
        depth: 3,
        out: None,
    };

    let mut positionals: Vec<String> = Vec::new();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--db" => opts.db = iter.next().unwrap_or_default(),
            "--input" => opts.input = iter.next().unwrap_or_default(),
            "--category" => opts.category = iter.next(),
            "--import-limit" => opts.import_limit = iter.next().and_then(|v| v.parse().ok()),
            "--no-gt" => opts.skip_gt = true,
            "--source" => opts.source = iter.next().unwrap_or_else(|| "any".to_string()),
            "--limit" => opts.limit = iter.next().and_then(|v| v.parse().ok()).unwrap_or(20),
            "--depth" => opts.depth = iter.next().and_then(|v| v.parse().ok()).unwrap_or(3),
            "--out" => opts.out = iter.next(),
            other => positionals.push(other.to_string()),
        }
    }

    let mode = positionals
        .first()
        .cloned()
        .unwrap_or_else(|| "import".to_string());
    let argument = positionals.get(1).cloned();

    match mode.as_str() {
        "import" => run_import(&opts),
        "item" => {
            let needle = need(argument, "item <名称片段>")?;
            let db = Db::open(&opts.db)?;
            q::print_items(&db.conn, &needle, opts.limit)
        }
        "reverse" => {
            let needle = need(argument, "reverse <名称|#id>")?;
            let db = Db::open(&opts.db)?;
            let item = q::resolve_item(&db.conn, &needle)?;
            q::print_reverse(&db.conn, &item, &opts.source, opts.limit)
        }
        "forward" => {
            let needle = need(argument, "forward <名称|#id>")?;
            let db = Db::open(&opts.db)?;
            let item = q::resolve_item(&db.conn, &needle)?;
            q::print_forward(&db.conn, &item, &opts.source, opts.limit)
        }
        "chain" => {
            let needle = need(argument, "chain <名称|#id>")?;
            let db = Db::open(&opts.db)?;
            let item = q::resolve_item(&db.conn, &needle)?;
            q::print_chain(&db.conn, &item, &opts.source, opts.depth, opts.limit)
        }
        "csv" => {
            let view = need(argument, "csv <视图名>")?;
            let db = Db::open(&opts.db)?;
            q::export_csv(&db.conn, &view, opts.out.as_deref())
        }
        "views" => {
            let db = Db::open(&opts.db)?;
            println!("数据库中的视图：");
            let mut stmt = db
                .conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                println!("  {}", row?);
            }
            Ok(())
        }
        other => {
            eprintln!("未知子命令：{other}\n\n{USAGE}");
            Ok(())
        }
    }
}

fn need(argument: Option<String>, usage: &str) -> Result<String> {
    match argument {
        Some(value) if !value.is_empty() => Ok(value),
        _ => bail!("缺少参数。用法：gtnh-recipes-db {usage}"),
    }
}

fn run_import(opts: &Opts) -> Result<()> {
    println!("数据目录：{}", opts.input);
    println!("数据库　：{}", opts.db);
    let started = Instant::now();

    let mut db = Db::open(&opts.db)?;
    let nei = import_nei(
        &mut db,
        Path::new(&opts.input),
        opts.category.as_deref(),
        opts.import_limit,
    )?;
    let gt = if opts.skip_gt {
        Default::default()
    } else {
        import_gt(&mut db, Path::new(&opts.input), opts.category.as_deref())?
    };

    println!(
        "\n===== 导入完成（{:.1} 秒）=====",
        started.elapsed().as_secs_f64()
    );
    println!("NEI：{} 个类别 / {} 条配方", nei.categories, nei.recipes);
    println!("GT ：{} 个配方图 / {} 条配方", gt.categories, gt.recipes);

    // 类别是 NEI 与 GT 的并集（同名 id 合并），重算每类的配方行数
    db.recompute_category_counts()?;

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
    let full_run = opts.category.is_none() && opts.import_limit.is_none();
    println!("\n===== 校验 =====");
    if full_run {
        let nei_actual = db.count_recipes_by_source("nei")?;
        println!(
            "  NEI 配方：期望 {} / 实际 {} {}",
            nei.expected,
            nei_actual,
            if nei.expected == nei_actual {
                "✔"
            } else {
                "✘ 不一致！"
            }
        );
        if gt.expected > 0 {
            let gt_actual = db.count_recipes_by_source("gt")?;
            println!(
                "  GT  配方：期望 {} / 实际 {} {}",
                gt.expected,
                gt_actual,
                if gt.expected == gt_actual {
                    "✔"
                } else {
                    "✘ 不一致！"
                }
            );
        }
    } else {
        println!("  （使用了 --category/--import-limit，跳过总数对账）");
    }

    println!("\n===== 配方最多的类别（前 10）=====");
    let mut stmt = db.conn.prepare(
        "SELECT id, recipe_count, sources FROM categories ORDER BY recipe_count DESC LIMIT 10",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, count, sources) = row?;
        println!("  [{sources:>7}] {count:>7}  {id}");
    }

    println!("\n数据库已就绪：{}", opts.db);
    println!("下一步试试：gtnh-recipes-db item 铁锭");
    Ok(())
}
