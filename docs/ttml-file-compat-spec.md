# TTML 文件直链兼容接口规格（SPlayer-Next 适配）

## 目标

兼容 SPlayer-Next 设置项 "AMLL TTML DB 地址" 的 URL 模板 `{host}/%p/%s.ttml`
（`%p`=平台，`%s`=平台歌曲 ID）。用户把该地址换成自己部署的实例域名后，
SPlayer 可直接拉取 TTML（含私库上传的歌词）。

对标对象：官方 `https://amll-ttml-db.gbclstudio.cn/%p/%s.ttml`（公开直链、无鉴权）。

## 接口

- `GET /{platform}/{song_id}.ttml`，挂在**根路由**（与 `/`、`/admin`、`/v1/*`、`/api/v1/*` 共存；
  Axum 中静态路径段优先于捕获段，`/` 和 `/admin` 不受影响）。
- `platform` 映射（未知 platform → 400）：
  - `ncm` → 按 `ncm_music_ids` 查
  - `qq` → 按 `qq_music_ids` 查
  - `apple` → 按 `apple_music_ids` 查
  - `spotify` → 按 `spotify_ids` 查
- `song_id` 取路径文件名去掉 `.ttml` 后缀的部分；不以 `.ttml` 结尾的请求不归本路由处理
  （让 fallback 照常返回 404）。
- 查找：复用现有搜索逻辑的 ID 查询（`SearchQuery.id_query` 的 `ncm_music_ids` 等字段已存在），
  取第一条结果。私库优先：现有 ranking 已是 `(source_rank, score)` 私库在前，直接复用。
- 成功：200，body 为该歌词的 `raw_ttml` 全文，`Content-Type: application/xml; charset=utf-8`。
- 未找到：404。未知 platform：400。
- **不鉴权**（对标官方直链就是公开的）。

## 约束

- 不改现有 API 行为；`cargo test` 全过；`cargo clippy --all-targets` 零警告。
- 分支 `feat/ttml-file-compat`（已从 main 切好）。
- 给 platform 映射和后缀处理写单元测试（纯逻辑，不依赖 DB）。

## 验收

- `cargo test` 通过、`cargo clippy --all-targets` 零警告
- 单元测试覆盖：platform 映射（四种 + 未知→400）、`.ttml` 后缀 stripping
- 端到端真机验证由助理在 frk 部署后做（curl 真实平台 ID），不属于本次 Codex 验收范围
