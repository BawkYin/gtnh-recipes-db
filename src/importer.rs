//! 导入逻辑：把导出目录里的 JSON 读进来，规范化后写入 SQLite。
//!
//! 两套数据集（互不覆盖）：
//! - NEI 通用：index.json + recipes/*.json → source = 'nei'
//! - GT 数值：gt/index.json + gt/*.json   → source = 'gt'（带 duration/eut/tier 等）

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use crate::db::Db;
use crate::model::{Category, CategoryFile, GtCategoryFile, GtIndexFile, GtItem, Ingredient, Item, IndexFile};

#[derive(Debug, Default)]
pub struct ImportStats {
    pub categories: usize,
    pub recipes: usize,
    pub skipped: usize,
    /// 源 index.json 声明的配方总数（用于导入后的对账校验）
    pub expected: i64,
}

/// 从文件读 JSON（大文件用 BufReader，避免把整个文件读成字符串再解析）。
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = File::open(path).with_context(|| format!("打不开文件：{}", path.display()))?;
    let reader = BufReader::with_capacity(1 << 20, file);
    serde_json::from_reader(reader).with_context(|| format!("解析 JSON 失败：{}", path.display()))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// 导入 NEI 通用数据集。
pub fn import_nei(db: &mut Db, input: &Path, filter: Option<&str>, limit: Option<usize>) -> Result<ImportStats> {
    let index_path = input.join("index.json");
    let index: IndexFile = read_json(&index_path)?;

    db.set_meta("nei_schema", &index.schema.to_string())?;
    db.set_meta("nei_source_path", &input.display().to_string())?;
    if let Some(generated_at) = index.generated_at.as_deref() {
        db.set_meta("nei_generated_at", generated_at)?;
    }
    println!(
        "NEI 索引：schema={} 类别={} 配方={}",
        index.schema,
        index.categories.len(),
        index.recipe_total
    );

    let mut stats = ImportStats::default();
    stats.expected = index.recipe_total;
    for category in &index.categories {
        if let Some(needle) = filter {
            if !category.id.contains(needle) {
                stats.skipped += 1;
                continue;
            }
        }
        let Some(file) = category.file.as_deref() else {
            stats.skipped += 1;
            continue;
        };
        let path = input.join(file);
        if !path.is_file() {
            stats.skipped += 1;
            continue;
        }

        let category_file: CategoryFile = read_json(&path)?;
        let total = category_file.recipes.len();

        // 一个类别一个事务：SQLite 事务内插入快几十倍，且失败可整类回滚
        db.conn.execute_batch("BEGIN")?;
        let mut inserted = 0usize;
        for (seq, recipe) in category_file.recipes.iter().enumerate() {
            if let Some(max) = limit {
                if inserted >= max {
                    break;
                }
            }
            let no_result = recipe.no_result.unwrap_or(false);
            let recipe_id = db.insert_recipe(
                &category.id,
                seq as i64,
                "nei",
                no_result,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )?;
            db.insert_slots(recipe_id, "input", &recipe.ingredients)?;
            db.insert_slots(recipe_id, "result", &recipe.results)?;
            if let Some(others) = recipe.others.as_ref() {
                db.insert_slots(recipe_id, "other", others)?;
            }
            inserted += 1;
        }
        db.upsert_category(category, "nei", None, inserted as i64)?;
        db.conn.execute_batch("COMMIT")?;

        stats.categories += 1;
        stats.recipes += inserted;
        println!(
            "  [NEI] {:<58} {:>7}/{} 条",
            truncate(&category.id, 58),
            inserted,
            total
        );
    }
    Ok(stats)
}

/// 导入 GT 数值数据集（gt/ 目录存在时才做）。
pub fn import_gt(db: &mut Db, input: &Path, filter: Option<&str>) -> Result<ImportStats> {
    let gt_dir = input.join("gt");
    let index_path = gt_dir.join("index.json");
    if !index_path.is_file() {
        println!("未发现 gt/index.json，跳过 GT 数据集");
        return Ok(ImportStats::default());
    }

    let index: GtIndexFile = read_json(&index_path)?;
    db.set_meta("gt_schema", &index.schema.to_string())?;
    if let Some(generated_at) = index.generated_at.as_deref() {
        db.set_meta("gt_generated_at", generated_at)?;
    }
    println!(
        "GT 索引：schema={} 配方图={} 配方={}",
        index.schema,
        index.maps.len(),
        index.recipe_total
    );

    let mut stats = ImportStats::default();
    stats.expected = index.recipe_total;
    for map in &index.maps {
        if let Some(needle) = filter {
            if !map.id.contains(needle) {
                stats.skipped += 1;
                continue;
            }
        }
        let Some(file) = map.file.as_deref() else {
            stats.skipped += 1;
            continue;
        };
        let path = input.join(file);
        if !path.is_file() {
            stats.skipped += 1;
            continue;
        }

        let category_file: GtCategoryFile = read_json(&path)?;
        let total = category_file.recipes.len();

        db.conn.execute_batch("BEGIN")?;
        let mut inserted = 0usize;
        for (seq, recipe) in category_file.recipes.iter().enumerate() {
            let recipe_id = db.insert_recipe(
                &map.id,
                seq as i64,
                "gt",
                false,
                recipe.duration,
                recipe.eut,
                recipe.tier.as_deref(),
                recipe.amperage.or(map.amperage),
                recipe.special,
                recipe.disabled,
                recipe.hidden,
                recipe.fake,
            )?;
            insert_gt_item_slots(db, recipe_id, "input", &recipe.inputs)?;
            insert_gt_item_slots(db, recipe_id, "result", &recipe.outputs)?;
            insert_gt_fluid_slots(db, recipe_id, "input", &recipe.fluid_inputs)?;
            insert_gt_fluid_slots(db, recipe_id, "result", &recipe.fluid_outputs)?;
            inserted += 1;
        }

        // GT 的类别信息来自 gt/index.json（有 amperage，没有 mod 名）
        let category = Category {
            id: map.id.clone(),
            name: None,
            mod_id: Some("gregtech".to_string()),
            mod_name: Some("GregTech".to_string()),
            recipe_count: total as i64,
            file: map.file.clone(),
            collected_by: None,
        };
        db.upsert_category(&category, "gt", map.amperage, inserted as i64)?;
        db.conn.execute_batch("COMMIT")?;

        stats.categories += 1;
        stats.recipes += inserted;
        println!("  [GT ] {:<58} {:>7}/{} 条", truncate(&map.id, 58), inserted, total);
    }
    Ok(stats)
}

/// GT 物品格：每个物品自带概率 → 一格 = 一个单候选组。
fn insert_gt_item_slots(db: &mut Db, recipe_id: i64, direction: &str, cells: &[Option<GtItem>]) -> Result<()> {
    for (slot_index, cell) in cells.iter().enumerate() {
        let Some(gt_item) = cell else { continue };
        let Some(item) = gt_item.item.as_ref() else { continue };
        let ingredient = Ingredient {
            kind: "item".to_string(),
            item: Some(Item { ..item.clone() }),
            ore: None,
            items: None,
            amount: None,
            chance: gt_item.chance,
        };
        let group_id = db.insert_group(&ingredient)?;
        db.conn.execute(
            "INSERT OR REPLACE INTO item_slots(recipe_id, direction, slot_index, group_id)
             VALUES(?1, ?2, ?3, ?4)",
            rusqlite::params![recipe_id, direction, slot_index as i64, group_id],
        )?;
    }
    Ok(())
}

/// GT 流体格（真正的流体表，带毫桶数量与概率）。
fn insert_gt_fluid_slots(
    db: &mut Db,
    recipe_id: i64,
    direction: &str,
    cells: &[Option<crate::model::GtFluid>],
) -> Result<()> {
    for (slot_index, cell) in cells.iter().enumerate() {
        let Some(fluid) = cell else { continue };
        let Some(name) = fluid.fluid.as_deref() else { continue };
        db.insert_fluid_slot(
            recipe_id,
            direction,
            slot_index as i64,
            name,
            fluid.display_name.as_deref(),
            fluid.amount.unwrap_or(0),
            fluid.chance,
        )?;
    }
    Ok(())
}
