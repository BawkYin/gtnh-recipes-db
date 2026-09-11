//! 查询与导出：物品解析、反向/正向/生产链查询、CSV 子集导出。
//!
//! 数据访问函数尽量返回结构化结果（Vec<...>），打印只是薄薄一层包装——
//! 这样单元测试可以直接断言结果，不依赖控制台输出。

use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::{bail, Context, Result};
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection, Row};

/// 解析出来的物品（items 表的一行）
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedItem {
    pub id: i64,
    pub unlocalized_name: String,
    pub meta: i64,
    pub display_name: Option<String>,
    pub is_fluid: i64,
    pub is_wildcard: i64,
}

impl ResolvedItem {
    pub fn label(&self) -> String {
        let name = self
            .display_name
            .clone()
            .unwrap_or_else(|| self.unlocalized_name.clone());
        let mut flags = Vec::new();
        if self.is_fluid != 0 {
            flags.push("流体伪物品");
        }
        if self.is_wildcard != 0 {
            flags.push("通配 meta");
        }
        let flag_text = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join("、"))
        };
        format!(
            "#{} {} （{}@{}）{}",
            self.id, name, self.unlocalized_name, self.meta, flag_text
        )
    }
}

/// 反向查询的一行：某个配方能产出目标物品
#[derive(Debug, Clone, PartialEq)]
pub struct ReverseRow {
    pub category_id: String,
    pub category_name: Option<String>,
    pub source: String,
    pub eut: Option<i64>,
    pub duration: Option<i64>,
    pub tier: Option<String>,
    pub chance: Option<i64>,
    pub count: i64,
}

/// 正向查询的一行：某个类别会消耗目标物品
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardRow {
    pub category_id: String,
    pub category_name: Option<String>,
    pub sources: String,
    pub uses: i64,
}

const ITEM_COLS: &str = "id, unlocalized_name, meta, display_name, is_fluid, is_wildcard";

fn item_from_row(row: &Row<'_>) -> rusqlite::Result<ResolvedItem> {
    Ok(ResolvedItem {
        id: row.get(0)?,
        unlocalized_name: row.get(1)?,
        meta: row.get(2)?,
        display_name: row.get(3)?,
        is_fluid: row.get(4)?,
        is_wildcard: row.get(5)?,
    })
}

/// 解析物品：支持 `#123`（按 id）、精确匹配（显示名/未本地化名）、模糊匹配（取第一个）。
pub fn resolve_item(conn: &Connection, needle: &str) -> Result<ResolvedItem> {
    if let Some(id_text) = needle.strip_prefix('#') {
        let id: i64 = id_text
            .parse()
            .with_context(|| format!("无效的物品 id：{needle}"))?;
        return item_by_id(conn, id);
    }

    let exact = format!(
        "SELECT {ITEM_COLS} FROM items WHERE display_name = ?1 OR unlocalized_name = ?1 LIMIT 1"
    );
    if let Some(item) = query_one_item(conn, &exact, needle)? {
        return Ok(item);
    }

    let fuzzy = format!(
        "SELECT {ITEM_COLS} FROM items
         WHERE display_name LIKE '%' || ?1 || '%' OR unlocalized_name LIKE '%' || ?1 || '%'
         ORDER BY id LIMIT 1"
    );
    if let Some(item) = query_one_item(conn, &fuzzy, needle)? {
        return Ok(item);
    }
    bail!("找不到匹配物品：{needle}（可先用 item 子命令模糊查一下）")
}

fn item_by_id(conn: &Connection, id: i64) -> Result<ResolvedItem> {
    let sql = format!("SELECT {ITEM_COLS} FROM items WHERE id = ?1");
    conn.query_row(&sql, [id], item_from_row)
        .with_context(|| format!("找不到物品 id={id}"))
}

fn query_one_item(conn: &Connection, sql: &str, needle: &str) -> Result<Option<ResolvedItem>> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([needle])?;
    match rows.next()? {
        Some(row) => Ok(Some(item_from_row(row)?)),
        None => Ok(None),
    }
}

/// 模糊列出候选物品（item 子命令）。
pub fn find_items(conn: &Connection, needle: &str, limit: usize) -> Result<Vec<ResolvedItem>> {
    let sql = format!(
        "SELECT {ITEM_COLS} FROM items
         WHERE display_name LIKE '%' || ?1 || '%' OR unlocalized_name LIKE '%' || ?1 || '%'
         ORDER BY CASE WHEN display_name = ?1 THEN 0 WHEN unlocalized_name = ?1 THEN 1 ELSE 2 END, id
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![needle, limit as i64], item_from_row)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

/// 反向：哪些配方产出该物品（可限定来源、按 EU/t 升序）。
pub fn reverse_rows(
    conn: &Connection,
    item_id: i64,
    source: &str,
    limit: usize,
) -> Result<Vec<ReverseRow>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, r.source, r.eut, r.duration, r.tier, g.chance, gc.count
         FROM group_candidates gc
         JOIN ingredient_groups g ON g.id = gc.group_id
         JOIN item_slots s        ON s.group_id = gc.group_id AND s.direction = 'result'
         JOIN recipes r           ON r.id = s.recipe_id
         JOIN categories c        ON c.id = r.category_id
         WHERE gc.item_id = ?1 AND (?2 = 'any' OR r.source = ?2)
         ORDER BY (r.eut IS NULL), r.eut, c.id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![item_id, source, limit as i64], |row| {
        Ok(ReverseRow {
            category_id: row.get(0)?,
            category_name: row.get(1)?,
            source: row.get(2)?,
            eut: row.get(3)?,
            duration: row.get(4)?,
            tier: row.get(5)?,
            chance: row.get(6)?,
            count: row.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 正向：该物品被哪些类别当作输入消耗（按次数降序）。
pub fn forward_rows(
    conn: &Connection,
    item_id: i64,
    source: &str,
    limit: usize,
) -> Result<Vec<ForwardRow>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.sources, COUNT(*) AS uses
         FROM group_candidates gc
         JOIN item_slots s ON s.group_id = gc.group_id AND s.direction = 'input'
         JOIN recipes r    ON r.id = s.recipe_id
         JOIN categories c ON c.id = r.category_id
         WHERE gc.item_id = ?1 AND (?2 = 'any' OR r.source = ?2)
         GROUP BY c.id, c.name, c.sources
         ORDER BY uses DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![item_id, source, limit as i64], |row| {
        Ok(ForwardRow {
            category_id: row.get(0)?,
            category_name: row.get(1)?,
            sources: row.get(2)?,
            uses: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 生产链的一个节点
#[derive(Debug, Clone, PartialEq)]
pub struct ChainNode {
    pub depth: i64,
    pub item: String,
    pub via: Option<String>,
    /// 备注：例如"任意 12 选 1 的代表"（输入格有多个候选时只展开代表）
    pub note: Option<String>,
}

/// 单个物品最多看几个产出配方（避免"物品 → 几千条配方"的发散）
const MAX_PRODUCERS_PER_ITEM: usize = 4;

/// 生产链：有界广度优先展开"做出目标物品需要什么"。
///
/// **为什么不用递归 CTE**：真实库里有 47 万条配方、268 万条候选边，且一个输入格
/// 可能有 251 个候选物品。纯 SQL 递归会在"每个候选都要找它的产出配方"上指数爆炸
/// （实测 60 秒都跑不完）。这里改成 BFS + 三重上限：
/// ① 每个物品最多看 `MAX_PRODUCERS_PER_ITEM` 个产出配方；② 每个输入格只取一个代表
/// （并标注"任意 N 选 1"）；③ 总节点数上限 `budget`。每一步都是索引查找，毫秒级。
pub fn chain_rows(
    conn: &Connection,
    item_id: i64,
    source: &str,
    depth: i64,
    budget: usize,
) -> Result<Vec<ChainNode>> {
    use std::collections::{HashSet, VecDeque};

    let mut visited: HashSet<i64> = HashSet::new();
    let mut printed: HashSet<(i64, String)> = HashSet::new();
    let mut queue: VecDeque<(i64, i64)> = VecDeque::new();
    let mut out: Vec<ChainNode> = Vec::new();

    visited.insert(item_id);
    queue.push_back((item_id, 0));

    while let Some((current, current_depth)) = queue.pop_front() {
        if current_depth >= depth || out.len() >= budget {
            continue;
        }

        // ① 产出该物品的配方（最多几个）
        let producers = producer_rows(conn, current, source, MAX_PRODUCERS_PER_ITEM)?;
        for (recipe_id, category_id) in producers {
            if out.len() >= budget {
                break;
            }
            // ② 该配方的输入格：每组只取一个代表 + 候选总数
            for input in input_groups(conn, recipe_id)? {
                if out.len() >= budget {
                    break;
                }
                let Some(rep_item_id) = input.rep_item_id else {
                    continue;
                };
                if !visited.insert(rep_item_id) {
                    continue; // 已经见过（含环路保护）
                }

                let note = if input.candidate_count > 1 {
                    Some(format!(
                        "任意 {} 选 1 的代表{}",
                        input.candidate_count,
                        input
                            .ore
                            .as_deref()
                            .map(|ore| format!("（矿辞 {ore}）"))
                            .unwrap_or_default()
                    ))
                } else {
                    None
                };

                out.push(ChainNode {
                    depth: current_depth + 1,
                    item: input.rep_name.unwrap_or_else(|| format!("#{rep_item_id}")),
                    via: Some(category_id.clone()),
                    note,
                });
                // 同名不同 meta 的物品很多（如"焙烧铁矿石"），展示时按 (深度, 名称) 去噪，
                // 但遍历仍然继续（已通过 visited 保证每个 item_id 只展开一次）
                let node = out.last().expect("刚 push 过");
                if !printed.insert((node.depth, node.item.clone())) {
                    out.pop();
                }
                queue.push_back((rep_item_id, current_depth + 1));
            }
        }
    }

    Ok(out)
}

/// 产出某物品的配方（recipe_id, category_id），按 EU/t 升序（无法比较的排后面）。
fn producer_rows(
    conn: &Connection,
    item_id: i64,
    source: &str,
    limit: usize,
) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT r.id, r.category_id
         FROM group_candidates gc
         JOIN item_slots s ON s.group_id = gc.group_id AND s.direction = 'result'
         JOIN recipes r    ON r.id = s.recipe_id
         WHERE gc.item_id = ?1 AND (?2 = 'any' OR r.source = ?2)
         ORDER BY (r.eut IS NULL), r.eut, r.id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![item_id, source, limit as i64], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 一个输入格的"代表"信息
struct InputGroup {
    rep_item_id: Option<i64>,
    rep_name: Option<String>,
    candidate_count: i64,
    ore: Option<String>,
}

/// 取某配方的输入格；每组只返回一个代表物品 + 候选总数（避免 251 路展开）。
fn input_groups(conn: &Connection, recipe_id: i64) -> Result<Vec<InputGroup>> {
    let mut stmt = conn.prepare(
        "SELECT (SELECT COUNT(*) FROM group_candidates gc WHERE gc.group_id = g.id),
                (SELECT gc.item_id FROM group_candidates gc WHERE gc.group_id = g.id ORDER BY gc.item_id LIMIT 1),
                (SELECT COALESCE(i.display_name, i.unlocalized_name)
                   FROM group_candidates gc JOIN items i ON i.id = gc.item_id
                  WHERE gc.group_id = g.id ORDER BY gc.item_id LIMIT 1),
                g.ore
         FROM item_slots s
         JOIN ingredient_groups g ON g.id = s.group_id
         WHERE s.recipe_id = ?1 AND s.direction = 'input'
         ORDER BY s.slot_index",
    )?;
    let rows = stmt.query_map([recipe_id], |row| {
        Ok(InputGroup {
            candidate_count: row.get(0)?,
            rep_item_id: row.get(1)?,
            rep_name: row.get(2)?,
            ore: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

// ==================== 配方详情 / 列表 ====================

/// 一条配方里的一个格（输入或输出）
#[derive(Debug, Clone, PartialEq)]
pub struct DetailGroup {
    pub kind: String,
    pub ore: Option<String>,
    pub amount: Option<i64>,
    pub chance: Option<i64>,
    /// 候选物品的显示名（最多保留前若干个，避免刷屏）
    pub candidates: Vec<String>,
    pub candidate_total: i64,
}

/// 一条配方里的流体格
#[derive(Debug, Clone, PartialEq)]
pub struct DetailFluid {
    pub fluid: String,
    pub amount: i64,
    pub chance: Option<i64>,
}

/// 一条配方的完整详情
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeDetail {
    pub recipe_id: i64,
    pub category_id: String,
    pub category_name: Option<String>,
    pub sources: String,
    pub source: String,
    pub eut: Option<i64>,
    pub duration: Option<i64>,
    pub tier: Option<String>,
    pub amperage: Option<i64>,
    pub no_result: bool,
    pub fake: bool,
    pub inputs: Vec<DetailGroup>,
    pub results: Vec<DetailGroup>,
    pub fluid_inputs: Vec<DetailFluid>,
    pub fluid_outputs: Vec<DetailFluid>,
}

/// 取一条配方的完整详情（输入/输出/候选/流体/数值字段）。
pub fn recipe_detail(conn: &Connection, recipe_id: i64) -> Result<RecipeDetail> {
    let (
        category_id,
        category_name,
        sources,
        source,
        eut,
        duration,
        tier,
        amperage,
        no_result,
        fake,
    ) = conn
        .query_row(
            "SELECT r.category_id, c.name, c.sources, r.source, r.eut, r.duration, r.tier,
                    r.amperage, r.no_result, r.fake
             FROM recipes r JOIN categories c ON c.id = r.category_id
             WHERE r.id = ?1",
            [recipe_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, i64>(8)? != 0,
                    row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                ))
            },
        )
        .with_context(|| format!("找不到配方 id={recipe_id}"))?;

    // 物品格：一行一个候选，按 (方向, 格序号) 分组
    let mut stmt = conn.prepare(
        "SELECT s.direction, s.slot_index, g.kind, g.ore, g.amount, g.chance,
                COALESCE(i.display_name, i.unlocalized_name)
         FROM item_slots s
         JOIN ingredient_groups g ON g.id = s.group_id
         LEFT JOIN group_candidates gc ON gc.group_id = g.id
         LEFT JOIN items i ON i.id = gc.item_id
         WHERE s.recipe_id = ?1
         ORDER BY s.direction, s.slot_index, gc.item_id",
    )?;
    let rows = stmt.query_map([recipe_id], |row| {
        Ok((
            row.get::<_, String>(0)?,         // direction
            row.get::<_, i64>(1)?,            // slot_index
            row.get::<_, String>(2)?,         // kind
            row.get::<_, Option<String>>(3)?, // ore
            row.get::<_, Option<i64>>(4)?,    // amount
            row.get::<_, Option<i64>>(5)?,    // chance
            row.get::<_, Option<String>>(6)?, // candidate name
        ))
    })?;

    let mut inputs: Vec<DetailGroup> = Vec::new();
    let mut results: Vec<DetailGroup> = Vec::new();
    let mut current_key: Option<(String, i64)> = None;
    for row in rows {
        let (direction, slot_index, kind, ore, amount, chance, candidate) = row?;
        let key = (direction.clone(), slot_index);
        if current_key.as_ref() != Some(&key) {
            let group = DetailGroup {
                kind,
                ore,
                amount,
                chance,
                candidates: Vec::new(),
                candidate_total: 0,
            };
            match direction.as_str() {
                "result" => results.push(group),
                _ => inputs.push(group), // input / other 都算输入侧
            }
            current_key = Some(key);
        }
        let target = if direction == "result" {
            results.last_mut()
        } else {
            inputs.last_mut()
        };
        if let Some(group) = target {
            // 只统计真正的候选物品（LEFT JOIN 在"无候选"时会产出一行 NULL）
            if let Some(name) = candidate {
                group.candidate_total += 1;
                if group.candidates.len() < 6 {
                    group.candidates.push(name);
                }
            }
        }
    }

    let fluid_rows = |direction: &str| -> Result<Vec<DetailFluid>> {
        let mut stmt = conn.prepare(
            "SELECT f.name, s.amount, s.chance
             FROM fluid_slots s JOIN fluids f ON f.id = s.fluid_id
             WHERE s.recipe_id = ?1 AND s.direction = ?2
             ORDER BY s.slot_index",
        )?;
        let rows = stmt.query_map(params![recipe_id, direction], |row| {
            Ok(DetailFluid {
                fluid: row.get(0)?,
                amount: row.get(1)?,
                chance: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    };

    Ok(RecipeDetail {
        recipe_id,
        category_id,
        category_name,
        sources,
        source,
        eut,
        duration,
        tier,
        amperage,
        no_result,
        fake,
        inputs,
        results,
        fluid_inputs: fluid_rows("input")?,
        fluid_outputs: fluid_rows("result")?,
    })
}

/// 配方列表里的一行摘要
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeSummary {
    pub recipe_id: i64,
    pub category_id: String,
    pub eut: Option<i64>,
    pub duration: Option<i64>,
    pub tier: Option<String>,
    /// 主产出的简短描述（第一个输出格的代表物品）
    pub main_output: Option<String>,
}

/// 列出某台机器（类别 id 含该子串）的配方摘要。
pub fn list_recipes(
    conn: &Connection,
    category_needle: &str,
    source: &str,
    limit: usize,
) -> Result<Vec<RecipeSummary>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.category_id, r.eut, r.duration, r.tier,
                (SELECT COALESCE(i.display_name, i.unlocalized_name) || ' ×' || gc.count
                 FROM item_slots s
                 JOIN group_candidates gc ON gc.group_id = s.group_id
                 JOIN items i ON i.id = gc.item_id
                 WHERE s.recipe_id = r.id AND s.direction = 'result'
                 ORDER BY s.slot_index, gc.item_id LIMIT 1)
         FROM recipes r
         JOIN categories c ON c.id = r.category_id
         WHERE r.category_id LIKE '%' || ?1 || '%' AND (?2 = 'any' OR r.source = ?2)
         ORDER BY r.id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![category_needle, source, limit as i64], |row| {
        Ok(RecipeSummary {
            recipe_id: row.get(0)?,
            category_id: row.get(1)?,
            eut: row.get(2)?,
            duration: row.get(3)?,
            tier: row.get(4)?,
            main_output: row.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

// ==================== 打印包装 ====================

pub fn print_items(conn: &Connection, needle: &str, limit: usize) -> Result<()> {
    let items = find_items(conn, needle, limit)?;
    println!("匹配 \"{needle}\" 的物品（{}/{}）：", items.len(), limit);
    for item in &items {
        println!("  {}", item.label());
    }
    Ok(())
}

pub fn print_reverse(
    conn: &Connection,
    item: &ResolvedItem,
    source: &str,
    limit: usize,
) -> Result<()> {
    println!("反向：什么能做出 {}（来源={source}）", item.label());
    let rows = reverse_rows(conn, item.id, source, limit)?;
    if rows.is_empty() {
        println!("  （无）");
    }
    for row in rows {
        let mut parts = vec![format!("[{}] {}", row.source, row.category_id)];
        if let Some(name) = row.category_name.as_deref() {
            parts.push(name.to_string());
        }
        if let Some(eut) = row.eut {
            parts.push(format!("{eut} EU/t"));
        }
        if let Some(duration) = row.duration {
            parts.push(format!("{duration} tick"));
        }
        if let Some(tier) = row.tier.as_deref() {
            parts.push(tier.to_string());
        }
        parts.push(format!("x{}", row.count));
        if let Some(chance) = row.chance {
            if chance != 10000 {
                parts.push(format!("概率 {:.2}%", chance as f64 / 100.0));
            }
        }
        println!("  {}", parts.join(" | "));
    }
    Ok(())
}

pub fn print_forward(
    conn: &Connection,
    item: &ResolvedItem,
    source: &str,
    limit: usize,
) -> Result<()> {
    println!("正向：{} 被哪些类别消耗（来源={source}）", item.label());
    let rows = forward_rows(conn, item.id, source, limit)?;
    if rows.is_empty() {
        println!("  （无）");
    }
    for row in rows {
        let name = row.category_name.unwrap_or_default();
        println!(
            "  [{:<7}] {:>6} 次  {} {}",
            row.sources, row.uses, row.category_id, name
        );
    }
    Ok(())
}

pub fn print_chain(
    conn: &Connection,
    item: &ResolvedItem,
    source: &str,
    depth: i64,
    limit: usize,
) -> Result<()> {
    println!(
        "生产链：做出 {} 需要什么（深度 {depth}，来源={source}）",
        item.label()
    );
    let rows = chain_rows(conn, item.id, source, depth, limit)?;
    if rows.is_empty() {
        println!("  （无）");
    }
    let mut last_depth = -1;
    for node in rows {
        if node.depth != last_depth {
            println!("  ── 深度 {} ──", node.depth);
            last_depth = node.depth;
        }
        let via = node.via.as_deref().unwrap_or("?");
        let note = node
            .note
            .as_deref()
            .map(|n| format!("  ({n})"))
            .unwrap_or_default();
        println!("    {}  ← 由 {}{}", node.item, via, note);
    }
    Ok(())
}

/// 打印一条配方的完整详情（人类可读）。
pub fn print_recipe(conn: &Connection, recipe_id: i64) -> Result<()> {
    let detail = recipe_detail(conn, recipe_id)?;
    let name = detail
        .category_name
        .as_deref()
        .map(|n| format!(" / {n}"))
        .unwrap_or_default();
    println!(
        "配方 #{}  [{}] {}{}",
        detail.recipe_id, detail.source, detail.category_id, name
    );
    println!("  类别来源集合：{}", detail.sources);

    let mut numbers = Vec::new();
    if let Some(eut) = detail.eut {
        numbers.push(format!("{eut} EU/t"));
    }
    if let Some(duration) = detail.duration {
        numbers.push(format!("{duration} tick"));
    }
    if let Some(tier) = detail.tier.as_deref() {
        numbers.push(tier.to_string());
    }
    if let Some(amperage) = detail.amperage {
        numbers.push(format!("{amperage} A"));
    }
    if detail.fake {
        numbers.push("伪配方".to_string());
    }
    if detail.no_result {
        numbers.push("无产出".to_string());
    }
    if !numbers.is_empty() {
        println!("  {}", numbers.join(" | "));
    }

    print_group_section("输入", &detail.inputs);
    print_fluid_section("流体输入", &detail.fluid_inputs);
    print_group_section("输出", &detail.results);
    print_fluid_section("流体输出", &detail.fluid_outputs);
    Ok(())
}

fn print_group_section(title: &str, groups: &[DetailGroup]) {
    if groups.is_empty() {
        return;
    }
    println!("  {title}：");
    for group in groups {
        let mut desc = if let Some(ore) = group.ore.as_deref() {
            format!("矿辞 {ore}")
        } else if group.candidate_total > 1 {
            let mut list = group.candidates.join(" / ");
            let extra = group.candidate_total - group.candidates.len() as i64;
            if extra > 0 {
                list.push_str(&format!(" / …（共 {} 个候选）", group.candidate_total));
            }
            list
        } else {
            group
                .candidates
                .first()
                .cloned()
                .unwrap_or_else(|| "?".to_string())
        };
        if let Some(amount) = group.amount {
            if amount != 1 {
                desc.push_str(&format!(" ×{amount}"));
            }
        }
        if let Some(chance) = group.chance {
            if chance != 10000 {
                desc.push_str(&format!("（{:.2}% 概率）", chance as f64 / 100.0));
            }
        }
        println!("    - {desc}");
    }
}

fn print_fluid_section(title: &str, fluids: &[DetailFluid]) {
    if fluids.is_empty() {
        return;
    }
    println!("  {title}：");
    for fluid in fluids {
        let mut desc = format!("{} {} mB", fluid.fluid, fluid.amount);
        if let Some(chance) = fluid.chance {
            if chance != 10000 {
                desc.push_str(&format!("（{:.2}% 概率）", chance as f64 / 100.0));
            }
        }
        println!("    - {desc}");
    }
}

/// 列出某台机器（类别 id 含子串）的配方摘要。
pub fn print_recipes(conn: &Connection, needle: &str, source: &str, limit: usize) -> Result<()> {
    let rows = list_recipes(conn, needle, source, limit)?;
    println!(
        "类别含 \"{needle}\" 的配方（{}/{}，来源={source}）：",
        rows.len(),
        limit
    );
    if rows.is_empty() {
        println!("  （无）");
    }
    for row in rows {
        let mut parts = vec![format!("#{}", row.recipe_id)];
        if let Some(output) = row.main_output.as_deref() {
            parts.push(output.to_string());
        }
        if let Some(eut) = row.eut {
            parts.push(format!("{eut} EU/t"));
        }
        if let Some(duration) = row.duration {
            parts.push(format!("{duration}t"));
        }
        if let Some(tier) = row.tier.as_deref() {
            parts.push(tier.to_string());
        }
        parts.push(format!("[{}]", row.category_id));
        println!("  {}", parts.join("  |  "));
    }
    Ok(())
}

// ==================== CSV 子集导出 ====================
/// 把某个视图/表导出为 CSV（stdout 或文件）。
pub fn export_csv(conn: &Connection, view: &str, out: Option<&str>) -> Result<()> {
    // 只允许字母数字下划线，杜绝 SQL 注入（表名无法参数化）
    if !view.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("非法的视图/表名：{view}");
    }

    let mut stmt = conn.prepare(&format!("SELECT * FROM {view}"))?;
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    let mut writer: Box<dyn Write> = match out {
        Some(path) => Box::new(BufWriter::new(
            File::create(path).with_context(|| format!("创建文件失败：{path}"))?,
        )),
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    writeln!(
        writer,
        "{}",
        columns
            .iter()
            .map(|c| csv_field(c))
            .collect::<Vec<_>>()
            .join(",")
    )?;

    let mut rows = stmt.query([])?;
    let mut count = 0u64;
    while let Some(row) = rows.next()? {
        let mut fields = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            let value = match row.get_ref(i)? {
                ValueRef::Null => String::new(),
                ValueRef::Integer(v) => v.to_string(),
                ValueRef::Real(v) => v.to_string(),
                ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                ValueRef::Blob(bytes) => format!("<blob {} bytes>", bytes.len()),
            };
            fields.push(csv_field(&value));
        }
        writeln!(writer, "{}", fields.join(","))?;
        count += 1;
    }
    writer.flush()?;

    if let Some(path) = out {
        println!("已导出 {count} 行 → {path}");
    }
    Ok(())
}

/// CSV 字段转义：含逗号/引号/换行时加引号，内部引号翻倍（RFC 4180）。
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 搭一个内存库：铁矿石 --(gt.recipe.macerator)--> 铁粉 --(gt.recipe.furnace)--> 铁锭
    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../schema.sql")).unwrap();
        conn.execute_batch(include_str!("../views.sql")).unwrap();
        conn.execute_batch(
            "INSERT INTO categories(id, name, sources) VALUES
                 ('gt.recipe.macerator','研磨机','gt'),
                 ('gt.recipe.furnace','电炉','gt');
             INSERT INTO items(id, unlocalized_name, meta, nbt, display_name) VALUES
                 (1,'ore.iron',0,'','铁矿石'),
                 (2,'dust.iron',0,'','铁粉'),
                 (3,'ingot.iron',0,'','铁锭');
             INSERT INTO ingredient_groups(id, kind, amount, chance) VALUES (1,'item',1,NULL),(2,'item',1,NULL);
             INSERT INTO group_candidates(group_id, item_id, count) VALUES (1,1,1),(1,3,1),(2,2,1);
             INSERT INTO recipes(id, category_id, seq, source, duration, eut, tier) VALUES
                 (1,'gt.recipe.macerator',0,'gt',200,2,'ULV'),
                 (2,'gt.recipe.furnace',0,'gt',100,32,'LV');
             INSERT INTO item_slots(recipe_id, direction, slot_index, group_id) VALUES
                 (1,'input',0,1),(1,'result',0,2),
                 (2,'input',0,2),(2,'result',0,1);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn csv_field_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn resolve_item_by_name_and_id() {
        let conn = fixture();
        let by_name = resolve_item(&conn, "铁锭").unwrap();
        assert_eq!(by_name.id, 3);
        let by_id = resolve_item(&conn, "#1").unwrap();
        assert_eq!(by_id.unlocalized_name, "ore.iron");
        // 模糊匹配：'铁' 会命中第一个（铁矿石）
        assert!(resolve_item(&conn, "铁").is_ok());
        assert!(resolve_item(&conn, "不存在的物品").is_err());
    }

    #[test]
    fn reverse_and_forward() {
        let conn = fixture();
        // 铁粉由研磨机产出（输入是"铁矿石或铁锭"这个候选组）
        let reverse = reverse_rows(&conn, 2, "gt", 10).unwrap();
        assert_eq!(reverse.len(), 1);
        assert_eq!(reverse[0].category_id, "gt.recipe.macerator");
        assert_eq!(reverse[0].eut, Some(2));

        // 铁矿石被研磨机消耗
        let forward = forward_rows(&conn, 1, "gt", 10).unwrap();
        assert_eq!(forward.len(), 1);
        assert_eq!(forward[0].category_id, "gt.recipe.macerator");
        assert_eq!(forward[0].uses, 1);
    }

    #[test]
    fn chain_expands_inputs() {
        let conn = fixture();
        // 铁锭 <-(电炉)- 铁粉 <-(研磨机)- {铁矿石, 铁锭}；深度 2 应能看到铁粉与铁矿石
        let rows = chain_rows(&conn, 3, "gt", 2, 50).unwrap();
        let names: Vec<&str> = rows.iter().map(|node| node.item.as_str()).collect();
        assert!(names.contains(&"铁粉"), "链里应包含铁粉：{names:?}");
        assert!(names.contains(&"铁矿石"), "链里应包含铁矿石：{names:?}");
        // 研磨机的输入是"铁矿石或铁锭"这个 2 选 1 候选组 → 应带备注
        assert!(
            rows.iter().any(|node| node.note.is_some()),
            "多候选格应给出备注：{rows:?}"
        );
    }

    #[test]
    fn recipe_detail_groups() {
        let conn = fixture();
        let detail = recipe_detail(&conn, 1).unwrap();
        assert_eq!(detail.category_id, "gt.recipe.macerator");
        assert_eq!(detail.eut, Some(2));
        assert_eq!(detail.duration, Some(200));
        assert_eq!(detail.inputs.len(), 1, "输入只有一个候选组");
        assert_eq!(detail.inputs[0].candidate_total, 2, "该组有 2 个候选物品");
        assert_eq!(detail.results.len(), 1);
        assert_eq!(detail.results[0].candidates, vec!["铁粉".to_string()]);
    }

    #[test]
    fn list_recipes_summary() {
        let conn = fixture();
        let rows = list_recipes(&conn, "macerator", "gt", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].recipe_id, 1);
        assert_eq!(rows[0].eut, Some(2));
        assert_eq!(rows[0].main_output.as_deref(), Some("铁粉 ×1"));
    }
}
