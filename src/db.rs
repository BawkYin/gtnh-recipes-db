//! 数据库访问层：建表、物品/候选组/配方/格的插入，以及查询用的统计。
//!
//! 性能要点：
//! - 物品去重靠内存缓存 `(unlocalized_name, meta, nbt) -> id`，同一物品只查一次库；
//! - 批量导入时由调用方用 `BEGIN`/`COMMIT` 包住（见 importer），SQLite 事务内插入快几十倍；
//! - `prepare_cached` 复用预编译语句。

use std::collections::HashMap;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::model::{Category, Ingredient, Item};

pub struct Db {
    pub conn: Connection,
    item_cache: HashMap<(String, i64, String), i64>,
    fluid_cache: HashMap<String, i64>,
}

impl Db {
    /// 打开（或创建）数据库并应用 schema。
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("打开数据库失败：{path}"))?;
        conn.execute_batch(include_str!("../schema.sql"))
            .context("应用 schema.sql 失败")?;
        // 导入期间放宽同步、把临时表放内存：显著加速（数据是派生物，坏了可重建）
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=OFF;
             PRAGMA temp_store=MEMORY;
             PRAGMA cache_size=-200000;",
        )?;
        Ok(Self { conn, item_cache: HashMap::new(), fluid_cache: HashMap::new() })
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 插入/更新一个类别。
    ///
    /// 同一个 id 可能同时来自 NEI 与 GT（同一台机器的两套数据），所以冲突时**合并**：
    /// 名称/模组/amperage 谁有就保留谁，`sources` 追加来源（如 'nei,gt'）。
    pub fn upsert_category(
        &self,
        cat: &Category,
        source: &str,
        amperage: Option<i64>,
        actual_recipe_count: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO categories(id, name, mod_id, mod_name, recipe_count, file, collected_by, sources, amperage)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                 name = COALESCE(categories.name, excluded.name),
                 mod_id = COALESCE(categories.mod_id, excluded.mod_id),
                 mod_name = COALESCE(categories.mod_name, excluded.mod_name),
                 file = COALESCE(categories.file, excluded.file),
                 collected_by = COALESCE(categories.collected_by, excluded.collected_by),
                 amperage = COALESCE(categories.amperage, excluded.amperage),
                 sources = CASE
                     WHEN instr(categories.sources, excluded.sources) > 0 THEN categories.sources
                     ELSE categories.sources || ',' || excluded.sources
                 END",
            params![
                cat.id,
                cat.name,
                cat.mod_id,
                cat.mod_name,
                actual_recipe_count,
                cat.file,
                cat.collected_by,
                source,
                amperage
            ],
        )?;
        Ok(())
    }

    /// 重算每个类别的配方行数（跨来源合计）。导入结束后调用一次即可。
    pub fn recompute_category_counts(&self) -> Result<()> {
        self.conn.execute_batch(
            "UPDATE categories
             SET recipe_count = (SELECT COUNT(*) FROM recipes r WHERE r.category_id = categories.id)",
        )?;
        Ok(())
    }

    /// 取物品 id（不存在则插入）。返回 None 表示该 JSON 条目没有可用的物品名。
    pub fn item_id(&mut self, item: &Item) -> Result<Option<i64>> {
        let Some(name) = item.unlocalized_name.as_deref() else {
            return Ok(None);
        };
        let meta = item.meta.unwrap_or(0);
        let nbt = item.nbt.clone().unwrap_or_default();
        let key = (name.to_string(), meta, nbt.clone());

        if let Some(id) = self.item_cache.get(&key) {
            return Ok(Some(*id));
        }

        let is_fluid = i64::from(name.starts_with("fluid."));
        let is_wildcard = i64::from(meta == 32767);
        self.conn.execute(
            "INSERT OR IGNORE INTO items(unlocalized_name, meta, nbt, display_name, is_fluid, is_wildcard)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![name, meta, nbt, item.display_name, is_fluid, is_wildcard],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM items WHERE unlocalized_name = ?1 AND meta = ?2 AND nbt = ?3",
            params![name, meta, nbt],
            |row| row.get(0),
        )?;
        self.item_cache.insert(key, id);
        Ok(Some(id))
    }

    /// 取流体 id（不存在则插入）。
    pub fn fluid_id(&mut self, name: &str, display_name: Option<&str>) -> Result<i64> {
        if let Some(id) = self.fluid_cache.get(name) {
            return Ok(*id);
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO fluids(name, display_name) VALUES(?1, ?2)",
            params![name, display_name],
        )?;
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM fluids WHERE name = ?1", params![name], |row| row.get(0))?;
        self.fluid_cache.insert(name.to_string(), id);
        Ok(id)
    }

    /// 插入一个"格子/候选组"，返回 group_id。
    pub fn insert_group(&mut self, ingredient: &Ingredient) -> Result<i64> {
        let kind = ingredient.kind.as_str();
        if kind == "oredict" {
            self.conn.execute(
                "INSERT INTO ingredient_groups(kind, ore, amount, chance) VALUES('oredict', ?1, ?2, ?3)",
                params![ingredient.ore, ingredient.amount, ingredient.chance],
            )?;
            return Ok(self.conn.last_insert_rowid());
        }

        self.conn.execute(
            "INSERT INTO ingredient_groups(kind, ore, amount, chance) VALUES(?1, NULL, ?2, ?3)",
            params![kind, ingredient.amount, ingredient.chance],
        )?;
        let group_id = self.conn.last_insert_rowid();

        // 收集候选物品：kind=item 一个；kind=anyOf 一组（矿辞展开的候选表）
        let mut candidates: Vec<&Item> = Vec::new();
        if let Some(item) = ingredient.item.as_ref() {
            candidates.push(item);
        }
        if let Some(items) = ingredient.items.as_ref() {
            for item in items.iter().flatten() {
                candidates.push(item);
            }
        }

        for candidate in candidates {
            if let Some(item_id) = self.item_id(candidate)? {
                let count = candidate.count.unwrap_or(1);
                self.conn.execute(
                    "INSERT OR IGNORE INTO group_candidates(group_id, item_id, count) VALUES(?1, ?2, ?3)",
                    params![group_id, item_id, count],
                )?;
            }
        }
        Ok(group_id)
    }

    /// 插入一条配方，返回 recipe_id。
    #[allow(clippy::too_many_arguments)]
    pub fn insert_recipe(
        &self,
        category_id: &str,
        seq: i64,
        source: &str,
        no_result: bool,
        duration: Option<i64>,
        eut: Option<i64>,
        tier: Option<&str>,
        amperage: Option<i64>,
        special: Option<i64>,
        disabled: Option<bool>,
        hidden: Option<bool>,
        fake: Option<bool>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO recipes(category_id, seq, source, no_result, duration, eut, tier, amperage,
                                 special, disabled, hidden, fake)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                category_id,
                seq,
                source,
                i64::from(no_result),
                duration,
                eut,
                tier,
                amperage,
                special,
                disabled.map(i64::from),
                hidden.map(i64::from),
                fake.map(i64::from)
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 把一组格（可能含 null 空格）插入 item_slots。
    pub fn insert_slots(
        &mut self,
        recipe_id: i64,
        direction: &str,
        cells: &[Option<Ingredient>],
    ) -> Result<()> {
        for (slot_index, cell) in cells.iter().enumerate() {
            let Some(ingredient) = cell else { continue }; // null = 空格
            let group_id = self.insert_group(ingredient)?;
            self.conn.execute(
                "INSERT OR REPLACE INTO item_slots(recipe_id, direction, slot_index, group_id)
                 VALUES(?1, ?2, ?3, ?4)",
                params![recipe_id, direction, slot_index as i64, group_id],
            )?;
        }
        Ok(())
    }

    /// 插入一个流体格。
    pub fn insert_fluid_slot(
        &mut self,
        recipe_id: i64,
        direction: &str,
        slot_index: i64,
        fluid_name: &str,
        display_name: Option<&str>,
        amount: i64,
        chance: Option<i64>,
    ) -> Result<()> {
        let fluid_id = self.fluid_id(fluid_name, display_name)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO fluid_slots(recipe_id, direction, slot_index, fluid_id, amount, chance)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![recipe_id, direction, slot_index, fluid_id, amount, chance],
        )?;
        Ok(())
    }

    /// 统计某张表的行数（校验用）。
    pub fn count(&self, table: &str) -> Result<i64> {
        // table 来自代码内部常量，不涉及注入
        let sql = format!("SELECT COUNT(*) FROM {table}");
        Ok(self.conn.query_row(&sql, [], |row| row.get(0))?)
    }

    /// 按 source 统计配方数（校验用）。
    pub fn count_recipes_by_source(&self, source: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM recipes WHERE source = ?1",
            params![source],
            |row| row.get(0),
        )?)
    }
}
