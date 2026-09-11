-- GTNH 配方数据库 schema v1
--
-- 设计目标：把"导出器产出的 JSON 文件"规范成可做图查询（物品 → 配方 → 物品）的关系模型。
--
-- 核心建模决策（对应第 09 课的讨论）：
-- 1) 配方与物品是"边"的关系，且**一格可能是多个候选物品**（矿辞展开的 anyOf，最多 251 个），
--    所以用 ingredient_groups（格子/候选组）+ group_candidates（组内候选）表达多对多。
-- 2) meta = 32767 是"任意变体"通配符 → items.is_wildcard 显式标记，避免查询时误当普通 meta。
-- 3) 流体在 NEI 数据里是伪物品（fluid.*），用 items.is_fluid 标记；GT 数据里有真正的流体表。
-- 4) NBT 是物品身份的一部分（蜜蜂基因组等）→ items.nbt 参与唯一键；
--    注意：SQLite 的 UNIQUE 视 NULL 互不相等，所以"无 NBT"统一存空串 ''。
-- 5) NEI 与 GT 是两套来源（有重叠），用 recipes.source 区分，GT 独有数值字段存 recipes 表。

PRAGMA foreign_keys = ON;

-- 构建元信息：schema 版本、导出时间、数据源路径等
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 配方类别（= NEI 的一个配方面板 / GT 的一个配方图）
--
-- 注意：NEI 与 GT 会用同一个 id 表示同一台机器（如 gt.recipe.macerator），
-- 因此这里以 id 为唯一键，用 sources 记录"哪些来源提供过这个类别"（如 'nei,gt'），
-- 元信息互补保留（name 来自 NEI、amperage 来自 GT）。
-- recipe_count 由导入结束后统一重算 = 该类别在库中的配方行数（跨来源合计）。
CREATE TABLE IF NOT EXISTS categories (
    id           TEXT PRIMARY KEY,          -- 如 gt.recipe.macerator / codechicken.nei.recipe.ShapedRecipeHandler
    name         TEXT,
    mod_id       TEXT,
    mod_name     TEXT,
    recipe_count INTEGER NOT NULL DEFAULT 0,
    file         TEXT,                      -- 相对 index.json 的配方文件路径
    collected_by TEXT,                      -- NULL=快速路径 / 'slow'=逐物品慢路径
    sources      TEXT NOT NULL,             -- 'nei' | 'gt' | 'nei,gt'
    amperage     INTEGER                    -- 仅 GT 配方图有意义
);

-- 物品（含 NEI 的流体伪物品）
CREATE TABLE IF NOT EXISTS items (
    id               INTEGER PRIMARY KEY,
    unlocalized_name TEXT NOT NULL,
    meta             INTEGER NOT NULL,
    nbt              TEXT NOT NULL DEFAULT '',   -- 空串 = 无 NBT（不能用 NULL，否则 UNIQUE 失效）
    display_name     TEXT,
    is_fluid         INTEGER NOT NULL DEFAULT 0, -- unlocalized_name 以 fluid. 开头
    is_wildcard      INTEGER NOT NULL DEFAULT 0, -- meta = 32767（任意变体）
    UNIQUE (unlocalized_name, meta, nbt)
);

-- 一个"输入/输出格"：可能是一个具体物品、一个矿辞名、或一组候选物品
CREATE TABLE IF NOT EXISTS ingredient_groups (
    id     INTEGER PRIMARY KEY,
    kind   TEXT NOT NULL,     -- 'item' | 'oredict' | 'anyOf'
    ore    TEXT,              -- kind='oredict' 时的矿辞名
    amount INTEGER,           -- NULL = 缺省 1
    chance INTEGER            -- 万分比；NULL = 缺省 10000（100%）
);

-- 候选组里的候选物品（多对多）
CREATE TABLE IF NOT EXISTS group_candidates (
    group_id INTEGER NOT NULL REFERENCES ingredient_groups(id),
    item_id  INTEGER NOT NULL REFERENCES items(id),
    count    INTEGER NOT NULL DEFAULT 1,   -- 候选物品自身的堆叠数
    PRIMARY KEY (group_id, item_id)
);

-- 配方本体；GT 独有数值字段（duration/eut/tier/...）仅 source='gt' 时有值
CREATE TABLE IF NOT EXISTS recipes (
    id          INTEGER PRIMARY KEY,
    category_id TEXT NOT NULL REFERENCES categories(id),
    seq         INTEGER NOT NULL,          -- 在类别文件里的序号（可回溯原始 JSON）
    source      TEXT NOT NULL,             -- 'nei' | 'gt'
    no_result   INTEGER NOT NULL DEFAULT 0,
    duration    INTEGER,
    eut         INTEGER,
    tier        TEXT,
    amperage    INTEGER,
    special     INTEGER,
    disabled    INTEGER,
    hidden      INTEGER,
    fake        INTEGER
);

-- 配方的物品格（引用候选组）
CREATE TABLE IF NOT EXISTS item_slots (
    recipe_id  INTEGER NOT NULL REFERENCES recipes(id),
    direction  TEXT NOT NULL,              -- 'input' | 'result' | 'other'
    slot_index INTEGER NOT NULL,           -- 0 起
    group_id   INTEGER NOT NULL REFERENCES ingredient_groups(id),
    PRIMARY KEY (recipe_id, direction, slot_index)
);

-- 真正的流体（GT 数据的 fluidInputs/fluidOutputs）
CREATE TABLE IF NOT EXISTS fluids (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL UNIQUE,     -- 如 hydrogen
    display_name TEXT
);

CREATE TABLE IF NOT EXISTS fluid_slots (
    recipe_id  INTEGER NOT NULL REFERENCES recipes(id),
    direction  TEXT NOT NULL,
    slot_index INTEGER NOT NULL,
    fluid_id   INTEGER NOT NULL REFERENCES fluids(id),
    amount     INTEGER NOT NULL,           -- 毫桶（mB）
    chance     INTEGER,                    -- 万分比；NULL = 100%
    PRIMARY KEY (recipe_id, direction, slot_index)
);

-- 索引：围绕"物品 → 配方 → 物品"的图查询设计
CREATE INDEX IF NOT EXISTS idx_items_lookup        ON items(unlocalized_name, meta);
CREATE INDEX IF NOT EXISTS idx_items_display       ON items(display_name);
CREATE INDEX IF NOT EXISTS idx_group_candidates_item  ON group_candidates(item_id);
CREATE INDEX IF NOT EXISTS idx_group_candidates_group ON group_candidates(group_id);
CREATE INDEX IF NOT EXISTS idx_item_slots_group    ON item_slots(group_id);
CREATE INDEX IF NOT EXISTS idx_item_slots_recipe   ON item_slots(recipe_id);
CREATE INDEX IF NOT EXISTS idx_recipes_category    ON recipes(category_id);
CREATE INDEX IF NOT EXISTS idx_recipes_source      ON recipes(source);
CREATE INDEX IF NOT EXISTS idx_fluid_slots_recipe  ON fluid_slots(recipe_id);
CREATE INDEX IF NOT EXISTS idx_fluid_slots_fluid   ON fluid_slots(fluid_id);
