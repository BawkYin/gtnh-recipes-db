-- 常用视图（M3）
--
-- 目的：把"配方 ↔ 物品"的复杂 join 固化成命名视图，日常查询只写一行 SQL。
-- 每次打开数据库都会重建这些视图（先 DROP 再 CREATE），所以修改定义即时生效。

-- 配方 + 类别信息的扁平视图
DROP VIEW IF EXISTS v_recipes_full;
CREATE VIEW v_recipes_full AS
SELECT r.id            AS recipe_id,
       r.category_id,
       c.name          AS category_name,
       c.mod_id,
       c.sources,
       r.source,
       r.seq,
       r.no_result,
       r.duration,
       r.eut,
       r.tier,
       r.amperage,
       r.special,
       r.fake,
       r.hidden,
       r.disabled
FROM recipes r
JOIN categories c ON c.id = r.category_id;

-- 物品格全展开：一行 = 配方里的一个候选物品
-- 注意：一格多候选时会展开成多行（kind='anyOf'），所以统计"用了什么"要 DISTINCT
DROP VIEW IF EXISTS v_item_slots_full;
CREATE VIEW v_item_slots_full AS
SELECT s.recipe_id,
       r.source,
       r.category_id,
       c.name          AS category_name,
       s.direction,                       -- input | result | other
       s.slot_index,
       g.id            AS group_id,
       g.kind          AS group_kind,     -- item | oredict | anyOf
       g.ore,
       g.amount,
       g.chance,
       gc.item_id,
       i.unlocalized_name,
       i.meta,
       i.nbt,
       i.display_name,
       i.is_fluid,
       i.is_wildcard,
       gc.count        AS stack_count
FROM item_slots s
JOIN recipes r           ON r.id = s.recipe_id
JOIN categories c        ON c.id = r.category_id
JOIN ingredient_groups g ON g.id = s.group_id
JOIN group_candidates gc ON gc.group_id = g.id
JOIN items i             ON i.id = gc.item_id;

-- 流体格全展开（GT 数据）
DROP VIEW IF EXISTS v_fluid_slots_full;
CREATE VIEW v_fluid_slots_full AS
SELECT s.recipe_id,
       r.source,
       r.category_id,
       c.name   AS category_name,
       s.direction,
       s.slot_index,
       f.name   AS fluid,
       f.display_name,
       s.amount,
       s.chance
FROM fluid_slots s
JOIN recipes r    ON r.id = s.recipe_id
JOIN categories c ON c.id = r.category_id
JOIN fluids f     ON f.id = s.fluid_id;

-- 类别统计：NEI / GT 两种来源各多少条（同名类别会合并到一行）
DROP VIEW IF EXISTS v_category_stats;
CREATE VIEW v_category_stats AS
SELECT c.id,
       c.name,
       c.sources,
       c.amperage,
       c.recipe_count,
       COALESCE(SUM(CASE WHEN r.source = 'nei' THEN 1 ELSE 0 END), 0) AS nei_recipes,
       COALESCE(SUM(CASE WHEN r.source = 'gt'  THEN 1 ELSE 0 END), 0) AS gt_recipes
FROM categories c
LEFT JOIN recipes r ON r.category_id = c.id
GROUP BY c.id;

-- GT 机器配方（带数值字段），供生产线计算直接使用
DROP VIEW IF EXISTS v_gt_recipes;
CREATE VIEW v_gt_recipes AS
SELECT r.id          AS recipe_id,
       r.category_id,
       c.name        AS category_name,
       c.amperage    AS map_amperage,
       r.eut,
       r.duration,
       r.tier,
       r.special,
       r.fake,
       r.hidden,
       r.disabled
FROM recipes r
JOIN categories c ON c.id = r.category_id
WHERE r.source = 'gt';
