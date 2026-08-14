<script setup lang="ts">
import { ref } from "vue";
import { apiUrl } from "@/lib/common/webPath";
import { Button } from "@/components/ui/button";
import PasswordInput from "@/components/ui/PasswordInput.vue";
import { Download, Upload, Loader2, CheckCircle } from "@lucide/vue";

const exportPassphrase = ref("");
const importPassphrase = ref("");
const importMode = ref("merge");
const importFile = ref<File | null>(null);
const loading = ref(false);
const result = ref<string>("");
const error = ref("");

const exportOptions = ref({
  connections: true,
  saved_sql: true,
  prompt_templates: true,
  history: true,
  ai_conversations: true,
  settings: true,
});

async function handleExport() {
  if (!exportPassphrase.value) {
    error.value = "Passphrase is required for export";
    return;
  }
  loading.value = true;
  error.value = "";
  result.value = "";
  try {
    const include = Object.entries(exportOptions.value)
      .filter(([, v]) => v)
      .map(([k]) => k);
    const res = await fetch(apiUrl("/api/backup/export"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ passphrase: exportPassphrase.value, include }),
    });
    if (!res.ok) throw new Error("Export failed");
    const blob = await res.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `dbx-backup-${new Date().toISOString().slice(0, 10)}.zip`;
    a.click();
    URL.revokeObjectURL(url);
    result.value = "Export completed successfully";
  } catch (e: any) {
    error.value = e?.message || "Export failed";
  } finally {
    loading.value = false;
  }
}

async function handleImport() {
  if (!importFile.value || !importPassphrase.value) {
    error.value = "File and passphrase are required";
    return;
  }
  loading.value = true;
  error.value = "";
  result.value = "";
  try {
    const formData = new FormData();
    formData.append("file", importFile.value);
    formData.append("passphrase", importPassphrase.value);
    formData.append("mode", importMode.value);
    const res = await fetch(apiUrl("/api/backup/import"), {
      method: "POST",
      body: formData,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(text || "Import failed");
    }
    const summary = await res.json();
    result.value = `Import completed: ${summary.connections} connections, ${summary.savedSqlFiles} snippets, ${summary.promptTemplates} templates, ${summary.historyEntries} history entries`;
  } catch (e: any) {
    error.value = e?.message || "Import failed";
  } finally {
    loading.value = false;
  }
}

function onFileChange(e: Event) {
  const target = e.target as HTMLInputElement;
  if (target.files && target.files[0]) importFile.value = target.files[0];
}
</script>

<template>
  <div class="space-y-6 max-w-2xl">
    <!-- Export -->
    <div class="space-y-3 border rounded-lg p-4">
      <div class="flex items-center gap-2">
        <Download class="w-5 h-5 text-primary" />
        <h3 class="text-sm font-semibold">Export My Data</h3>
      </div>
      <div class="space-y-2">
        <label class="text-sm font-medium">Select data to export:</label>
        <div class="grid grid-cols-2 gap-2 text-sm">
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.connections" /> Connections</label>
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.saved_sql" /> Snippets</label>
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.prompt_templates" /> Prompt Templates</label>
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.history" /> History</label>
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.ai_conversations" /> AI Conversations</label>
          <label class="flex items-center gap-2"><input type="checkbox" v-model="exportOptions.settings" /> Settings</label>
        </div>
      </div>
      <div>
        <label class="text-sm font-medium">Backup Passphrase:</label>
        <PasswordInput v-model="exportPassphrase" inputClass="h-10 mt-1" placeholder="Encrypts your connection passwords" />
      </div>
      <Button @click="handleExport" :disabled="loading || !exportPassphrase"> <Loader2 v-if="loading" class="w-4 h-4 animate-spin mr-1" /> Export </Button>
    </div>

    <!-- Import -->
    <div class="space-y-3 border rounded-lg p-4">
      <div class="flex items-center gap-2">
        <Upload class="w-5 h-5 text-primary" />
        <h3 class="text-sm font-semibold">Import Data</h3>
      </div>
      <div>
        <label class="text-sm font-medium">Backup file:</label>
        <input type="file" @change="onFileChange" accept=".zip" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" />
      </div>
      <div>
        <label class="text-sm font-medium">Passphrase:</label>
        <PasswordInput v-model="importPassphrase" inputClass="h-10 mt-1" placeholder="Enter backup passphrase" />
      </div>
      <div>
        <label class="text-sm font-medium">Mode:</label>
        <select v-model="importMode" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1">
          <option value="merge">Merge (keep existing, add new)</option>
          <option value="replace">Replace (clear current, import all)</option>
        </select>
      </div>
      <Button @click="handleImport" :disabled="loading || !importFile || !importPassphrase"> <Loader2 v-if="loading" class="w-4 h-4 animate-spin mr-1" /> Import </Button>
    </div>

    <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
    <p v-if="result" class="flex items-center gap-1 text-sm text-green-600"><CheckCircle class="w-4 h-4" /> {{ result }}</p>
  </div>
</template>
