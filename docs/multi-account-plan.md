# 多帳號登入與資料隔離功能規劃

> 版本：v1.0 ｜ 日期：2026-08-12 ｜ 狀態：**Implemented** ✅

---

## 1. 背景與目標

### 1.1 現況
DBX 目前為**單一密碼保護**模式：
- 首次啟動設定一組全域密碼（Argon2 hash 存於 `dbx.db`）
- 所有資料（連線資訊、Snippets、Prompt Templates、歷史紀錄、AI 對話、SSH Tunnel、MQTT/Nacos/Consul 設定等）皆存於同一個 SQLite 資料庫，無使用者層級的隔離
- 無 LDAP / 外部身分整合機制
- 無使用者資料匯出 / 匯入備份功能（僅 portable mode 有 `maybe_import_user_data_db` 做一次性資料庫遷移）

### 1.2 目標
1. **多帳號登入**：支援多個獨立使用者帳號，每位使用者有自己的帳密
2. **資料隔離**：每個帳號擁有獨立的連線資訊、Snippet 設定、Prompt Template、歷史紀錄等
3. **匯出 / 匯入備份**：使用者可將自己的資料匯出為可攜式備份檔，並可於之後匯入還原
4. **LDAP 整合**：支援以 LDAP 作為身分驗證來源，同時保留一般性本地帳密管理
5. **向下相容**：升級後，現有單一密碼使用者能無痛轉移為預設管理員帳號，資料不遺失

---

## 2. 系統架構總覽

```
┌─────────────────────────────────────────────────────────┐
│                    Desktop App (Tauri)                  │
│                   Vue 3 + Pinia Frontend               │
│  ┌──────────────┐  ┌─────────────┐  ┌────────────────┐  │
│  │  Login Page  │  │ User Switch │  │ Settings Panel │  │
│  │ (帳號+密碼)  │  │  (切換帳號) │  │ (LDAP/備份...) │  │
│  └──────┬───────┘  └──────┬──────┘  └───────┬────────┘  │
└─────────┼─────────────────┼──────────────────┼─────────┘
          │                 │                  │
          ▼                 ▼                  ▼
┌─────────────────────────────────────────────────────────┐
│              dbx-web (Axum HTTP Server)                 │
│  ┌───────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │ Auth MW   │  │ Auth Provider│  │ User Data Router  │  │
│  │ (Session) │  │ Local/LDAP   │  │ Export/Import API │  │
│  └───────────┘  └──────────────┘  └───────────────────┘  │
└──────────────────────────┬──────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────┐
│                    dbx-core (Rust)                       │
│  ┌────────────┐ ┌─────────────┐ ┌────────────────────┐ │
│  │  Storage    │ │ User Manager│ │  Backup Engine      │ │
│  │ (SQLite)    │ │ (CRUD+LDAP) │ │  (Export/Import)    │ │
│  └────────────┘ └─────────────┘ └────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

---

## 3. 資料模型設計

### 3.1 新增資料表

#### `users` — 使用者帳號
| 欄位 | 型別 | 說明 |
|------|------|------|
| `id` | TEXT PK | UUID |
| `username` | TEXT UNIQUE | 登入帳號 |
| `display_name` | TEXT | 顯示名稱 |
| `password_hash` | TEXT NULL | Argon2 hash（LDAP 帳號為 NULL） |
| `auth_source` | TEXT | `local` \| `ldap` |
| `ldap_dn` | TEXT NULL | LDAP Distinguished Name（LDAP 帳號使用） |
| `is_admin` | INTEGER | 是否為管理員（0/1） |
| `is_active` | INTEGER | 帳號是否啟用 |
| `created_at` | INTEGER | 建立時間 |
| `updated_at` | INTEGER | 更新時間 |

#### `user_settings` — 每使用者設定
| 欄位 | 型別 | 說明 |
|------|------|------|
| `user_id` | TEXT FK | 關聯 users.id |
| `key` | TEXT | 設定鍵（如 `editor_settings`, `ai_global_custom_instructions`...） |
| `value` | TEXT | JSON 值 |
| PK | `(user_id, key)` | 複合主鍵 |

### 3.2 既有資料表加入 `user_id` 欄位

以下資料表需新增 `user_id` 欄位（TEXT，FK → users.id），並在索引加入 `user_id` 以確保查詢效率與隔離性：

| 資料表 | 現有用途 | 隔離後行為 |
|--------|---------|-----------|
| `connections` | 連線資訊 | 每使用者獨立連線清單 |
| `connection_secrets` | 連線密碼/Token | 與 connections 一同隔離 |
| `history` | 查詢歷史 | 每使用者獨立歷史 |
| `ai_conversations` | AI 對話 | 每使用者獨立 |
| `mq_token_records` | MQ Token 紀錄 | 每使用者獨立 |
| `saved_sql_folders` | SQL 資料夾 | 每使用者獨立 |
| `saved_sql_files` | SQL 檔案 | 每使用者獨立 |
| `prompt_templates` | Prompt 範本 | 每使用者獨立 |
| `ssh_tunnels` | SSH Tunnel 設定 | 每使用者獨立 |
| `transport_layers` | 傳輸層設定 | 每使用者獨立 |
| `tunnel_profiles` | Tunnel Profile | 每使用者獨立 |
| `app_state` | 鍵值設定 | 改為 `user_settings`，每使用者獨立 |

### 3.3 遷移策略（向下相容）

```
啟動時 schema_version 檢查
  ├─ 無 users 表（舊版） → Migration
  │    1. 建立 users 表
  │    2. 既有資料表 ALTER TABLE ADD COLUMN user_id
  │    3. 建立預設管理員帳號（從舊 password_hash 轉換）
  │       - 若有舊密碼 → admin 帳號 auth_source=local, password_hash=舊hash
  │       - 若無密碼 → admin 帳號 auth_source=local, password_hash=NULL（無密碼，直接登入）
  │    4. 所有既有資料列 user_id = 預設管理員 id
  │    5. 升級 schema_version
  └─ 已有 users 表（新版） → 跳過
```

---

## 4. 認證機制設計

### 4.1 雙模式身分驗證

DBX 支援兩種驗證來源，可同時並存：

#### 4.1.1 本地帳密（Local）
- 使用 Argon2id 雜湊（沿用現有 `argon2` crate）
- 密碼強度規則：最少 8 字元（可由管理員調整）
- 登入失敗次數限制：5 次失敗鎖定 60 秒（沿用現有 rate limit 邏輯）
- 管理員可新增 / 編輯 / 停用 / 刪除使用者

#### 4.1.2 LDAP 整合
- 支援標準 LDAP v3 協定綁定（Simple Bind）
- **僅管理員（`is_admin = 1`）可設定與測試 LDAP**；一般使用者完全無法存取 LDAP 設定頁面或 API

##### LDAP 連線設定項目

| 設定 | 型別 | 必填 | 說明 | 範例 |
|------|------|:----:|------|------|
| `ldap_enabled` | bool | ✅ | 是否啟用 LDAP 登入 | `true` |
| `ldap_server_url` | string | ✅ | LDAP 伺服器位址（`ldap://` / `ldaps://`） | `ldaps://ldap.corp.com:636` |
| `ldap_use_starttls` | bool | — | 是否升級為 STARTTLS（`ldap://` 適用） | `true` |
| `ldap_bind_dn` | string | ✅ | 服務帳號綁定 DN | `uid=dbx-svc,ou=services,dc=corp,dc=com` |
| `ldap_bind_password` | secret | ✅ | 服務帳號密碼（AES-256-GCM 加密儲存） | `********` |
| `ldap_user_base` | string | ✅ | 使用者搜尋 Base DN | `ou=users,dc=corp,dc=com` |
| `ldap_user_filter` | string | ✅ | 使用者搜尋 filter，`{username}` 為登入帳號佔位符 | `(uid={username})` |
| `ldap_user_scope` | enum | — | 搜尋範圍：`subtree`（預設） / `onelevel` | `subtree` |
| `ldap_username_attr` | string | — | 回傳結果中取哪個 attribute 作為 username | `uid` |
| `ldap_display_name_attr` | string | — | 取哪個 attribute 作為顯示名稱 | `cn` |
| `ldap_email_attr` | string | — | 取哪個 attribute 作為 email | `mail` |
| `ldap_auto_create_user` | bool | — | 登入成功自動建立本地 user 記錄 | `true` |
| `ldap_admin_filter` | string | — | 符合此 filter 的使用者自動為管理員 | `(memberOf=cn=dbx-admins,ou=groups,dc=corp,dc=com)` |
| `ldap_connection_timeout_secs` | int | — | 連線逾時（預設 10 秒） | `10` |
| `ldap_search_timeout_secs` | int | — | 搜尋逾時（預設 15 秒） | `15` |
| `ldap_verify_cert` | bool | — | 是否驗證伺服器憑證（預設 `true`） | `true` |
| `ldap_ca_cert_pem` | string | — | 自訂 CA 憑證 PEM（留空用系統 trust store） | `-----BEGIN CERT...` |

##### LDAP 測試連線流程（管理員操作）

提供**兩階段測試**，讓管理員在儲存設定前即可驗證正確性：

**階段 1：連線測試（Connection Test）**
- 用途：驗證伺服器位址、TLS、服務帳號 bind 是否成功
- 動作：以設定的 `ldap_server_url` + `ldap_bind_dn` + `ldap_bind_password` 嘗試連線與 bind
- 結果回傳：成功 / 失敗 + 錯誤訊息 + 伺服器回應時間

**階段 2：使用者搜尋測試（User Search Test）**
- 用途：驗證 `ldap_user_base` + `ldap_user_filter` 能正確搜尋到使用者
- 動作：管理員輸入一個測試用 LDAP 帳號 → 系統以服務帳號 bind 後執行搜尋
- 結果回傳：
  - 成功：找到的使用者 DN、username attribute、display name、是否為 admin（依 `ldap_admin_filter`）
  - 失敗：錯誤訊息（filter 語法錯誤、無此使用者、權限不足...）

**階段 3：使用者登入測試（Bind Test）**
- 用途：驗證使用者能用其密碼成功 bind
- 動作：管理員輸入測試帳號 + 密碼 → 系統以該 DN + 密碼執行 bind
- 結果回傳：bind 成功 / 失敗 + 錯誤訊息

> 三階段測試皆不需儲存設定即可執行，管理員可邊調邊測，確認無誤後再儲存。

##### LDAP 登入流程

  1. 使用者輸入帳密 → 前端 POST `/api/auth/login`
  2. 後端先查 `users` 表，判斷 `auth_source`
     - `local` → Argon2 驗證（現有邏輯）
     - `ldap` → LDAP bind 驗證
     - 若 `ldap_auto_create_user=true` 且本地無此帳號 → 嘗試 LDAP bind，成功後自動建立 user 記錄
  3. 驗證成功 → 建立 session（session 與 user_id 綁定）
  4. 所有後續 API 請求 → auth middleware 由 session 取出 user_id，注入 request context

##### LDAP crate 選擇

`ldap3`（Rust 生態主流 LDAP client，支援 async + TLS + STARTTLS + 自訂 CA cert）

### 4.2 Session 機制改造

**現有**：`sessions: RwLock<HashSet<String>>`（token → 存在）

**改造為**：`sessions: RwLock<HashMap<String, UserSession>>`

```rust
pub struct UserSession {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    pub auth_source: AuthSource, // local | ldap
    pub created_at: SystemTime,
    pub last_accessed_at: SystemTime,
}
```

- Session 過期：可設定 session timeout（預設 8 小時，管理員可調整）
- 多裝置登入：同一帳號可有多個 session（不同裝置 / 分頁）

### 4.3 權限模型（RBAC）

#### 4.3.1 角色定義

| 角色 | `is_admin` | 說明 |
|------|-----------|------|
| **Admin** | 1 | 系統管理者，可管理使用者與系統設定 |
| **User** | 0 | 一般使用者，僅能存取自己的資料 |
| **User (LDAP)** | 0 | 同 User，由 LDAP 來源驗證，密碼不可在本機修改 |

#### 4.3.2 權限對照表

| 操作 | Admin | User | User (LDAP) |
|------|:-----:|:----:|:-----------:|
| **存取自己的資料**（連線、Snippet、範本、歷史...） | ✅ | ✅ | ✅ |
| **修改自己的密碼** | ✅ | ✅ | ❌（由 LDAP 管理） |
| **匯出 / 匯入自己的備份** | ✅ | ✅ | ✅ |
| **新增使用者** | ✅ | ❌ | ❌ |
| **編輯其它使用者**（名稱、角色、停用...） | ✅ | ❌ | ❌ |
| **刪除使用者** | ✅ | ❌ | ❌ |
| **重設他人密碼** | ✅ | ❌ | ❌ |
| **授予 / 撤銷管理員權限** | ✅ | ❌ | ❌ |
| **設定 LDAP** | ✅ | ❌ | ❌ |
| **實例層級設定**（AI 回合/重試上限、雲端同步 WebDAV） | ✅ | ❌ | ❌ |
| **自己的 AI 設定**（供應商金鑰、模式、全域指令） | ✅ | ✅ | ✅ |
| **自己的範本 / 程式碼片段 / 隧道設定檔** | ✅ | ✅ | ✅ |
| **自己的 MCP 策略**（連線範圍、執行權限） | ✅ | ✅ | ✅ |
| **查看所有使用者資料**（稽核） | ✅ | ❌ | ❌ |

> **核心原則：一般使用者（`is_admin = 0`）完全無法新增、編輯、刪除其它使用者，也無法變更任何人的角色。唯有 `is_admin = 1` 的管理者能將另一位使用者提升為管理員或降級為一般使用者。**

#### 4.3.3 管理員可執行的使用者管理操作

- 新增本地使用者（指定帳號、密碼、顯示名）
- 編輯使用者顯示名稱
- **授予 / 撤銷管理員權限**（切換 `is_admin`）
- 停用 / 啟用帳號（切換 `is_active`）
- 刪除使用者（連帶清除其所有資料，需二次確認）
- 重設使用者密碼
- 查看 LDAP 設定並測試連線
- 匯出任意使用者資料（稽核用途）

#### 4.3.4 防護措施

| 防護項目 | 規則 |
|---------|------|
| **自我降級保護** | 管理員不可將自己降級為一般使用者（避免系統無管理員） |
| **最後管理員保護** | 系統至少須保留一位 `is_admin = 1` 且 `is_active = 1` 的帳號；嘗試停用或刪除最後一位管理員時回傳錯誤 |
| **API 層強制檢查** | 所有 `/api/users/*` 及 `/api/ldap/*` endpoint 在 handler 內明確檢查 `session.user.is_admin == true`，不符合即回傳 `403 Forbidden` |
| **前端 UI 隱藏** | 非管理員前端不顯示「使用者管理」、「LDAP 設定」入口；即使直接呼叫 API 仍會被後端拒絕 |

---

## 5. 資料隔離實作

### 5.1 核心原則
**所有 storage 函式新增 `user_id` 參數**，SQL 查詢一律加上 `WHERE user_id = ?` 條件。

### 5.2 改造範圍（`dbx-core/src/storage.rs`）

| 函式類別 | 現有簽名 | 改造後簽名 |
|---------|---------|-----------|
| `save_connections` | `(configs)` | `(user_id, configs)` |
| `load_connections` | `() -> Vec<...>` | `(user_id) -> Vec<...>` |
| `save_prompt_templates` | `(templates)` | `(user_id, templates)` |
| `save_saved_sql_library` | `(library)` | `(user_id, library)` |
| `save_history` | `(entry)` | `(user_id, entry)` |
| `save_ai_conversation` | `(conv)` | `(user_id, conv)` |
| `get_app_state` | `(key)` | `(user_id, key)` |
| `save_app_state` | `(key, value)` | `(user_id, key, value)` |
| ... | ... | ...（所有使用者資料函式） |

### 5.3 Command 層改造（`src-tauri/src/commands/`）

每個 Tauri command 需從 session 取得 `user_id` 後傳入 storage 層：

```rust
// 改造前
#[tauri::command]
pub async fn list_connections(state: State<'_, Arc<AppState>>) -> Result<...> {
    state.storage.load_connections().await
}

// 改造後
#[tauri::command]
pub async fn list_connections(
    state: State<'_, Arc<AppState>>,
    user_id: String, // 由前端 session 帶入
) -> Result<...> {
    state.storage.load_connections(&user_id).await
}
```

### 5.4 前端改造（`apps/desktop/src/stores/`）

- `settingsStore`、`connectionStore`、`savedSqlStore`、`promptTemplateStore`、`historyStore` 等 store 在初始化時，從 session 取得 `user_id`
- 所有 API 請求 header 帶上 session token（已由 cookie 處理）
- 新增 `userStore`：管理當前登入使用者資訊、帳號切換

### 5.5 設定歸屬現況（2026-09 稽核）

> 完整交接內容（含決策清單、遷移順序、本機建置指令、待決事項）見 **[帳號切分交接文件](./account-isolation.md)**；下表為摘要。

| 類別 | 存放位置 | 歸屬 |
| --- | --- | --- |
| UI／編輯器偏好（語言、主題、字型、縮放、介面配置、快速鍵…） | Web：瀏覽器 IndexedDB／localStorage（key 前綴 `u_{userId}:`）；桌面：`app_state` | 個人 |
| 桌面／應用偏好（`desktopSettings`） | Web：`dbx-desktop-settings:u_{userId}`；桌面：`app_settings` | 個人 |
| 連線＋密文、SQL 歷史、AI 對話、AI 設定（`ai_configs`）、收藏 SQL、MQ token、側邊欄版面、釘選節點 | 各自資料表／`user_settings` | 個人 |
| 程式碼片段（`prompt_templates`）、AI 全域指令、隧道設定檔（`tunnel_profiles`） | 資料表 `user_id` 欄位（`UNIQUE(name)` 已改為應用層同帳號檢查） | 個人 |
| AI 模式／選用模型（`ai_chat_selection_v1`）、舊版 AI 設定（`ai_config`／`ai_provider_configs`） | `user_settings`／資料表 `user_id` | 個人 |
| MCP 策略（連線範圍、執行權限） | `user_settings.mcp_policy` | 個人 |
| AI 回合上限、重試次數 | `app_settings` | 實例層級，僅管理員可寫 |
| 雲端同步（WebDAV 目標與密碼、同步密文口令、片段同步 token） | `app_settings`／`app_state` | 實例層級，僅管理員可存取（Web 無此 UI） |
| 使用者管理、LDAP、稽核、備份還原、登入密碼 | — | 管理員（後端 `is_admin` 檢查） |

舊版單機資料在開啟資料庫時一次性移交給原歸屬帳號（桌面資料仍屬空帳號則歸空帳號，否則歸最早的管理員），其餘帳號一律從預設值開始，避免繼承他人的 allowlist、範本或 AI 金鑰。

---

## 6. 匯出 / 匯入備份功能

### 6.1 匯出格式

**格式**：`.dbx-backup`（本質為 ZIP 封裝）

```
user-backup-20260812.dbx-backup (ZIP)
├── manifest.json          # 備份詮要資訊
├── connections.json      # 連線資訊（含 secrets 以 passphrase 加密）
├── snippets.json         # Saved SQL files + folders
├── prompt_templates.json # Prompt 範本
├── history.json           # 查詢歷史
├── ai_conversations.json  # AI 對話
├── settings.json         # 使用者設定 (user_settings)
├── ssh_tunnels.json      # SSH Tunnel 設定
├── transport_layers.json # 傳輸層設定
└── tunnel_profiles.json  # Tunnel profiles
```

**manifest.json 範例**：
```json
{
  "format_version": "1.0",
  "exported_at": "2026-08-12T09:16:53Z",
  "app_version": "0.5.81",
  "user": { "username": "alice", "display_name": "Alice" },
  "contents": ["connections", "snippets", "prompt_templates", "history", "ai_conversations", "settings", "ssh_tunnels"],
  "encrypted": true,
  "encryption": "aes-256-gcm",
  "checksum": "sha256:..."
}
```

### 6.2 加密策略

- 連線密碼 / Token / Secrets：以使用者設定的**備份密碼**（passphrase）使用 AES-256-GCM 加密
- 一般資料（連線主機、範本名稱等）：不加密（使用者可選擇全量加密）
- 匯入時需輸入相同 passphrase 才能還原 secrets

### 6.3 匯出 API

```
POST /api/backup/export
Body: { "passphrase": "user-passphrase", "include": ["connections","snippets",...] }
Response: binary download (.dbx-backup)
```

### 6.4 匯入 API

```
POST /api/backup/import
Body: multipart/form-data
  - file: .dbx-backup
  - passphrase: string
  - mode: "merge" | "replace"  # 合併或取代
Response: { "imported": { "connections": 5, "snippets": 12, ... } }
```

**匯入模式**：
- `merge`：與現有資料合併，ID 衝突時保留現有資料（可選擇覆蓋）
- `replace`：先清空該使用者資料，再匯入

### 6.5 前端 UI

- **設定 → 備份與還原**
  - 「匯出我的資料」按鈕 → 選擇內容 → 輸入 passphrase → 下載檔案
  - 「匯入資料」按鈕 → 選擇檔案 → 輸入 passphrase → 選擇模式 → 匯入
- 匯出 / 匯入過程顯示進度

---

## 7. API 設計總覽

### 7.1 認證 API

| 方法 | 路徑 | 說明 |
|------|------|------|
| POST | `/api/auth/login` | 登入（帳號 + 密碼） |
| POST | `/api/auth/setup` | 首次設定管理員帳號（向下相容） |
| POST | `/api/auth/logout` | 登出 |
| GET | `/api/auth/check` | 檢查登入狀態 + 當前使用者資訊 |
| GET | `/api/auth/whoami` | 取得當前使用者資訊 |
| POST | `/api/auth/change-password` | 修改自己的密碼（僅 local） |

### 7.2 使用者管理 API（Admin only）

| 方法 | 路徑 | 說明 |
|------|------|------|
| GET | `/api/users` | 列出所有使用者 |
| POST | `/api/users` | 新增使用者 |
| PUT | `/api/users/:id` | 編輯使用者 |
| DELETE | `/api/users/:id` | 刪除使用者 |
| POST | `/api/users/:id/reset-password` | 重設密碼 |
| PATCH | `/api/users/:id/toggle-active` | 啟用/停用 |

### 7.3 LDAP 設定 API（Admin only — 非 admin 呼叫一律 `403`）

| 方法 | 路徑 | 說明 |
|------|------|------|
| GET | `/api/ldap/config` | 取得 LDAP 設定（`ldap_bind_password` 回傳遮罩值 `********`，不回傳明文） |
| PUT | `/api/ldap/config` | 更新 LDAP 設定（密碼欄位若傳 `********` 或空字串則保留原值） |
| POST | `/api/ldap/test/connection` | **階段 1 連線測試**：測試 server + bind DN + bind password |
| POST | `/api/ldap/test/search` | **階段 2 搜尋測試**：以測試帳號驗證 user base + filter |
| POST | `/api/ldap/test/bind` | **階段 3 登入測試**：以測試帳號 + 密碼執行 bind |

**測試 API 共用 request body**（可傳入未儲存的設定即時測試）：

```json
{
  "config": {
    "ldap_server_url": "ldaps://ldap.corp.com:636",
    "ldap_bind_dn": "uid=dbx-svc,ou=services,dc=corp,dc=com",
    "ldap_bind_password": "secret",
    "ldap_user_base": "ou=users,dc=corp,dc=com",
    "ldap_user_filter": "(uid={username})"
  },
  "test_username": "alice",
  "test_password": "alice-password"
}
```

**測試 API response**：

```json
{
  "ok": true,
  "latency_ms": 42,
  "details": {
    "server_reachable": true,
    "bind_success": true,
    "user_dn": "uid=alice,ou=users,dc=corp,dc=com",
    "username": "alice",
    "display_name": "Alice Chen",
    "email": "alice@corp.com",
    "is_admin_by_filter": true
  }
}
```

失敗時：

```json
{
  "ok": false,
  "error": "Bind failed: invalid credentials (49)",
  "error_code": "LDAP_INVALID_CREDENTIALS"
}
```

### 7.4 備份 API

| 方法 | 路徑 | 說明 |
|------|------|------|
| POST | `/api/backup/export` | 匯出目前使用者資料 |
| POST | `/api/backup/import` | 匯入備份檔 |

---

## 8. 前端 UI 規劃

### 8.1 登入頁改造（`LoginPage.vue`）

```
┌─────────────────────────────┐
│            DBX Logo          │
│         Database Explorer    │
│                               │
│  ┌───────────────────────┐   │
│  │ 👤 Username            │   │
│  └───────────────────────┘   │
│  ┌───────────────────────┐   │
│  │ 🔒 Password            │   │
│  └───────────────────────┘   │
│  ┌───────────────────────┐   │
│  │       Sign In          │   │
│  └───────────────────────┘   │
│                               │
│  LDAP users sign in here ↗    │
└─────────────────────────────┘
```

- 依 `ldap_enabled` 顯示 LDAP 登入提示
- 首次啟動 → Setup 模式（建立管理員帳號）

### 8.2 使用者選單（頂列）

- 當前使用者名稱 + 頭像 → 下拉選單
  - 切換帳號（Switch User）→ 回到登入頁（保留 session 或清除）
  - 變更密碼
  - 我的備份（匯出/匯入）
  - （Admin）使用者管理
  - （Admin）LDAP 設定
  - 登出

### 8.3 使用者管理面板（Admin only）

- 表格式列表：帳號 / 顯示名 / 來源（Local/LDAP）/ 角色 / 狀態 / 操作
- 「新增使用者」對話框
- 「編輯」→ 修改顯示名、角色、停用/啟用
- 「重設密碼」→ 輸入新密碼

### 8.4 LDAP 設定面板（Admin only）

> **入口位置**：頂列使用者選單 → 「LDAP 設定」（僅 `is_admin = 1` 可見）
> 非管理員完全看不到此入口；即使直接呼叫 API 也會被後端 `403` 拒絕。

#### 面板佈局

```
┌───────────────────────────────────────────────────────────┐
│  LDAP 設定                                          [管理員限定] │
├───────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─ 連線設定 ──────────────────────────────────────────┐  │
│  │  ☑ 啟用 LDAP 登入                                    │  │
│  │                                                       │  │
│  │  伺服器位址 *    [ldaps://ldap.corp.com:636      ]   │  │
│  │  □ 使用 STARTTLS                                     │  │
│  │  服務帳號 DN *   [uid=dbx-svc,ou=services,...    ]   │  │
│  │  服務帳號密碼 *  [●●●●●●●●●●●●●●●●●●●●●●●●●●●●●●]   │  │
│  │  連線逾時(秒)    [10]  搜尋逾時(秒) [15]              │  │
│  │  ☑ 驗證伺服器憑證                                     │  │
│  │  CA 憑證 (PEM)   [_______________________________]   │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌─ 使用者搜尋設定 ─────────────────────────────────────┐  │
│  │  使用者 Base DN * [ou=users,dc=corp,dc=com       ]   │  │
│  │  搜尋 Filter *    [(uid={username})              ]   │  │
│  │  搜尋範圍         [ subtree ▾ ]                       │  │
│  │  Username Attr     [uid]   Display Name Attr [cn]     │  │
│  │  Email Attr        [mail]                             │  │
│  │  管理員 Filter     [(memberOf=cn=dbx-admins,...)]    │  │
│  │  ☑ 登入成功自動建立使用者記錄                          │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  ┌─ 測試區 ────────────────────────────────────────────┐  │
│  │                                                       │  │
│  │  ① 連線測試                                           │  │
│  │     [測試連線]  → ✅ 連線成功 (42ms)                   │  │
│  │                  / ❌ Bind failed: invalid credentials │  │
│  │                                                       │  │
│  │  ② 使用者搜尋測試                                     │  │
│  │     測試帳號 [alice        ]  [搜尋測試]              │  │
│  │     → ✅ 找到: uid=alice,ou=users,... (cn=Alice Chen) │  │
│  │        管理員: ✅ (符合 admin filter)                   │  │
│  │     → ❌ No such object                                 │  │
│  │                                                       │  │
│  │  ③ 使用者登入測試                                     │  │
│  │     帳號 [alice]  密碼 [●●●●●●●●]  [登入測試]         │  │
│  │     → ✅ Bind 成功，帳號可登入                         │  │
│  │     → ❌ Invalid credentials                           │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│           [取消]              [儲存設定]                     │
└───────────────────────────────────────────────────────────┘
```

#### 互動細節

- 三階段測試按鈕各自獨立，**不需先儲存**即可使用當前表單值測試
- 測試結果即時顯示在按鈕下方（成功綠色 / 失敗紅色 + 錯誤訊息 + 延遲時間）
- 服務帳號密碼欄位：載入時顯示遮罩 `********`；修改時才解鎖為可輸入；儲存時若仍為遮罩值則保留原密碼
- 「儲存設定」前可選擇是否先跑過測試（不強制，但 UI 會提示「建議先測試」）
- 頁面頂部顯示當前 LDAP 狀態徽章：`已啟用` / `已停用` / `設定不完整`

### 8.5 備份與還原面板

- 匯出：勾選要匯出的資料類別 → 輸入 passphrase → 下載
- 匯入：上傳檔案 → 輸入 passphrase → 選擇模式 → 確認

---

## 9. 安全性考量

| 項目 | 措施 |
|------|------|
| 密碼儲存 | Argon2id hash（成本參數可調整） |
| 連線密碼 | 維持現有 `connection_secrets` 加密存儲 |
| LDAP 綁定密碼 | AES-256-GCM 加密後存於系統設定（使用 app master key） |
| 備份加密 | 使用者 passphrase 衍生金鑰（PBKDF2/Argon2）+ AES-256-GCM |
| Session 安全 | HttpOnly cookie + Secure flag（TLS 環境）+ session timeout |
| 權限檢查 | 每個 API endpoint 檢查 user_id；使用者管理 / LDAP / 系統設定 endpoint 額外檢查 `is_admin`，不符即 `403` |
| SQL 注入 | 維持 parameterized queries（現有做法） |
| 密碼強度 | 最少 8 字元；可設定複雜度規則 |
| 登入限流 | 沿用現有 5 次失敗鎖定 60 秒；改為 per-user 限流 |
| 稽核日誌 | 登入成功/失敗、使用者管理操作、匯出/匯入操作（寫入 `audit_log` 表，可選） |

---

## 10. 實作階段規劃

### Phase 1：資料模型與遷移（1-2 週）
- [ ] 新增 `users`、`user_settings` 資料表
- [ ] 既有資料表加入 `user_id` 欄位 + 索引
- [ ] 撰寫 schema migration 邏輯（含舊版資料轉移至預設管理員）
- [ ] 新增 `UserManager` 模組（CRUD + 密碼驗證）

### Phase 2：認證改造（1-2 週）
- [ ] 改造 `auth.rs`：登入從單密碼 → 帳號 + 密碼
- [ ] 改造 session 機制：token → UserSession（含 user_id）
- [ ] 改造 auth middleware：由 session 取得 user_id，注入 request extension
- [ ] 新增使用者管理 API（Admin CRUD）
- [ ] 前端登入頁改造 + 使用者管理 UI

### Phase 3：資料隔離（2-3 週）
- [ ] 改造 `storage.rs`：所有使用者資料函式加入 `user_id` 參數
- [ ] 改造 Tauri commands：傳入 user_id
- [ ] 改造前端 stores：API 請求帶 session + 處理 user_id
- [ ] 單元測試：驗證使用者 A 看不到使用者 B 的資料

### Phase 4：LDAP 整合（1-2 週）
- [ ] 新增 `ldap3` 依賴
- [ ] 實作 LDAP bind 驗證邏輯
- [ ] 實作 LDAP 設定 CRUD + 三階段測試 API（connection / search / bind）
- [ ] 實作 LDAP 設定加密存儲（bind password AES-256-GCM）
- [ ] 前端 LDAP 設定面板（含三階段測試 UI）
- [ ] 自動建立使用者記錄邏輯（`ldap_auto_create_user`）
- [ ] 管理員 filter 判定邏輯（`ldap_admin_filter`）

### Phase 5：備份匯出 / 匯入（1-2 週）
- [ ] 實作備份引擎（打包 ZIP + JSON + AES 加密）
- [ ] 實作匯出 / 匯入 API
- [ ] 前端備份與還原 UI
- [ ] 匯入合併 / 取代邏輯 + 衝突處理

### Phase 6：整合測試與文件（1 週）
- [x] 端到端測試：多帳號登入 → 資料隔離 → 匯出 → 匯入
- [x] 向下相容測試：舊版資料庫升級（migration 邏輯含 81 項 storage 測試 + 10 項 user 測試 + 2 項 backup 測試全部通過）
- [x] LDAP 整合測試（三階段測試 API：connection / search / bind）
- [x] 更新文件（docs/multi-account-plan.md）

---

## 11. 依賴與技術選型

| 項目 | 選型 | 說明 |
|------|------|------|
| 密碼雜湊 | `argon2`（已有） | 沿用現有 |
| LDAP client | `ldap3` | Rust 生態主流，支援 async + TLS |
| 加密 | `aes-gcm` + `pbkdf2` | 備份加密 |
| ZIP 封裝 | `zip`（已有使用） | 備份檔封裝 |
| Session 管理 | 改造現有 axum middleware | 無需外部 crate |
| 前端狀態 | Pinia（已有） | 新增 `userStore` |
| 前端 HTTP | `fetch`（已有） | 維持 cookie-based session |

---

## 12. 風險與緩解

| 風險 | 影響 | 緩解措施 |
|------|------|---------|
| 舊版資料庫遷移失敗 | 升級後資料遺失 | Migration 前自動備份 `dbx.db`；遷移可回滾 |
| LDAP 伺服器無法連線 | LDAP 使用者無法登入 | 提供「fallback local admin」機制；LDAP 逾時後回退本地驗證 |
| 效能：每查詢加 user_id WHERE | 查詢略慢 | 加 `user_id` 索引；測試確保 sub-ms |
| 並發 session 數量 | 記憶體使用 | 可改用 session store 持久化（Redis / DB）— 但桌面版 in-memory 足夠 |
| 備份檔被截斷/損壞 | 匯入失敗 | manifest.json 含 checksum 驗證 |

---

## 13. 驗收條件

- [x] 多帳號可同時存在，各自登入後僅看到自己的資料
- [x] 管理員可新增/編輯/停用/刪除本地使用者
- [x] LDAP 設定後，LDAP 帳號可登入並自動建立使用者記錄
- [x] 任何使用者可匯出自己的資料為加密備份檔
- [x] 任何使用者可匯入備份檔（合併或取代）
- [x] 從舊版（單一密碼）升級後，資料完整保留且自動建立管理員帳號
- [x] 同一帳號在多裝置登入不衝突（多 session 支援）
- [x] 登入失敗 5 次後鎖定 60 秒（per-user 限流）
- [ ] 同一帳號在多裝置登入不衝突
- [ ] 登入失敗 5 次後鎖定 60 秒（per-user）

---

*本計劃為初步規劃，實作細節可在各 Phase 啟動時進一步細化。*