//! 与导出 JSON 一一对应的数据结构（serde 反序列化）。
//!
//! JSON 的键是 camelCase，Rust 字段用 snake_case，靠 `#[serde(rename_all = "camelCase")]` 映射。
//! 所有可选字段都用 `Option<T>` + `#[serde(default)]`，保持与导出器"缺省即省略"的约定一致。

use serde::Deserialize;

// ==================== NEI 通用数据集（schema v3） ====================

/// index.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexFile {
    pub schema: i64,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub category_count: i64,
    #[serde(default)]
    pub recipe_total: i64,
    #[serde(default)]
    pub duplicates_removed: i64,
    #[serde(default)]
    pub recipes_without_result: i64,
    #[serde(default)]
    pub categories_skipped_empty: i64,
    #[serde(default)]
    pub categories: Vec<Category>,
}

/// index.json 里的一个类别条目
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mod_id: Option<String>,
    #[serde(default)]
    pub mod_name: Option<String>,
    #[serde(default)]
    pub recipe_count: i64,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub collected_by: Option<String>,
}

/// recipes/<类别>.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryFile {
    #[serde(default)]
    pub handler: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mod_id: Option<String>,
    #[serde(default)]
    pub mod_name: Option<String>,
    #[serde(default)]
    pub recipes: Vec<Recipe>,
}

/// 一条 NEI 通用配方
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    /// 输入格（有序配方的网格里会有 null 空格）
    #[serde(default)]
    pub ingredients: Vec<Option<Ingredient>>,
    #[serde(default)]
    pub results: Vec<Option<Ingredient>>,
    /// NEI 的 otherStacks（容器/催化剂等副显示）；整个字段可能省略
    #[serde(default)]
    pub others: Option<Vec<Option<Ingredient>>>,
    /// true = 有输入无输出的伪配方（GT 燃料值/热量类）
    #[serde(default)]
    pub no_result: Option<bool>,
}

/// 一个输入/输出格
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingredient {
    /// "item" | "oredict" | "anyOf"
    pub kind: String,
    #[serde(default)]
    pub item: Option<Item>,
    #[serde(default)]
    pub ore: Option<String>,
    #[serde(default)]
    pub items: Option<Vec<Option<Item>>>,
    /// 数量（缺省 1）
    #[serde(default)]
    pub amount: Option<i64>,
    /// 概率，万分比（缺省 10000 = 100%）
    #[serde(default)]
    pub chance: Option<i64>,
}

/// 一个具体物品
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(default)]
    pub unlocalized_name: Option<String>,
    #[serde(default)]
    pub meta: Option<i64>,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub display_name: Option<String>,
    /// NBT 的 SNBT 文本；没有 NBT 时字段省略
    #[serde(default)]
    pub nbt: Option<String>,
}

// ==================== GT 数值数据集（schema v4） ====================

/// gt/index.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GtIndexFile {
    pub schema: i64,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub map_count: i64,
    #[serde(default)]
    pub recipe_total: i64,
    #[serde(default)]
    pub maps_skipped_empty: i64,
    #[serde(default)]
    pub maps: Vec<GtMap>,
}

/// gt/index.json 里的一个配方图
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GtMap {
    pub id: String,
    #[serde(default)]
    pub recipe_count: i64,
    #[serde(default)]
    pub amperage: Option<i64>,
    #[serde(default)]
    pub file: Option<String>,
}

/// gt/<配方图>.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // id/amperage 目前用 gt/index.json 里的值，这里保留字段便于以后校验
pub struct GtCategoryFile {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub amperage: Option<i64>,
    #[serde(default)]
    pub recipes: Vec<GtRecipe>,
}

/// 一条 GT 机器配方（带数值字段）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // owners 字段留到 M3（注册者追踪）再入库
pub struct GtRecipe {
    #[serde(default)]
    pub inputs: Vec<Option<GtItem>>,
    #[serde(default)]
    pub outputs: Vec<Option<GtItem>>,
    #[serde(default)]
    pub fluid_inputs: Vec<Option<GtFluid>>,
    #[serde(default)]
    pub fluid_outputs: Vec<Option<GtFluid>>,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub eut: Option<i64>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub amperage: Option<i64>,
    #[serde(default)]
    pub special: Option<i64>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub fake: Option<bool>,
    #[serde(default)]
    pub owners: Option<Vec<String>>,
}

/// GT 物品格（物品 + 它自己的概率）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GtItem {
    #[serde(default)]
    pub item: Option<Item>,
    #[serde(default)]
    pub chance: Option<i64>,
}

/// GT 流体格
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GtFluid {
    #[serde(default)]
    pub fluid: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub amount: Option<i64>,
    #[serde(default)]
    pub chance: Option<i64>,
}
