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

語意重點：**舊資料只交給一個帳號**。其他帳號一律從預設值開始，避免繼承到指向他人連線的 allowlist、他人的範本或 AI 金鑰。

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

## 7. 已知邊界與待決事項

1. **隧道 profile 解析仍跨帳號**：清單個人化，但連線（含尚未儲存的連線）用 profile id 查找 `load_all_tunnel_profiles()`。要嚴格依帳號，未儲存連線將無法用他人 profile id，且測試連線前必須先儲存。
2. **AI 回合上限／重試次數**目前是實例層級（僅管理員可寫）。若要改成每人一組，改放 `user_settings`。
3. **片段同步 token**目前與 WebDAV 一起限管理員。真正「每人自己的同步」需要重做整個同步子系統（token、gist id、上傳下載範圍）。
4. **`mq_token_records`** 仍是實例共用（`mq/service.rs` 未帶 `user_id`），且 `delete_user` 的清理對這些列無效。
5. **Web 的 UI 偏好與分頁存瀏覽器**，不跨裝置。
6. **「關於我們」在 Web 只給管理員**（因內含「重設所有預設值」）。實際上那顆按鈕只重設自己的 UI／桌面偏好，不會影響他人；若希望一般使用者仍能重設，改成只把按鈕限管理員、頁籤開放。
7. **Web MCP 尚未支援多人帳號登入**：`WebBackend` 只用 `DBX_WEB_PASSWORD` 送 `/api/auth/login`，而伺服器要求 `username`；`/api/connection/list` 本來就需要 session。Web 模式要能指定帳號（例如 `DBX_WEB_USERNAME`）才能讓「每個帳號各自的 MCP 策略」完整生效。
8. 備份匯入匯出以帳號為單位（`export_backup/import_backup` 帶 `user_id`），`USER_DATA_TABLES` 未含片段與隧道表，匯入匯出時需注意。

---

## 8. 本機建置與測試（此開發機的坑）

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

## 9. 主要程式碼位置

| 主題 | 檔案 |
| --- | --- |
| 帳號欄位、遷移、個人化 storage | `crates/dbx-core/src/storage.rs`（`init_schema`、`legacy_scope_owner`、`*_for_user`、`load_mcp_user_policy`、`connection_user_id`） |
| 使用者與 `user_settings` | `crates/dbx-core/src/user.rs` |
| MCP 策略（Rust runtime） | `crates/dbx-mcp/src/backend.rs`、`crates/dbx-mcp/src/server.rs` |
| Web 末端防護與策略解析 | `crates/dbx-web/src/routes/mcp_policy.rs` |
| Web 路由（session／管理員） | `crates/dbx-web/src/routes/*`、`crates/dbx-web/src/auth.rs`、`crates/dbx-web/src/error.rs`（`AppError::forbidden`） |
| 桌面指令 | `src-tauri/src/commands/*`、`src-tauri/src/commands/mcp_bridge.rs` |
| 前端個人化與閘門 | `apps/desktop/src/lib/backend/browserAppStateStorage.ts`、`apps/desktop/src/lib/backend/http.ts`、`apps/desktop/src/stores/settingsStore.ts`、`apps/desktop/src/components/editor/EditorSettingsDialog.vue`、`apps/desktop/src/components/editor/AiAssistant.vue`、`apps/desktop/src/App.vue` |
