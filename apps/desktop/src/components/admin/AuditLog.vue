<script setup lang="ts">
import { ref, onMounted } from "vue";
import { apiUrl } from "@/lib/common/webPath";
import { useUserStore } from "@/stores/userStore";
import { Button } from "@/components/ui/button";
import { Loader2, Shield, Database, Filter } from "@lucide/vue";

const userStore = useUserStore();
const loading = ref(false);
const tab = ref<"audit" | "sql">("audit");

interface AuditLogEntry {
  id: string;
  userId: string;
  username: string;
  action: string;
  details: string | null;
  ipAddress: string | null;
  success: boolean;
  createdAt: string;
}

interface HistoryWithUser {
  id: string;
  connectionName: string;
  database: string;
  sql: string;
  executedAt: string;
  executionTimeMs: number;
  success: boolean;
  error: string | null;
  activityKind: string;
  connectionId: string;
  userId: string;
  username: string;
}

const auditLogs = ref<AuditLogEntry[]>([]);
const sqlHistory = ref<HistoryWithUser[]>([]);
const filterUserId = ref("");
const filterAction = ref("");

async function loadAuditLogs() {
  loading.value = true;
  try {
    const params = new URLSearchParams();
    if (filterUserId.value) params.set("userId", filterUserId.value);
    if (filterAction.value) params.set("action", filterAction.value);
    params.set("limit", "200");
    const res = await fetch(apiUrl(`/api/admin/audit-logs?${params}`));
    if (res.ok) auditLogs.value = await res.json();
  } finally {
    loading.value = false;
  }
}

async function loadSqlHistory() {
  loading.value = true;
  try {
    const params = new URLSearchParams();
    if (filterUserId.value) params.set("userId", filterUserId.value);
    params.set("limit", "200");
    const res = await fetch(apiUrl(`/api/admin/sql-history?${params}`));
    if (res.ok) sqlHistory.value = await res.json();
  } finally {
    loading.value = false;
  }
}

function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

function actionLabel(action: string) {
  const map: Record<string, string> = {
    login: "Login",
    logout: "Logout",
    login_failed: "Login Failed",
  };
  return map[action] || action;
}

onMounted(() => {
  loadAuditLogs();
});
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center gap-2">
      <Shield class="w-5 h-5 text-primary" />
      <h2 class="text-lg font-semibold">Audit & Activity Log</h2>
    </div>

    <!-- Tab switcher -->
    <div class="flex gap-2 border-b">
      <button
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
        :class="tab === 'audit' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'"
        @click="
          tab = 'audit';
          loadAuditLogs();
        "
      >
        Login Audit Log
      </button>
      <button
        class="px-4 py-2 text-sm font-medium border-b-2 transition-colors"
        :class="tab === 'sql' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'"
        @click="
          tab = 'sql';
          loadSqlHistory();
        "
      >
        SQL Command History
      </button>
    </div>

    <!-- Filters -->
    <div class="flex items-center gap-2 text-sm">
      <Filter class="w-4 h-4 text-muted-foreground" />
      <input v-model="filterUserId" type="text" placeholder="Filter by user ID" class="flex h-8 w-40 rounded-md border border-input bg-background px-3 text-sm" />
      <input v-if="tab === 'audit'" v-model="filterAction" type="text" placeholder="Filter by action (login, logout, login_failed)" class="flex h-8 w-56 rounded-md border border-input bg-background px-3 text-sm" />
      <Button size="sm" variant="outline" @click="tab === 'audit' ? loadAuditLogs() : loadSqlHistory()"> Refresh </Button>
    </div>

    <div v-if="loading" class="flex items-center gap-2 text-muted-foreground"><Loader2 class="w-4 h-4 animate-spin" /> Loading...</div>

    <!-- Audit Log Table -->
    <div v-else-if="tab === 'audit' && auditLogs.length > 0" class="rounded-md border overflow-auto max-h-[500px]">
      <table class="w-full text-sm">
        <thead class="bg-muted/50 sticky top-0">
          <tr>
            <th class="text-left p-2 font-medium">Time</th>
            <th class="text-left p-2 font-medium">User</th>
            <th class="text-left p-2 font-medium">Action</th>
            <th class="text-left p-2 font-medium">Status</th>
            <th class="text-left p-2 font-medium">IP</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="log in auditLogs" :key="log.id" class="border-t hover:bg-muted/30">
            <td class="p-2 whitespace-nowrap">{{ formatTime(log.createdAt) }}</td>
            <td class="p-2">{{ log.username || log.userId || "—" }}</td>
            <td class="p-2">{{ actionLabel(log.action) }}</td>
            <td class="p-2">
              <span v-if="log.success" class="text-green-600">✓ Success</span>
              <span v-else class="text-red-500">✗ Failed</span>
            </td>
            <td class="p-2 text-muted-foreground">{{ log.ipAddress || "—" }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- SQL History Table -->
    <div v-else-if="tab === 'sql' && sqlHistory.length > 0" class="rounded-md border overflow-auto max-h-[500px]">
      <table class="w-full text-sm">
        <thead class="bg-muted/50 sticky top-0">
          <tr>
            <th class="text-left p-2 font-medium">Time</th>
            <th class="text-left p-2 font-medium">User</th>
            <th class="text-left p-2 font-medium">Connection</th>
            <th class="text-left p-2 font-medium">Database</th>
            <th class="text-left p-2 font-medium">SQL</th>
            <th class="text-left p-2 font-medium">Status</th>
            <th class="text-right p-2 font-medium">Time (ms)</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in sqlHistory" :key="entry.id" class="border-t hover:bg-muted/30">
            <td class="p-2 whitespace-nowrap">{{ formatTime(entry.executedAt) }}</td>
            <td class="p-2">{{ entry.username || "—" }}</td>
            <td class="p-2">{{ entry.connectionName }}</td>
            <td class="p-2">{{ entry.database }}</td>
            <td class="p-2 max-w-[400px] truncate font-mono text-xs" :title="entry.sql">{{ entry.sql }}</td>
            <td class="p-2">
              <span v-if="entry.success" class="text-green-600">✓</span>
              <span v-else class="text-red-500" :title="entry.error || ''">✗</span>
            </td>
            <td class="p-2 text-right text-muted-foreground">{{ entry.executionTimeMs }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <p v-else-if="!loading" class="text-sm text-muted-foreground text-center py-8">No records found.</p>
  </div>
</template>
