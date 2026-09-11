# 使用教程（不需要看代码）

这份文档只讲**怎么用**。你需要的全部东西就是几条命令，或者一个能打开 SQLite 的工具。

---

## 0. 它能回答什么问题

| 你想知道 | 用哪条命令 |
|---|---|
| "铁锭是怎么做出来的？哪些机器能做？" | `reverse 铁锭` |
| "铁锭能用来干什么？" | `forward 铁锭` |
| "做一个铁锭，原料从哪来？"（多层展开） | `chain 铁锭 --depth 3` |
| "化学浸洗机都有哪些配方？" | `recipes chemicalbath` |
| "这条配方具体要什么？" | `recipe 311165` |
| "把某台机器的配方导成表格给 Excel" | `csv ...` 或一段 Python |

---

## 1. 一次性准备

### 1.1 先有数据

本工具**不含数据**。数据来自另一个项目 [ExportRecipe](../ExportRecipe)（游戏里的 mod）：

```
/exportrecipes nei      # 全部 mod 的配方（约 1~3 分钟）
/exportrecipes slow     # 补齐少数特殊面板（约 8 分钟）
/exportrecipes gt       # GT 机器配方数值（EU/t、时长、电压，约 1 分钟）
```

导出结果在 `.minecraft/exportrecipe-output/`（约 740MB）。

### 1.2 构建工具（一次即可）

```bash
cd gtnh-recipes-db
cargo build --release          # 首次约 1 分钟（会把 SQLite 一起编译进二进制）
```

> 受限环境提示：如果 cargo 无法写 `~/.cargo`，用
> `CARGO_HOME=../.cargo-home cargo build --release`（缓存落在项目内）。

### 1.3 导入数据库（每次数据更新后跑一次）

```bash
target/release/gtnh-recipes-db import \
  --input "/path/to/.minecraft/exportrecipe-output" \
  --db gtnh-recipes.db
```

输出结尾会有一行**对账**，确认没丢数据：

```
  NEI 配方：期望 289542 / 实际 289542 ✔
  GT  配方：期望 186380 / 实际 186380 ✔
```

约 23 秒，生成的 `gtnh-recipes.db` 约 436MB。

> 下文把 `target/release/gtnh-recipes-db` 简写成 `RECIPES`。
> 想更省事：`alias recipes="$PWD/target/release/gtnh-recipes-db"`。

---

## 2. 命令速查表

| 命令 | 作用 |
|---|---|
| `item <名字片段>` | 查物品（拿 `#id`），支持中文名模糊匹配 |
| `reverse <名字\|#id>` | 什么东西能**做出**它 |
| `forward <名字\|#id>` | 它被哪些机器/类别**消耗** |
| `chain <名字\|#id>` | 生产链：需要什么才能做出来（多层） |
| `recipe <配方id>` | 一条配方的完整详情（输入/输出/流体/数值） |
| `recipes <类别子串>` | 列出某台机器的配方摘要 |
| `csv <视图名> --out 文件.csv` | 把整张视图导成 CSV |
| `views` | 列出可导出的视图 |
| `import ...` | 导入/重建数据库 |

通用选项：

| 选项 | 说明 |
|---|---|
| `--db <文件>` | 数据库文件（默认 `gtnh-recipes.db`） |
| `--source any\|nei\|gt` | 只看某个来源的数据（`gt` = 有 EU/t 和时长的机器配方） |
| `--limit <N>` | 结果条数上限（`chain` 时表示最多展开多少个节点） |
| `--depth <N>` | `chain` 的层数（默认 3） |
| `--out <文件>` | `csv` 的输出文件（默认打印到屏幕） |

---

## 3. 五个典型场景（下面输出都是真实运行结果）

### 场景 1：铁锭怎么做

```bash
$ RECIPES item 铁锭 --limit 3
匹配 "铁锭" 的物品（3/3）：
  #7136 铁锭 （item.ingotIron@0）
  #4242 注血铁锭 （gt.metaitem.01.11977@11977）
  #4243 注血铁锭 （item.blood_infused_iron@0）

$ RECIPES reverse 铁锭 --source gt --limit 5
反向：什么能做出 #7136 铁锭 （item.ingotIron@0）（来源=gt）
  [gt] gt.recipe.primitiveblastfurnace | 砖高炉 | 0 EU/t | 2400 tick | x3
  [gt] gt.recipe.primitiveblastfurnace | 砖高炉 | 0 EU/t | 1600 tick | x3
  ...
```

> 每一行是**一条配方**。同一个产出出现多次很正常——它们的**输入不同**（用 `recipe <id>` 看详情）。

### 场景 2：某台机器有哪些配方

```bash
$ RECIPES recipes chemicalbath --source gt --limit 4
类别含 "chemicalbath" 的配方（4/4，来源=gt）：
  #311165  |  鞣制皮革 ×1  |  30 EU/t  |  300t  |  LV  |  [gt.recipe.chemicalbath]
  #311167  |  活塞 ×1  |  30 EU/t  |  30t  |  LV  |  [gt.recipe.chemicalbath]
  #311168  |  覆膜电路基板 ×1  |  8 EU/t  |  100t  |  ULV  |  [gt.recipe.chemicalbath]
```

### 场景 3：看一条配方的完整内容

```bash
$ RECIPES recipe 311165
配方 #311165  [gt] gt.recipe.chemicalbath / 化学浸洗机
  类别来源集合：nei,gt
  30 EU/t | 300 tick | LV | 1 A
  输入：
    - 皮革
  流体输入：
    - phosphoricacid_gt5u 250 mB
  输出：
    - 鞣制皮革
```

（`类别来源集合：nei,gt` 表示同一台机器在"全部 mod 配方"和"GT 数值配方"两套数据里都有。）

### 场景 4：这个物品能用来干什么

```bash
$ RECIPES forward 铁锭 --limit 5
正向：#7136 铁锭 （item.ingotIron@0） 被哪些类别消耗（来源=any）
  [nei    ]    238 次  codechicken.nei.recipe.ShapedRecipeHandler 有序合成
  [nei,gt ]     64 次  gt.recipe.extruder 压模机
  [nei    ]     51 次  codechicken.nei.recipe.RepairRecipeHandler 修复
  [nei,gt ]     38 次  gt.recipe.assembler 组装机
  [nei    ]     27 次  gt.recipe.category.tic_part_extruding 匠魂部件压模
```

### 场景 5：生产链（需要什么才能做出来）

```bash
$ RECIPES chain 铁锭 --source gt --depth 2 --limit 12
生产链：做出 #7136 铁锭 （item.ingotIron@0） 需要什么（深度 2，来源=gt）
  ── 深度 1 ──
    焙烧铁粉  ← 由 gt.recipe.primitiveblastfurnace
    煤炭  ← 由 gt.recipe.primitiveblastfurnace
    ...
  ── 深度 2 ──
    含杂焙烧铁粉  ← 由 gt.recipe.cauldron
    焙烧铁矿石  ← 由 gt.recipe.macerator
    ...
```

> 有些行会带备注 `(任意 12 选 1 的代表)`：说明那一格在原配方里是"多种材料都行"，
> 这里只展开其中一个代表，避免结果爆炸。

---

## 4. 导出数据

### 4.1 整张视图导成 CSV

```bash
$ RECIPES views
  v_category_stats
  v_fluid_slots_full
  v_gt_recipes
  v_item_slots_full
  v_recipes_full

$ RECIPES csv v_gt_recipes --out gt_recipes.csv
已导出 186380 行 → gt_recipes.csv        # 约 0.2 秒，10.7MB
```

可用的视图：

| 视图 | 一行代表什么 |
|---|---|
| `v_gt_recipes` | 一条 GT 机器配方（含 EU/t、时长、电压、伪配方标记） |
| `v_recipes_full` | 一条配方 + 类别/模组/来源 |
| `v_item_slots_full` | 配方里的**一个候选物品**（含方向、数量、概率、通配标记） |
| `v_fluid_slots_full` | 配方里的一个流体格（毫桶 + 概率） |
| `v_category_stats` | 一个类别（NEI 与 GT 各多少条） |

### 4.2 只要某台机器的配方（Python 示例）

```python
import csv, sqlite3
conn = sqlite3.connect("gtnh-recipes.db")     # 注意文件名是 gtnh-recipes.db
sql = """
SELECT r.id, r.eut, r.duration, r.tier,
       (SELECT group_concat(COALESCE(i.display_name, i.unlocalized_name) || ' x' || gc.count, ' | ')
        FROM item_slots s JOIN group_candidates gc ON gc.group_id = s.group_id
        JOIN items i ON i.id = gc.item_id
        WHERE s.recipe_id = r.id AND s.direction = 'input')  AS inputs,
       (SELECT group_concat(COALESCE(i.display_name, i.unlocalized_name) || ' x' || gc.count, ' | ')
        FROM item_slots s JOIN group_candidates gc ON gc.group_id = s.group_id
        JOIN items i ON i.id = gc.item_id
        WHERE s.recipe_id = r.id AND s.direction = 'result') AS outputs
FROM recipes r
WHERE r.category_id = 'gt.recipe.macerator' AND r.source = 'gt'
ORDER BY r.id
"""
rows = list(conn.execute(sql))
with open("macerator.csv", "w", newline="", encoding="utf-8") as f:
    w = csv.writer(f)
    w.writerow(["recipe_id", "eut", "duration", "tier", "inputs", "outputs"])
    w.writerows(rows)
```

运行结果示例（研磨机前 5 条）：

```
#351239 [ULV] 2 EU/t 400t : 奶酪片 x1 -> 小堆奶酪粉 x1
#351240 [ULV] 2 EU/t 400t : 可可豆 x1 -> 可可粉 x1
#351241 [ULV] 2 EU/t 400t : 南瓜 x1 -> 南瓜种子 x4
```

### 4.3 不想敲命令？用图形工具

装一个 **DB Browser for SQLite**（免费），打开 `gtnh-recipes.db` → 选视图 → 点"导出为 CSV"。
所有视图和表都能直接浏览、筛选、排序，不需要写代码。

---

## 5. 数据怎么读（字段语义）

| 概念 | 说明 |
|---|---|
| `chance` | **万分比**：10000 = 100%。**字段缺省 = 100%**（省体积）。100 = 1% |
| `amount` | 数量。**缺省 = 1** |
| `meta` | 子类型/损坏值；**32767 = 任意变体**（`items.is_wildcard=1`），别当普通数字用 |
| 流体（NEI 数据） | 表现为伪物品 `fluid.xxx`（`items.is_fluid=1`），`amount` 是毫桶 |
| 流体（GT 数据） | 存在 `fluids` / `fluid_slots` 表，`amount` 是毫桶 |
| `nbt` | 物品的 NBT（蜜蜂基因组、附魔等）。**同一物品名 + meta，NBT 不同就是不同物品** |
| `sources` | 该类别有哪些来源：`nei`（全部 mod 配方）/ `gt`（GT 数值配方）/ `nei,gt`（两者都有） |
| `no_result` | GT 的"燃料值/热量"类伪配方：有输入没输出 |
| `fake` / `hidden` | GT 伪配方 / 在 NEI 里隐藏的配方 |
| `eut` / `duration` / `tier` | EU/t、单次时长（tick，20 = 1 秒）、能跑该配方的最低电压等级；**只有 GT 数据有** |
| `amperage` | 配方图安培数，实际功率 = `eut × amperage` |

---

## 6. 常见问题

**Q：为什么同一个物品名出现好几次？**
不同 `meta`、不同 NBT（如"注血铁锭"有 gt 版和普通版），或者 NEI/GT 两套数据。用 `item` 命令看 `#id` 区分。

**Q：`reverse` 结果里有重复行？**
那是**不同配方**（输入不同、产出相同）。用 `recipe <id>` 看每条的输入就明白了。

**Q：数字可信吗？**
数据是游戏运行时真实注册表导出的；导入时会和源文件声明的总数对账（✔）。你可以在 `meta` 表里看到来源路径与导出时间。

**Q：生产链为什么只给"代表物品"？**
真实数据里一个输入格最多有 **251 个候选**（矿辞）。全部展开会指数爆炸（实测 60 秒都跑不完）；工具改用有界 BFS，每组取一个代表并标注 `(任意 N 选 1 的代表)`。

**Q：报错 `no such table: recipes`？**
数据库路径写错了（SQLite 会默默新建一个空文件）。确认 `--db` 指向 `gtnh-recipes.db`，或先跑一次 `import`。

**Q：数据更新了怎么办？**
删掉 `gtnh-recipes.db` 重新 `import`（23 秒）。数据库是**派生物**，随便重建。

**Q：想看某个物品在某个机器里的具体配方？**
先 `item 物品` 拿到 `#id`，再：

```bash
RECIPES recipes <机器类别子串> --limit 100 | grep <物品名>     # 粗筛
```

或直接用 SQL（`v_item_slots_full` 视图），例如：

```sql
SELECT DISTINCT recipe_id FROM v_item_slots_full
WHERE item_id = 7136 AND direction = 'input' AND category_id LIKE '%assembler%';
```

---

## 7. 术语表

| 词 | 含义 |
|---|---|
| **NEI** | NotEnoughItems，游戏里查配方的界面；它的"配方面板"是导出数据的中央索引 |
| **GT / GT5U** | GregTech 5 Unofficial，整合包的核心机器 mod |
| **RecipeMap / 配方图** | GT 的配方表，一台机器对应一张（如 `gt.recipe.macerator`） |
| **EU/t** | GT 的电力单位（每 tick 消耗的 EU） |
| **tick** | 游戏时间单位，20 tick = 1 秒 |
| **tier** | 电压等级（ULV/LV/MV/HV/EV/IV/LuV/ZPV/UV…）；本工具给的是"能跑该配方的最低等级" |
| **矿辞（OreDictionary）** | "任意一种铁锭"这类别名机制；对应工具里的"候选组" |
| **伪配方** | 没有真实产出的登记项（燃料值、热量等），已用 `no_result`/`fake` 标记 |

---

## 8. 想更深入

- 数据是怎么导出的：另一个仓库 [ExportRecipe](../ExportRecipe) 的 `docs/`
  （尤其第 06~09 课：NEI 采集器、GT 数值、慢路径、数据库设计）
- 数据库表结构与设计取舍：本仓库 `schema.sql`（有中文注释）与 `README.md`
- 想看 SQL 视图定义：`views.sql`
