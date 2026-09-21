# 帳號切分（Per-Account Isolation）交接文件

- 日期：2026-09-18
- 分支：`AccountManagement`
- 相關 commit：`ae2029caf`（設定依帳號隔離）、`66b03a32d`（AI 對話／SQL 歷史／分頁保留）
- 前置文件：[多帳號登入與資料隔離功能規劃](./multi-account-plan.md)、[MCP 中央訪問策略交接說明](./pips/plans/2026-07-18-mcp-access-policy-handoff.md)
- 適用範圍：桌面版（Tauri）、DBX Web（多帳號）、原生 MCP Server、CLI／Desktop bridge

本文件記錄「哪一筆資料屬於哪個帳號、程式碼在哪裡決定、新增設定時該怎麼做」。接手前請先讀第 1、2、6 節。

---

## 1. 核心模型

### 1.1 account id

所有個人資料以 `user_id TEXT` 表示，唯一例外是桌面版使用**空字串**：

```rust
// crates/dbx-core/src/storage.rs
pub const DESKTOP_ACCOUNT_ID: &str = "";
```

沒有帳號概念的執行環境（桌面 App、本機 MCP Server、Desktop bridge）一律傳 `DESKTOP_ACCOUNT_ID`；Web 相關路徑一律使用登入 session 的 `user_id`。

### 1.2 各執行環境如何決定 account id

| 執行環境 | account id 來源 | 決定位置 |
| --- | --- | --- |
| 桌面 App（Tauri command） | `DESKTOP_ACCOUNT_ID` | `src-tauri/src/commands/*` |
| DBX Web UI | 登入 session 的 `user_id` | `crates/dbx-web/src/routes/*`（`axum::extract::Extension<UserSession>`） |
| 本機 MCP Server（`LocalBackend`） | `DESKTOP_ACCOUNT_ID` | `crates/dbx-mcp/src/backend.rs` |
| Web MCP Server（`WebBackend`） | 以 `DBX_WEB_PASSWORD` 登入的 session | 同上（呼叫 `/api/app-settings/mcp-policy`、`/api/connection/list`） |
| Desktop MCP bridge | `DESKTOP_ACCOUNT_ID` | `src-tauri/src/commands/mcp_bridge.rs` |

Web 的 MCP 末端防護（`crates/dbx-web/src/routes/mcp_policy.rs`）**不是**用呼叫者身分解析策略，而是用「連線的所屬帳號」：

```rust
async fn load_policy_for_connection(state, connection_id) -> McpUserPolicy {
    let owner = storage.connection_user_id(connection_id)?;   // 找不到 → CONNECTION_NOT_FOUND
    storage.load_mcp_user_policy(&owner)
}
```

理由：連線本身的權限由擁有者決定，避免同一個請求同時牽涉兩個帳號時出現不一致。

---

## 2. 儲存層

### 2.1 兩種存放方式

| 方式 | 適用 | 用法 |
| --- | --- | --- |
| 資料表的 `user_id` 欄位 | 清單型資料（連線、歷史、片段、隧道…） | 每個 storage 方法加 `user_id: &str`，SQL 一律 `WHERE user_id = ?` |
| `user_settings (user_id, key, value)` | 單值型個人設定 | `get_user_setting` / `save_user_setting` / `delete_user_setting` |

`ensure_user_id_columns_sync`（`init_schema`）會為下列表補上 `user_id TEXT NOT NULL DEFAULT ''`：`connections`、`connection_secrets`、`history`、`ai_conversations`、`mq_token_records`、`saved_sql_folders`、`saved_sql_files`、`prompt_templates`、`tunnel_profiles`、`ai_configs`、`ai_config`、`ai_provider_configs`。

### 2.2 完整歸屬表

| 資料／設定 | 存放 | 歸屬 | 備註 |
| --- | --- | --- | --- |
| 連線＋密文 | `connections` / `connection_secrets` | 個人 | `save_connections(configs, user_id)` 是**整份覆寫**：先 `DELETE ... WHERE user_id` 再寫入，所以一次呼叫要帶完整清單 |
| SQL 歷史 | `history` | 個人 | 每帳號各自保留 `MAX_HISTORY = 1000` 筆 |
| AI 對話 | `ai_conversations` | 個人 | 每帳號保留最近 50 則；重登自動接續同連線最近一則 |
| AI 設定（多組） | `ai_configs` | 個人 | |
| 舊版 AI 設定 | `ai_config` / `ai_provider_configs` | 個人 | 只剩一次性遷移會讀 |
| AI 模式／選用模型 | `user_settings.ai_chat_selection` | 個人 | 舊鍵 `app_state.ai_chat_selection_v1` 由遷移接手 |
| AI 全域指令 | `user_settings.ai_global_custom_instructions` | 個人 | |
| MCP 策略（allowlist＋執行權限） | `user_settings.mcp_policy` | 個人 | 舊鍵 `app_settings.mcp_global_policy` 由遷移接手 |
| 程式碼片段 | `prompt_templates` | 個人 | 同名檢查只在同帳號內；表層 `UNIQUE(name)` 已移除 |
| 隧道設定檔 | `tunnel_profiles` | 個人 | 清單個人化；連線解析改以 profile id 跨帳號查找（見第 7 節） |
| 收藏 SQL | `saved_sql_folders` / `saved_sql_files` | 個人 | |
| 側邊欄版面／釘選節點 | `user_settings` | 個人 | `*_for_user` 系列 |
| MQ token 紀錄 | `mq_token_records` | **實例共用（待決）** | 有 `user_id` 欄但 `mq/service.rs` 讀寫未過濾 |
| UI／編輯器偏好 | Web：瀏覽器 `editor_settings`；桌面：`app_state.editor_settings` | 個人 | Web 見第 5 節 |
| 桌面／應用偏好（tray、duckdb worker、debug log…） | Web：`dbx-desktop-settings:u_{userId}`；桌面：`app_settings` | 個人 | |
| 開啟分頁 | Web：瀏覽器 `open_tabs`；桌面：`app_state` | 個人 | 關閉分頁前會 flush |
| AI 回合上限／重試次數 | `app_settings` | 實例層級，**寫入限管理員** | 讀取開放給所有帳號 |
| 雲端同步（WebDAV 帳密、同步密文口令、片段同步 token） | `app_settings` / `app_state` | 實例層級，**Web 僅管理員** | Sync 頁籤在 Web 不顯示 |
| 使用者管理／LDAP／稽核／備份還原／登入密碼 | — | 管理員 | |

`USER_DATA_TABLES`（`storage.rs`）另用於「這個 DB 是否含使用者資料」與備份匯入判斷，目前只列清單型表，不含 `prompt_templates`、`tunnel_profiles`。

### 2.3 命名與簽名慣例

```rust
pub async fn load_prompt_templates(&self, user_id: &str) -> Result<Vec<PromptTemplate>, String>
pub async fn save_prompt_template(&self, id: &str, name: &str, content: &str, user_id: &str) -> ...
pub async fn load_ai_chat_selection(&self, user_id: &str) -> Result<Option<AiChatSelectionState>, String>
pub async fn load_all_connections(&self) -> ...            // 僅供跨帳號的系統作業，勿用在一般請求
pub async fn load_all_tunnel_profiles(&self) -> ...        // 僅供 profile id 解析
pub async fn connection_user_id(&self, connection_id: &str) -> Result<Option<String>, String>
```

`with_conn` 的閉包需要 `'static`，所以方法開頭通常要 `let user_id = user_id.to_string();`。

---

## 3. 管理員限定

| 位置 | 作法 |
| --- | --- |
| `crates/dbx-web/src/routes/admin.rs`、`auth.rs` | handler 內 `if !session.is_admin { return Err(StatusCode::FORBIDDEN) }` |
| `routes/app_settings.rs`（AI 上限）、`routes/cloud_sync.rs` | 共用 `fn require_admin(session) -> Result<(), AppError>` → `AppError::forbidden(...)`（403） |
| 路由對照 | `/api/users/*`、`/api/ldap/*`、`/admin/audit-logs`、`/admin/sql-history`、`/backup/*`、`PUT /api/app-settings/max-agent-turns`、`PUT /api/app-settings/max-retries`、全部 `/api/cloud-sync/*` |
| 前端閘門 | `EditorSettingsDialog.vue`：`canManageInstanceSettings = computed(() => !isWeb || userStore.isAdmin)`；`settingsCategoryNav` 對非管理員隱藏 `admin` / `ldap` / `audit` / `backup-restore`，Web 模式另外隱藏 `about`（內含「重設所有預設值」） |

原則：**後端一定要擋**，前端隱藏只是避免誤按；新管理員功能兩邊都要加。

---

## 4. 啟動時的一次性遷移

`Storage::init_schema`（`storage.rs`）依序執行，全部必須可重入（idempotent）：

1. `SCHEMA_STATEMENTS` 建表，接著 `ensure_history_columns_sync`、`ensure_saved_sql_columns_sync`、`ensure_tab_runtime_cache_columns_sync`、`ensure_ai_configs_columns_sync`、`ensure_state_store_columns_sync`、`ensure_user_id_columns_sync` 補欄位
2. `migrate_legacy_password_to_admin_user` — 舊單一密碼 → 建立首個 admin，並把既有 user-data 列歸戶
3. `migrate_legacy_mcp_policy_to_user_policy` — `app_settings.settings_json.mcp_global_policy` → 歸屬帳號的 `user_settings.mcp_policy`，成功後刪除舊鍵；**無法解析的舊值轉為唯讀**（fail-closed）
4. `migrate_prompt_template_name_uniqueness` — DDL 仍含 `UNIQUE` 時重建 `prompt_templates`（移除全表唯一、改由應用層做同帳號同名檢查）
5. `migrate_legacy_user_scoped_rows` — 把 `ai_config`、`ai_provider_configs`、`prompt_templates`、`tunnel_profiles` 中 `user_id = ''` 的殘留列歸給歸屬帳號
6. `migrate_legacy_ai_state_to_user_settings` — `app_state.ai_chat_selection_v1`、`app_state.ai_global_custom_instructions` → `user_settings`，並刪除舊列

### 歸屬帳號（`legacy_scope_owner`）

```text
若 connections 仍有 user_id = '' 的列 → 空字串（桌面資料）
否則最早的管理員
否則最早的帳號
都沒有 → 空字串
```

### 只有「來源唯一」時才會歸戶（`legacy_handoff_is_unambiguous`）

`users` 數量 ≤ 1 時，全實例的值必然是該帳號（或帳號制之前）寫的，可以安全歸戶；
**≥ 2 個帳號時，寫入者已不可考，一律不歸給任何人**：

| 情況 | 行為 |
| --- | --- |
| `users <= 1`（桌面版、或剛從單機升級） | 把全實例值交給 `legacy_scope_owner`，保留使用者原本的設定 |
| `users >= 2`（多帳號已在用） | 不歸戶、不刪除：資料留在原處（`user_id = ''` 的列／`app_state`／`app_settings`），由於正式讀取一律帶真正的帳號，這些殘留對所有人都是不可見的惰性資料，並在 log 留下 warning |

理由：多帳號環境下把某個人的設定交給另一個人（例如最早的管理員），會讓對方看到不屬於自己的內容，看起來就像是沒有切乾淨。寧可讓大家回到預設值，也不要錯誤歸戶。

語意重點：**舊資料只交給一個帳號，而且只在來源唯一時才交**。其他帳號一律從預設值開始，避免繼承到指向他人連線的 allowlist、他人的範本或 AI 金鑰。

---

## 5. 前端：瀏覽器本機狀態

Web 模式下部分偏好在瀏覽器而非伺服器：

| 項目 | key | 位置 |
| --- | --- | --- |
| 編輯器／UI 偏好、開啟分頁、收藏 SQL 游標位置 | `u_{userId}:editor_settings`、`u_{userId}:open_tabs`、`u_{userId}:saved_sql_editor_positions` | IndexedDB store `dbx-app-state`，fallback localStorage `dbx-app-state:` |
| 桌面／應用偏好 | `dbx-desktop-settings:u_{userId}` | localStorage |

- 前綴由 `browserAppStateStorage.setCurrentUserId(id)` 決定（`currentUserScope()`），`http.ts` 的 `desktopSettingsStorageKey()` 用 `getCurrentUserId()`。
- 呼叫時機：`App.vue` 的 `onMounted`（Web：`/api/auth/check` 取得 `data.user` 之後、`initApp()` 之前）與 `onLoginSuccess()`；換帳號時 `clearAllBrowserAppState()`。
- 因此：**同一瀏覽器不同帳號互不干擾，但換裝置／換瀏覽器不會帶走偏好與分頁**。要跨裝置需改存伺服器端 per-user。
- 開啟分頁在關閉前會 flush：`App.vue` 的 `flushPendingTabPersist()` 掛在 `pagehide` 與 `visibilitychange`（僅 Web）。

---

## 6. 新增設定時的檢查清單

1. **先分類**：個人偏好／資料，還是實例層級（會影響所有人）？
2. 個人：
   - 清單型 → storage 方法加 `user_id: &str`，SQL 加 `WHERE user_id = ?`，寫入時注意「是覆寫還是 upsert」。
   - 單值型 → 放 `user_settings`，用 `get_user_setting` / `save_user_setting`。
   - 呼叫端：Tauri 傳 `DESKTOP_ACCOUNT_ID`；Web route 加 `Extension<UserSession>` 並用 `session.user_id`。
3. 實例層級：寫入端加 `require_admin`（照 `app_settings.rs` / `cloud_sync.rs` 的樣式），前端用 `canManageInstanceSettings` 隱藏或停用。
4. **舊資料**：若原本是全域欄位，必須在 `init_schema` 加一次性遷移，並考慮 fail-closed（無法解析時往「更嚴格」的方向）。
5. **測試**：
   - storage：兩個帳號互相看不到、互不覆寫（參考 `prompt_templates_are_scoped_per_account`、`tunnel_profiles_are_scoped_per_account`、`ai_selection_instructions_and_legacy_configs_are_scoped_per_account`、`history_eviction_is_scoped_to_the_owning_account`）。
   - 舊資料歸戶：`legacy_instance_rows_are_handed_to_the_owning_account`。
   - Web route：`crates/dbx-web` 內已有 `fn session(is_admin) -> UserSession` 輔助（`app_settings.rs`、`cloud_sync.rs`、`connection.rs`）。
   - 前端：`oxfmt` 會檢查 `.vue` 樣板語法（多一個 `</div>` 就會讓 commit hook 失敗）。
6. 文件：更新本文件第 2.2 節的歸屬表。

---

## 7. 資料庫 session 的帳號標註（Oracle）

Docker/Web 模式下資料庫連線由 **DBX 主機（容器）** 建立，因此 Oracle 端看到的 `OSUSER` 是 `root`、`MACHINE` 是容器 ID——這兩欄由客戶端環境決定，SQL 改不了。可以標註的是 session 屬性：

| `V$SESSION` 欄位 | 設定方式 |
| --- | --- |
| `CLIENT_IDENTIFIER` | `DBMS_SESSION.SET_IDENTIFIER` |
| `MODULE` / `ACTION` | `DBMS_APPLICATION_INFO.SET_MODULE('DBX', <label>)` |
| `CLIENT_INFO` | `DBMS_APPLICATION_INFO.SET_CLIENT_INFO(<label>)` |

`session_label` 一路從 Web session 傳到 agent：

1. `crates/dbx-web/src/routes/query.rs`：`Extension<UserSession>` → `QueryExecutionOptions.account_label`
   = `UserSession::account_label()`（顯示名稱，空白時退回帳號名稱）。
2. `crates/dbx-core/src/connection.rs`：`get_or_create_pool_for_session_with_label()` 轉呼叫
   `get_or_create_pool_for_session_inner(..., account_label)`。**只有 tab-scoped pool 會帶標註**：
   連線層級 pool 是所有帳號共用，命名成某一個人會是錯的，因此該情況下標註會被丟棄。
3. `crates/dbx-core/src/agent_connection.rs`：`agent_connect_params_with_role(..., account_label)`
   在 JSON 多加一個 `session_label`；舊版 agent 會忽略未知欄位，協定維持向後相容。
4. `agents/drivers/oracle-go/main.go`：`sessionLabelConnector` 包裝 `driver.Connector`，
   **每一條實體連線**（`SetMaxOpenConns(4)`）建立時都執行一次上述 PL/SQL，`MODULE` 固定為 `DBX`。
   - 標註失敗只寫 stderr，不阻擋連線：標註是輔助資訊，不該讓使用者無法查詢。
   - 依 Oracle 限制截斷（CLIENT_IDENTIFIER / CLIENT_INFO 64 bytes、ACTION 32 bytes），且在 UTF-8 字元邊界截斷。

未涵蓋：桌面版（沒有帳號概念，不標註）、MCP（沒有 tab session）、批次與 SQL 檔執行（走連線層級共用 pool）。若要涵蓋這些，需要改成每個帳號各自的連線池。

## 8. 已知邊界與待決事項

1. **隧道 profile 解析仍跨帳號**：清單個人化，但連線（含尚未儲存的連線）用 profile id 查找 `load_all_tunnel_profiles()`。要嚴格依帳號，未儲存連線將無法用他人 profile id，且測試連線前必須先儲存。
2. **AI 回合上限／重試次數**目前是實例層級（僅管理員可寫）。若要改成每人一組，改放 `user_settings`。
3. **片段同步 token**目前與 WebDAV 一起限管理員。真正「每人自己的同步」需要重做整個同步子系統（token、gist id、上傳下載範圍）。
4. **`mq_token_records`** 仍是實例共用（`mq/service.rs` 未帶 `user_id`），且 `delete_user` 的清理對這些列無效。
5. **Web 的 UI 偏好與分頁存瀏覽器**，不跨裝置。
6. **「關於我們」在 Web 只給管理員**（因內含「重設所有預設值」）。實際上那顆按鈕只重設自己的 UI／桌面偏好，不會影響他人；若希望一般使用者仍能重設，改成只把按鈕限管理員、頁籤開放。
7. **Web MCP 尚未支援多人帳號登入**：`WebBackend` 只用 `DBX_WEB_PASSWORD` 送 `/api/auth/login`，而伺服器要求 `username`；`/api/connection/list` 本來就需要 session。Web 模式要能指定帳號（例如 `DBX_WEB_USERNAME`）才能讓「每個帳號各自的 MCP 策略」完整生效。
8. 備份匯入匯出以帳號為單位（`export_backup/import_backup` 帶 `user_id`），`USER_DATA_TABLES` 未含片段與隧道表，匯入匯出時需注意。

---

## 9. 本機建置與測試（此開發機的坑）

直接跑 `cargo` 會因為 MSVC 標頭／OpenSSL 而失敗，需先載入 VS2019 BuildTools 環境。作一個包裝批次檔：

```bat
@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
cd /d D:\Code_AI\dbx
cargo %*
```

```powershell
%TEMP%\dbx-rust.bat check -p dbx-core --tests --no-default-features --features mq-admin
%TEMP%\dbx-rust.bat test  -p dbx-core --no-default-features --features mq-admin --lib
%TEMP%\dbx-rust.bat test  -p dbx-web  --no-default-features --features mq-admin
%TEMP%\dbx-rust.bat test  -p dbx-mcp  --no-default-features
%TEMP%\dbx-rust.bat check -p dbx --tests --no-default-features      # Tauri（測試執行檔在本機無法啟動，只能編譯檢查）
```

- `dbx-web` 要帶 `--features mq-admin` 才會編譯 `routes/mq.rs` 的測試。
- 前端：`npx vitest run`、`npx vue-tsc --noEmit --project apps/desktop/tsconfig.json`、`npx oxfmt --check "apps/desktop/src/**/*.{ts,vue}"`、`npx oxlint --vue-plugin apps/desktop/src`。
- commit 前的 `lint-staged` 會對 staged 的 Rust 與前端檔案執行 formatter，失敗會擋下 commit（oxfmt 對 `.vue` 會做語法檢查）。

### 本機已知失敗（與帳號切分無關）

| 測試 | 原因 |
| --- | --- |
| `agent_manager::tests::resolves_manifest_agent_launch_with_driver_dir_templates` | Windows 路徑分隔符 |
| `packages/app-tests/i18nAutofillParser.test.ts` | `i18n-autofill.mjs` 在 Windows 用反斜線呼叫 `git show` |
| `windowsInstallerTemplate.spec.ts` | vendored `wry` 與測試預期不同步 |
| `docs-export/exportSmoke.spec.ts` | 需要能編譯 C 相依（MSVC 環境） |

---

## 10. 版本與建置識別（如何確認容器跑的是哪一版）

改版號與建置識別是**部署可驗證性**的一部分：多帳號環境下常需要確認「跑起來的容器到底有沒有含這次修正」。

### 10.1 版號規則

- 版號同時存在 4 個檔案，改版時必須一起跳號（否則前端顯示、Docker tag、`/api/version` 會不一致）：
  `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`crates/dbx-web/Cargo.toml`。
- **`Cargo.lock` 也要跟著改**：`[[package]] name = "dbx"` 與 `name = "dbx-web"` 兩處的 `version`。
  （Docker 建置不會去更新 lock file，沒改會建置失敗。）
- 判定「要不要跳版」的標準：**只要動到使用者可見行為或需要重建容器，就跳 patch 版號**（`0.5.82` → `0.5.83`）。只有內部重構、測試、註解可以不跳。
- Web 版對外顯示的版號是 `crates/dbx-web/Cargo.toml` 的 `CARGO_PKG_VERSION`（`/api/version`、`/api/auth/check` 都回傳它）。

### 10.2 建置識別（commit + 建置時間）

`crates/dbx-web/build.rs` 會在編譯時把兩個值寫進執行檔：

| 環境變數 | 說明 | 未提供時 |
| --- | --- | --- |
| `DBX_BUILD_COMMIT` | git revision（取前 12 字元） | 執行 `git rev-parse --short=12 HEAD`，失敗則 `unknown` |
| `DBX_BUILD_TIME` | 建置當下的 Unix 毫秒 | build.rs 執行時間 |

Docker 建置 context 不含 `.git`（見 `deploy/Dockerfile.dockerignore`），所以 `deploy/Dockerfile` 的 backend stage 有
`ARG DBX_BUILD_COMMIT`，並由 `.github/workflows/release.yml`、`docker-dev.yml` 以
`build-args: DBX_BUILD_COMMIT=${{ github.sha }}` 帶入。手動建置時：

```bash
docker build -f deploy/Dockerfile --build-arg DBX_BUILD_COMMIT=$(git rev-parse --short=12 HEAD) .
```

API 曝露位置（兩者都是**免登入**，登入頁才讀得到）：

- `GET /api/version` → `{ "version": "0.5.82", "commit": "6b12c3005abc", "buildTimeMs": "1767225600000" }`
  （`/api/version` 位於需登入的 router 內，Web 前端另有 `auth/*` 例外清單。）
- `GET /api/auth/check` → 除 `required` / `authenticated` / `setup_required` 外，多回傳 `version` / `commit` / `buildTimeMs`。

前端 `apps/desktop/src/components/auth/LoginPage.vue` 會呼叫 `/api/auth/check`，把結果交給
`apps/desktop/src/lib/app/buildLabel.ts` 的 `formatBuildLabel()`，在登入畫面下方顯示
`v0.5.82 · 6b12c3005abc · 2026-01-01 02:30`（UTC）。桌面版（Tauri）一樣走這個頁面。

### 10.3 確認部署版本的實務做法

```bash
# 1. 前端有沒有含某次修正（以 adminOnlySetting 這個新增的閘門為例）
docker inspect -f '{{.Created}}' dbx-multi-account
docker exec dbx-multi-account grep -rl "adminOnlySetting" /app/static | head

# 2. 後端 build identity（最準，因為 build.rs 是編譯期寫入）
curl -s http://localhost:4224/api/auth/check | jq '{version, commit, buildTimeMs}'

# 3. 登入頁直接看版本字串
```

**注意**：Docker image 只建置前端與 `dbx-web`（`deploy/Dockerfile.dockerignore` 排除 `agents/`），
所以 **Oracle session 標註（§7）需要另外重建並安裝 `agents/drivers/oracle-go` 的執行檔**，
只重建 image 不會生效。重建 image 也無法取代 `Cargo.lock` / 版號的更新。

---

## 11. 主要程式碼位置
| 主題 | 檔案 |
| --- | --- |
| 帳號欄位、遷移、個人化 storage | `crates/dbx-core/src/storage.rs`（`init_schema`、`legacy_scope_owner`、`*_for_user`、`load_mcp_user_policy`、`connection_user_id`） |
| 使用者與 `user_settings` | `crates/dbx-core/src/user.rs` |
| MCP 策略（Rust runtime） | `crates/dbx-mcp/src/backend.rs`、`crates/dbx-mcp/src/server.rs` |
| Web 末端防護與策略解析 | `crates/dbx-web/src/routes/mcp_policy.rs` |
| Web 路由（session／管理員） | `crates/dbx-web/src/routes/*`、`crates/dbx-web/src/auth.rs`、`crates/dbx-web/src/error.rs`（`AppError::forbidden`） |
| 桌面指令 | `src-tauri/src/commands/*`、`src-tauri/src/commands/mcp_bridge.rs` |
| 前端個人化與閘門 | `apps/desktop/src/lib/backend/browserAppStateStorage.ts`、`apps/desktop/src/lib/backend/http.ts`、`apps/desktop/src/stores/settingsStore.ts`、`apps/desktop/src/components/editor/EditorSettingsDialog.vue`、`apps/desktop/src/components/editor/AiAssistant.vue`、`apps/desktop/src/App.vue` |
