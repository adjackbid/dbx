<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useUserStore } from "@/stores/userStore";
import { apiUrl } from "@/lib/common/webPath";
import { Button } from "@/components/ui/button";
import PasswordInput from "@/components/ui/PasswordInput.vue";
import { Loader2, Wifi, Search, LogIn, CheckCircle, XCircle } from "@lucide/vue";

const userStore = useUserStore();
const loading = ref(false);
const saving = ref(false);
const error = ref("");

const config = ref({
  enabled: false,
  serverUrl: "",
  useStarttls: false,
  bindDn: "",
  bindPassword: "",
  userBase: "",
  userFilter: "(uid={username})",
  userScope: "subtree",
  usernameAttr: "uid",
  displayNameAttr: "cn",
  emailAttr: "mail",
  autoCreateUser: true,
  adminFilter: "",
  connectionTimeoutSecs: 10,
  searchTimeoutSecs: 15,
  verifyCert: true,
  caCertPem: "",
});

// Test state
const testConnResult = ref<{ ok: boolean; latencyMs: number; error: string | null } | null>(null);
const testSearchResult = ref<any>(null);
const testBindResult = ref<any>(null);
const testUsername = ref("");
const testPassword = ref("");
const testing = ref(false);

async function loadConfig() {
  loading.value = true;
  try {
    const res = await fetch(apiUrl("/api/ldap/config"));
    if (res.ok) config.value = { ...config.value, ...(await res.json()) };
  } catch (e: any) {
    error.value = e?.message;
  } finally {
    loading.value = false;
  }
}

async function saveConfig() {
  saving.value = true;
  error.value = "";
  try {
    const res = await fetch(apiUrl("/api/ldap/config"), {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(config.value),
    });
    if (!res.ok) throw new Error("Failed to save LDAP config");
  } catch (e: any) {
    error.value = e?.message;
  } finally {
    saving.value = false;
  }
}

async function testConnection() {
  testing.value = true;
  testConnResult.value = null;
  try {
    const res = await fetch(apiUrl("/api/ldap/test/connection"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: config.value }),
    });
    testConnResult.value = await res.json();
  } finally {
    testing.value = false;
  }
}

async function testSearch() {
  testing.value = true;
  testSearchResult.value = null;
  try {
    const res = await fetch(apiUrl("/api/ldap/test/search"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: config.value, testUsername: testUsername.value }),
    });
    testSearchResult.value = await res.json();
  } finally {
    testing.value = false;
  }
}

async function testBind() {
  testing.value = true;
  testBindResult.value = null;
  try {
    const res = await fetch(apiUrl("/api/ldap/test/bind"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: config.value, testUsername: testUsername.value, testPassword: testPassword.value }),
    });
    testBindResult.value = await res.json();
  } finally {
    testing.value = false;
  }
}

onMounted(() => loadConfig());
</script>

<template>
  <div class="space-y-6 max-w-3xl">
    <div class="flex items-center gap-2">
      <Wifi class="w-5 h-5 text-primary" />
      <h2 class="text-lg font-semibold">LDAP Settings</h2>
      <span v-if="config.enabled" class="px-2 py-0.5 rounded text-xs bg-green-100 text-green-700">Enabled</span>
      <span v-else class="px-2 py-0.5 rounded text-xs bg-gray-100 text-gray-500">Disabled</span>
    </div>

    <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
    <div v-if="loading" class="flex items-center gap-2 text-muted-foreground"><Loader2 class="w-4 h-4 animate-spin" /> Loading...</div>

    <div v-else class="space-y-6">
      <!-- Connection Settings -->
      <div class="space-y-3 border rounded-lg p-4">
        <h3 class="text-sm font-semibold text-muted-foreground">Connection</h3>
        <label class="flex items-center gap-2 text-sm"> <input type="checkbox" v-model="config.enabled" /> Enable LDAP login </label>
        <div>
          <label class="text-sm font-medium">Server URL *</label>
          <input v-model="config.serverUrl" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="ldaps://ldap.corp.com:636" />
        </div>
        <label class="flex items-center gap-2 text-sm"> <input type="checkbox" v-model="config.useStarttls" /> Use STARTTLS </label>
        <div>
          <label class="text-sm font-medium">Service Account DN *</label>
          <input v-model="config.bindDn" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="uid=dbx-svc,ou=services,dc=corp,dc=com" />
        </div>
        <div>
          <label class="text-sm font-medium">Service Account Password *</label>
          <PasswordInput v-model="config.bindPassword" inputClass="h-10 mt-1" placeholder="********" />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="text-sm font-medium">Connection Timeout (s)</label>
            <input v-model.number="config.connectionTimeoutSecs" type="number" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
          </div>
          <div>
            <label class="text-sm font-medium">Search Timeout (s)</label>
            <input v-model.number="config.searchTimeoutSecs" type="number" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
          </div>
        </div>
        <label class="flex items-center gap-2 text-sm"> <input type="checkbox" v-model="config.verifyCert" /> Verify server certificate </label>
      </div>

      <!-- User Search Settings -->
      <div class="space-y-3 border rounded-lg p-4">
        <h3 class="text-sm font-semibold text-muted-foreground">User Search</h3>
        <div>
          <label class="text-sm font-medium">User Base DN *</label>
          <input v-model="config.userBase" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="ou=users,dc=corp,dc=com" />
        </div>
        <div>
          <label class="text-sm font-medium">Search Filter *</label>
          <input v-model="config.userFilter" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="(uid={username})" />
        </div>
        <div class="grid grid-cols-3 gap-3">
          <div>
            <label class="text-sm font-medium">Username Attr</label>
            <input v-model="config.usernameAttr" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
          </div>
          <div>
            <label class="text-sm font-medium">Display Name Attr</label>
            <input v-model="config.displayNameAttr" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
          </div>
          <div>
            <label class="text-sm font-medium">Email Attr</label>
            <input v-model="config.emailAttr" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
          </div>
        </div>
        <div>
          <label class="text-sm font-medium">Admin Filter</label>
          <input v-model="config.adminFilter" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="(memberOf=cn=dbx-admins,ou=groups,dc=corp,dc=com)" />
        </div>
        <label class="flex items-center gap-2 text-sm"> <input type="checkbox" v-model="config.autoCreateUser" /> Auto-create user on first login </label>
      </div>

      <!-- Test Area -->
      <div class="space-y-4 border rounded-lg p-4">
        <h3 class="text-sm font-semibold text-muted-foreground">Test</h3>

        <!-- Stage 1: Connection -->
        <div class="space-y-2">
          <div class="flex items-center gap-2">
            <Button size="sm" variant="outline" @click="testConnection" :disabled="testing"> <Wifi class="w-4 h-4 mr-1" /> Test Connection </Button>
            <span v-if="testConnResult?.ok" class="flex items-center gap-1 text-green-600 text-sm"> <CheckCircle class="w-4 h-4" /> OK ({{ testConnResult.latencyMs }}ms) </span>
            <span v-else-if="testConnResult && !testConnResult.ok" class="flex items-center gap-1 text-red-500 text-sm"> <XCircle class="w-4 h-4" /> {{ testConnResult.error }} </span>
          </div>
        </div>

        <!-- Stage 2: Search -->
        <div class="space-y-2">
          <div class="flex items-center gap-2">
            <input v-model="testUsername" type="text" placeholder="Test username" class="flex h-9 w-40 rounded-md border border-input bg-background px-3 py-1 text-sm" />
            <Button size="sm" variant="outline" @click="testSearch" :disabled="testing || !testUsername"> <Search class="w-4 h-4 mr-1" /> Search </Button>
          </div>
          <span v-if="testSearchResult?.ok" class="text-green-600 text-sm">
            Found: {{ testSearchResult.details?.userDn }} ({{ testSearchResult.details?.displayName }})
            <span v-if="testSearchResult.details?.isAdminByFilter" class="ml-2 text-amber-600">Admin</span>
          </span>
          <span v-else-if="testSearchResult && !testSearchResult.ok" class="text-red-500 text-sm">{{ testSearchResult.error }}</span>
        </div>

        <!-- Stage 3: Bind -->
        <div class="space-y-2">
          <div class="flex items-center gap-2">
            <input v-model="testPassword" type="password" placeholder="Test password" class="flex h-9 w-40 rounded-md border border-input bg-background px-3 py-1 text-sm" />
            <Button size="sm" variant="outline" @click="testBind" :disabled="testing || !testUsername || !testPassword"> <LogIn class="w-4 h-4 mr-1" /> Login Test </Button>
          </div>
          <span v-if="testBindResult?.ok" class="text-green-600 text-sm">Bind successful — user can log in</span>
          <span v-else-if="testBindResult && !testBindResult.ok" class="text-red-500 text-sm">{{ testBindResult.error }}</span>
        </div>
      </div>

      <div class="flex justify-end gap-2">
        <Button variant="outline" @click="loadConfig">Cancel</Button>
        <Button @click="saveConfig" :disabled="saving"> <Loader2 v-if="saving" class="w-4 h-4 animate-spin mr-1" /> Save Settings </Button>
      </div>
    </div>
  </div>
</template>
