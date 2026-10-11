# Web UI 规格：搜索页 + 管理员上传页

## 背景

`amll-ttml-api` 目前是纯 JSON API。需要加一个极简 Web UI：
- 公开搜索页 `/`：搜歌词、看结果、下载 TTML
- 管理员上传页 `/admin`：上传私库歌词（鉴权仍走现有 `POST /v1/admin/lyrics` 的 Bearer ADMIN_SECRET）

## 现有 API（不要改行为，只调用）

- `GET /api/v1/lyrics/search`：参数 `q`（全局关键词）、`musicName`、`artistName`、`albumName`、`lyricText`、`authorId`、`authorUsername`（至少一个非空，否则 400）；`source=private|public|all`（默认 all）；`pageSize`（默认 20）；`cursor`（翻页游标；**注意：cursor 必须配 source=private 或 public，配 all 会 400**）。返回 `{"status":200,"data":{"items":[...],"cursor":...}}`，item 含 `id`、`filename`、`source`、`musicNames`、`artistNames`、`albumNames`、`matchContext.snippet` 等。
- `GET /api/v1/lyrics/get?id=<id>`：取单首歌词全文（含 `raw_ttml`）。
- `POST /api/v1/admin/lyrics`：multipart，字段 `filename`（文本）+ `file`（文件）；Header `Authorization: Bearer <ADMIN_SECRET>`；成功 `{"status":200,"data":{"id":...,"filename":...,"source":"private"}}`；密钥错 401。

## 实现要求

1. **静态文件嵌入**：新增 `rust-embed` 依赖，把 `web/` 目录（`index.html`、`admin.html`、`app.js`、`style.css` 等）编译进二进制，单文件部署、运行时无外部文件依赖。Dockerfile 如需同步请一并改（`web/` 在构建上下文内即可）。
2. **路由**（挂在根 router，不在 `/v1` 或 `/api/v1` 下，放在 fallback 之前）：
   - `GET /` → `index.html`
   - `GET /admin` → `admin.html`
   - 静态资源按路径 serve（`rust-embed` + `mime_guess` 或等价方案）。
   - `/admin` 不从公开页链接（低调处理），**不做服务端鉴权**——真正的鉴权由上传 API 的 Bearer 保证。
3. **搜索页 `/`**：表单含 q / musicName / artistName / albumName / lyricText（至少填一项，前端校验）；source 下拉（all/public/private，默认 all）；结果卡片展示歌名、歌手、专辑、source 徽标（private 高亮）、匹配片段；点击展开看 TTML 全文（调 get 接口）+ "下载 .ttml" 按钮；pageSize=20，上一页/下一页用 cursor（翻页时保持用户选的 source，避免 cursor+all=400）。
4. **管理页 `/admin`**：顶部 ADMIN_SECRET 密码框（只存 sessionStorage，不进 localStorage）；上传表单：文件选择（.ttml）、filename 文本框（默认取所选文件名、可改）；提交 multipart 到 `/api/v1/admin/lyrics`；成功显示 id/filename/source，失败显示服务端错误信息（401 时提示密钥错误）；上传后提供"按文件名搜索确认"快捷按钮。
5. **前端**：纯 HTML/CSS/JS，不引入框架；fetch 调 `/api/v1/...`；页面中文；移动端可用。
6. **约束**：不改现有 API 行为；`cargo test` 全过；`cargo clippy --all-targets` 零警告；分支 `feat/webui`（已切好）。

## 验收

- `cargo test` 通过、`cargo clippy --all-targets` 零警告
- 本地起服务：`/` 可搜索翻页、source 过滤生效；`/admin` 不输密钥上传报 401，输对密钥上传成功且 source=private
