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
# 1) 导入（740MB JSON，约 23 秒；默认子命令就是 import）
cargo run --release -- import --input /path/to/exportrecipe-output --db gtnh-recipes.db

# 调试导入：只导某类 / 每类只导 N 条 / 跳过 GT
cargo run --release -- import --category macerator
cargo run --release -- import --import-limit 100 --no-gt

# 2) 查询（都可加 --source gt|nei 限定数据集）
cargo run --release -- item 铁锭                    # 查物品 id（支持 #123 直接按 id）
cargo run --release -- reverse 铁锭 --source gt     # 反向：什么能做出它（带 EU/t、时长、概率）
cargo run --release -- forward 铁锭                 # 正向：它被哪些机器消耗
cargo run --release -- chain 铁锭 --depth 3 --limit 60   # 生产链（--limit 为节点上限）

# 3) 导出与查看
cargo run --release -- views                          # 列出视图
cargo run --release -- csv v_gt_recipes --out gt_recipes.csv   # 18.6 万行约 0.2 秒
```

导入结束会打印库内统计，并与 `index.json` / `gt/index.json` 声明的总数**对账**。

## 内置视图

| 视图 | 内容 |
|---|---|
| `v_recipes_full` | 配方 + 类别名/模组/来源，扁平化 |
| `v_item_slots_full` | 物品格全展开（一行 = 一个候选物品，含 direction/chance/amount/通配标记） |
| `v_fluid_slots_full` | 流体格（GT 数据：毫桶数量 + 概率） |
| `v_category_stats` | 每个类别的 NEI / GT 各自条数 |
| `v_gt_recipes` | GT 机器配方（EU/t、时长、电压、伪配方标记） |

## 示例查询

```sql
-- 反向：什么东西能做出它（只统计"必定产出"，排除概率副产物）
SELECT c.id, r.eut, r.duration, r.tier
FROM v_item_slots_full v
JOIN recipes r    ON r.id = v.recipe_id
JOIN categories c ON c.id = r.category_id
WHERE v.item_id = 7136 AND v.direction = 'result' AND v.source = 'gt'
  AND (v.chance IS NULL OR v.chance = 10000)
ORDER BY r.eut;

-- 正向：哪些机器会消耗它
SELECT category_id, COUNT(DISTINCT recipe_id) AS uses
FROM v_item_slots_full
WHERE item_id = 7136 AND direction = 'input'
GROUP BY category_id ORDER BY uses DESC;

-- 用 GT 数值数据算"一条配方总耗电"（EU/t × 时长）
SELECT category_id, eut, duration, eut * duration AS total_eu
FROM v_gt_recipes
WHERE eut IS NOT NULL AND duration IS NOT NULL
ORDER BY total_eu DESC LIMIT 10;
```

> 生产链查询为什么不用递归 CTE：真实库有 268 万条候选边，且一格可能有 251 个候选，
> 纯 SQL 递归会指数爆炸（实测 60 秒跑不完）。`chain` 子命令改用**有界 BFS**
> （每物品最多 4 个产出配方、每组只取一个代表并标注"任意 N 选 1"、总节点上限），
> 实测 **0.04 秒**。

## 状态与路线图

- [x] **M1**：项目骨架 + schema + NEI 数据集全量导入 + 总数对账
- [x] **M2**：GT 数值数据集导入（duration/eut/tier/流体/概率）
- [x] **M3**：视图 + 查询子命令（正/反/生产链）+ CSV 子集导出 + 单元测试（CI 自动跑）
- [ ] **M4**：NEI ↔ GT 映射视图（同一台机器两套数据对照）

## 许可与数据说明

- **代码许可**：MIT，见 [LICENSE](LICENSE)。
- **本仓库只包含工具，不包含数据**：导出的 JSON（约 740MB）与生成的
  `gtnh-recipes.db`（约 436MB）都在 `.gitignore` 中，请自行用
  [ExportRecipe](https://github.com/BawkYin/ExportRecipe) 生成：
  `/exportrecipes nei` → `/exportrecipes slow` → `/exportrecipes gt`。
- **关于再分发数据**：导出的数据来自 GTNH 整合包内各 mod 的运行时注册表。
  如果你想**公开分发数据**（例如作为 Release 附件），请先确认相关 mod 的许可证与署名要求；
  代码可以自由公开，数据的分发是另一件事。

## 说明

- 数据库是**派生物**：任何 schema 变更后直接删掉 `.db` 重新导入即可（全量 23 秒）；
- 导入结束会与 `index.json` / `gt/index.json` 声明的配方总数**对账**，
  不一致会明确打印 `✘`，避免静默丢数据。
