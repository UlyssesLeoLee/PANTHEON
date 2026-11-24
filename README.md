# PANTHEON

**PANTHEON** — 面向数据库开发、结构理解与工程治理的现代化工作台。

版本：`0.1.0`（可运行 MVP 源码；非商业生产版） · 日期：2026-11-03

> 按 IPA 开发过程组织设计/测试文件，但不代表 IPA 官方认证或已通过测试。

## 已实现（可运行）

- Next.js 16 / React 19 前端：数据库总览、SQL 编辑器、结果网格、基础 ER 图、表定义书、CSV 及 Markdown 导出。
- Rust/Axum API：**SQLite** 实际数据库元数据读取、外键读取、单条只读查询、行数上限 1000、5 秒 SQLite progress 超时、Bearer Token、回环地址默认监听。
- **PITHOS 原生只读支持**：直接读取 PITHOS（PolyDB V1）的 KV / Document WAL 存储，把每个 `<存储>/<命名空间>` 映射为一张表，前端无需改动。见下文「PITHOS 原生支持」。
- Tauri 2 桌面外壳源文件（**未打包验证**）；SQL API **需要单独启动**，桌面安装包/sidecar 未集成。
- Rust UT + Playwright E2E 代码；CI 样例；可复制的 SQLite 示例数据库生成器。

## 尚未实现（仅设计）

PostgreSQL/MySQL/SQL Server/Oracle 驱动、PITHOS 关系型（实验性 Turso worker）数据、DDL 生成或执行、结构对比/迁移审批、AI Provider、GitGit 对接、多人协作、ER 拖拽与大规模自动布局、密钥保险库、严格生产审计日志、原生安装包。上述功能见 `docs/` 及 `docs/10_实现状态与差距分析.md`。禁止以 MVP 作为生产变更执行工具。

## 本地启动（Windows PowerShell）

运行条件：Node.js 22+、npm、Rust stable（需单独安装）、Python 3（仅创建 demo）。

**一键启动（推荐）**：脚本会在两个窗口分别启动 API 和前端，并打印本次随机 Token。数据库路径默认依次取 `E:\PITHOS\data`、`E:\PITHOS`、内置 demo；内存紧张的机器加 `-Lean`。

```powershell
.\scripts\start-dev.ps1
.\scripts\start-dev.ps1 -DbPath E:\PITHOS\data
.\scripts\start-dev.ps1 -DbPath .\data\pantheon-demo.db -Lean
```

浏览器打开 `http://localhost:3000`，「连接设置」里粘贴脚本打印的 Token 并连接。下面是手动启动的等价步骤。

```powershell
cd PANTHEON_Workspace
python scripts/create_demo.py
$env:PANTHEON_DB_PATH=(Resolve-Path .\data\pantheon-demo.db).Path
$env:PANTHEON_API_TOKEN="replace-this-with-a-random-long-token-32chars"
$env:PANTHEON_UI_ORIGIN="http://localhost:3000"
cd backend
cargo run
```

打开第二个 PowerShell 窗口：

```powershell
cd PANTHEON_Workspace\frontend
npm install
npm run dev
```

浏览 `http://localhost:3000`，点「连接设置」，API 默认 `http://127.0.0.1:8788`，输入与后端环境变量一致的 token，点击连接。API 不支持在 UI 中更改数据库文件：修改 `PANTHEON_DB_PATH` 后重启 API。

**重要：** Token 应由密码管理器或安全随机数生成工具提供，不要使用上述示例字符串处理真实数据。示例数据库包含虚构演示数据。

## PITHOS 原生支持

`PANTHEON_DB_PATH` 按路径类型自动选择后端：目录或 `*.wal` 文件（也包括以 `PDB1` 开头的文件）按 **PITHOS** 读取，其他文件按 **SQLite** 读取。

```powershell
$env:PANTHEON_DB_PATH="E:\PITHOS\data"     # PITHOS 数据目录（含 kv.wal / document.wal）
cd backend; cargo run                        # 启动横幅会显示 backend: pithos
```

- **只读且无副作用**：只以只读方式打开 WAL，不创建文件、不追加、不调用 PITHOS 引擎；每次请求重新读取，PITHOS 之后写入的数据无需重启 PANTHEON 即可看到。目录自身没有 `*.wal` 时会回退到其 `data` 子目录（`polydb` 命令行的默认位置）。
- **表映射**：每个存储文件（`kv.wal` → `kv`）的每个有活跃键的命名空间是一张表，名为 `<存储>/<命名空间>`，列为 `key`（TEXT，主键）和 `value`（BLOB；有效 UTF-8 显示为文本，否则显示 `[BLOB n bytes]`）。
- **查询语法**（只读 SQL 子集，表名必须用双引号）：

  ```sql
  SELECT * FROM "kv/app";
  SELECT value FROM "kv/app" WHERE key = 'user:1';
  SELECT key, value FROM "kv/app" WHERE key LIKE 'user:%' LIMIT 50;
  ```

  `LIKE` 只支持末尾单个 `%` 的字面前缀（`_` 按字面处理）；不支持 JOIN、ORDER BY、GROUP BY 和写入语句，不支持的语句会返回错误并附带语法说明。行数上限与 SQLite 相同（默认 200，最多 1000）。
- **失败即停**：WAL 末尾截断、CRC 不一致、魔数错误等一律报错（错误信息包含文件名），不会跳过或修复。PITHOS 正在写入时可能短暂读到截断尾部，重试即可。
- **边界**：不支持 PITHOS 关系型（实验性 Turso worker）数据；单个 WAL 文件上限 256 MiB。PANTHEON 通过黄金夹具（由真实 `polydb` 生成的 WAL）校验与 PITHOS 写入格式的兼容性。

## 运行测试

```powershell
cd backend
cargo test
# 另一个终端
cd frontend
npm install
npx playwright install chromium
npm run lint
npm run build
npm run test:e2e
```

Playwright 测试默认不需要 API，通过前端预览态测量 UI；Rust 单测在隔离的临时 SQLite 文件中运行。**当前交付环境未提供 cargo，故未实际执行 Rust 编译/UT；npm 网络安装超时，故未执行 Next 编译/E2E。所有测试状态保持未执行。**

## Tauri 桌面壳

`src-tauri/` 是外壳代码，构建时必须从仓库根目录运行 Tauri CLI，确保 `frontend/out` 存在。配置 `PANTHEON_UI_ORIGIN` 匹配部署后 WebView origin，另行启动并管理 Rust API。v0.1 尚未打包 sidecar，故推荐先使用 Web + 本地 API 的开发模式。

## 代码结构

```text
PANTHEON_Workspace/
  frontend/        Next.js UI、样式、E2E 配置
  backend/         Rust Axum API、rusqlite、安全策略和 UT
  src-tauri/       Tauri 2 桌面外壳（未验证）
  scripts/         演示数据创建脚本
  tests/e2e/      Playwright 系统 UI 自动测试
  docs/           IPA 对齐要件定义、基本/详细设计、UT/IT/ST、运维、安全、差异分析
  PANTHEON_IPA_测试管理.xlsx  需求/测试/追踪管理（保留前版数据）
```

## 安全注意

- 仅绑定 `127.0.0.1`；请勿将开发 API 暴露给公网。
- 服务端通过 SQLite 只读连接及 `PRAGMA query_only=ON` 执行查询，但此 MVP 不提供完整 SQL 沙箱；不要打开不可信 SQLite 数据库或运行不可信 SQL。
- CSV/Markdown 导出仅在浏览器本地完成；API 无导出接口。导入非受信任 CSV 到电子表格软件时应注意公式注入风险。
- 因前端无 Token 持久化，每次刷新页面需重新连接。生产环境需进一步加固日志、CSP、鉴权、数据脱敏和来源校验。

MIT 代码许可证适用于本包新编写的示例实现；第三方依赖遵循各自许可证。已有设计文档为用户项目资料。
