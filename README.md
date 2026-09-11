# gtnh-recipes-db

把 [ExportRecipe](https://github.com/BawkYin/ExportRecipe)（GTNH 配方导出器）产出的 JSON
数据，规范化导入 **SQLite**，提供可直接查询的配方数据库。

> 定位：**数据消费层**。JSON 导出文件是"唯一真相"，本工具是**可随时重建的派生物**；
> 不改动导出器与导出格式。

## 为什么需要它

导出数据是 337 + 165 个 JSON 文件（约 740MB），适合"按需读取"，但做**跨类别查询**很痛苦：

- "哪些机器/配方会用到铁锭？"
- "铁锭是怎么做出来的？"（反向）
- "从矿石到某台机器的完整生产链是什么？"（多层递归）

这些是**图查询**（物品 → 配方 → 物品）。规范化成关系模型后，SQLite 上毫秒级完成。

## 数据库设计要点

```
categories           配方类别（NEI 面板 / GT 配方图）
recipes              配方本体（NEI 与 GT 用 source 区分；GT 独有 duration/eut/tier/...）
items                物品（unlocalized_name + meta + nbt 唯一；标记 is_fluid / is_wildcard）
ingredient_groups    一个"输入/输出格"（kind: item | oredict | anyOf）
group_candidates     候选组里的候选物品（多对多）—— 矿辞一格可能是上百个候选
item_slots           配方 → 候选组（方向 input/result/other + 格序号）
fluids / fluid_slots 真正的流体（GT 数据；毫桶数量 + 概率）
meta                 schema 版本、数据源路径、导出时间
```

建模时处理的四个"身份语义"（否则查询会骗人）：

| 现象 | 处理 |
|---|---|
| 一格有多个候选物品（矿辞展开，最多 251 个） | `ingredient_groups` + `group_candidates` 多对多 |
| `meta = 32767` 是"任意变体"通配符 | `items.is_wildcard` 显式标记 |
| 流体在 NEI 数据里是伪物品 `fluid.*` | `items.is_fluid` 标记；GT 数据走真正的 `fluids` 表 |
| NBT 是物品身份的一部分（蜜蜂基因组等） | `items.nbt` 参与唯一键（无 NBT 存空串，避免 SQLite 的 NULL 唯一性坑） |
| 概率/数量缺省 | `chance` 缺省 10000（万分比）、`amount` 缺省 1 |

## 用法

```bash
# 默认：读 ./exportrecipe-output，写 ./gtnh-recipes.db
cargo run --release

# 指定路径
cargo run --release -- --input /path/to/exportrecipe-output --db gtnh-recipes.db

# 调试：只导某类，或每类只导 N 条
cargo run --release -- --category macerator
cargo run --release -- --limit 100

# 只要 NEI 数据集（跳过 GT）
cargo run --release -- --no-gt
```

导入结束会打印库内统计，并与 `index.json` / `gt/index.json` 声明的总数**对账**。

## 示例查询

```sql
-- 物品 id 速查（支持显示名模糊匹配）
SELECT id, unlocalized_name, meta, display_name FROM items WHERE display_name LIKE '%铁锭%';

-- 什么东西能做出它（反向：以某物品为输出的配方）
SELECT r.id, c.id AS category, r.duration, r.eut
FROM items i
JOIN group_candidates gc ON gc.item_id = i.id
JOIN item_slots s       ON s.group_id = gc.group_id AND s.direction = 'result'
JOIN recipes r          ON r.id = s.recipe_id
JOIN categories c       ON c.id = r.category_id
WHERE i.unlocalized_name = 'gt.metaitem.01.11001';

-- 它可以用在哪些配方里（正向：作为输入）
SELECT DISTINCT c.id AS category, COUNT(*) AS uses
FROM group_candidates gc
JOIN item_slots s ON s.group_id = gc.group_id AND s.direction = 'input'
JOIN recipes r    ON r.id = s.recipe_id
JOIN categories c ON c.id = r.category_id
WHERE gc.item_id = 12345
GROUP BY c.id ORDER BY uses DESC;

-- 只看必定产出的 GT 主产物（排除伪配方），且只要标准机器
SELECT c.id, r.eut, r.duration, r.tier
FROM recipes r JOIN categories c ON c.id = r.category_id
WHERE r.source = 'gt' AND r.fake IS NULL
  AND r.eut IS NOT NULL AND r.tier IN ('LV','MV','HV','EV','IV')
LIMIT 20;
```

## 状态与路线图

- [x] **M1**：项目骨架 + schema + NEI 数据集全量导入 + 总数对账
- [x] **M2**：GT 数值数据集导入（duration/eut/tier/流体/概率）
- [ ] **M3**：视图与常用查询（正向/反向/生产链递归 CTE）+ 导出 CSV 子集
- [ ] **M4**：NEI 与 GT 的映射视图（同一台机器的两套数据对照）

## 说明

- 数据库文件（`gtnh-recipes.db*`）**不进 git**，随时可用 `cargo run --release` 重建；
- 数据源不随本仓库分发（体积约 740MB），请用导出器生成：
  `/exportrecipes nei` → `/exportrecipes slow` → `/exportrecipes gt`。
