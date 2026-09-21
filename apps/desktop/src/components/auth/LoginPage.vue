<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";
import PasswordInput from "@/components/ui/PasswordInput.vue";
import { Lock, Loader2, ShieldCheck, User } from "@lucide/vue";
import AppLogo from "@/components/icons/AppLogo.vue";
import { apiUrl } from "@/lib/common/webPath";
import { formatBuildLabel } from "@/lib/app/buildLabel";
import { translateBackendError } from "@/i18n/backend-errors";

const props = withDefaults(
  defineProps<{
    setupMode?: boolean;
  }>(),
  { setupMode: false },
);

const emit = defineEmits<{ authenticated: [user?: AuthUserInfo] }>();
const { t } = useI18n();

export interface AuthUserInfo {
  id: string;
  username: string;
  displayName: string;
  isAdmin: boolean;
  authSource: string;
}

const username = ref("");
const password = ref("");
const confirmPassword = ref("");
const error = ref("");
const loading = ref(false);
const buildLabel = ref("");

onMounted(async () => {
  // Pre-fill default admin username in setup mode
  if (props.setupMode) {
    username.value = "admin";
  }
  // Show which build this server serves, so an upgrade can be confirmed from the
  // sign-in screen without logging in.
  try {
    const res = await fetch(apiUrl("/api/auth/check"));
    if (res.ok) {
      const data = await res.json();
      buildLabel.value = formatBuildLabel(data);
    }
  } catch {
    /* server unreachable — keep the label empty */
  }
});

async function readAuthError(res: Response): Promise<string> {
  const text = (await res.text()).trim();
  if (!text) return t("auth.loginFailed");
  let message = text;
  try {
    const parsed = JSON.parse(text);
    if (parsed && typeof parsed.error === "string") message = parsed.error;
  } catch {
    // not JSON — fall through with the raw body
  }
  return translateBackendError(t, message) || t("auth.loginFailed");
}

async function submit() {
  if (props.setupMode && password.value !== confirmPassword.value) {
    error.value = t("auth.passwordMismatch");
    return;
  }

  loading.value = true;
  error.value = "";
  try {
    const url = apiUrl(props.setupMode ? "/api/auth/setup" : "/api/auth/login");
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        username: username.value || undefined,
        password: password.value,
      }),
    });
    if (res.ok) {
      const data = await res.json().catch(() => ({}));
      emit("authenticated", data.user);
    } else {
      error.value = await readAuthError(res);
    }
  } catch (e: any) {
    error.value = e?.message || t("auth.connectFailed");
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="flex items-center justify-center h-screen bg-gradient-to-br from-background via-background to-blue-950/20">
    <div class="w-[360px] space-y-8">
      <div class="flex flex-col items-center gap-4">
        <AppLogo class="w-20 h-20 rounded-2xl shadow-lg shadow-blue-500/20" />
        <div class="text-center">
          <h1 class="text-2xl font-bold tracking-tight">DBX</h1>
          <p class="text-sm text-muted-foreground mt-1">
            {{ setupMode ? t("auth.setupDescription") : t("auth.loginDescription") }}
          </p>
        </div>
      </div>

      <form class="space-y-4" @submit.prevent="submit" autocomplete="off">
        <div v-if="setupMode" class="flex items-center justify-center gap-2 text-sm text-muted-foreground">
          <ShieldCheck class="w-4 h-4" />
          <span>{{ t("auth.setupTitle") }}</span>
        </div>
        <div class="relative">
          <User class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <input
            v-model="username"
            type="text"
            :placeholder="setupMode ? t('auth.newUsername') : t('auth.username')"
            class="flex h-11 w-full rounded-md border border-input bg-background pl-10 pr-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
            autocomplete="off"
            autofocus
          />
        </div>
        <div class="relative">
          <Lock class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <PasswordInput v-model="password" :placeholder="setupMode ? t('auth.newPassword') : t('auth.enterPassword')" inputClass="pl-10 h-11" autocomplete="off" />
        </div>
        <div v-if="setupMode" class="relative">
          <Lock class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
          <PasswordInput v-model="confirmPassword" :placeholder="t('auth.confirmPassword')" inputClass="pl-10 h-11" autocomplete="off" />
        </div>
        <p v-if="error" class="text-sm text-destructive text-center">{{ error }}</p>
        <Button type="submit" class="w-full h-11 text-sm font-medium" :disabled="loading || !password || (setupMode && !confirmPassword) || (!setupMode && !username)">
          <Loader2 v-if="loading" class="w-4 h-4 animate-spin mr-2" />
          {{ loading ? t("auth.processing") : setupMode ? t("auth.setPassword") : t("auth.login") }}
        </Button>
      </form>

      <p class="text-center text-xs text-muted-foreground/50">Powered by DBX</p>
      <p v-if="buildLabel" class="text-center font-mono text-[11px] text-muted-foreground/50" data-testid="login-build-label">
        {{ buildLabel }}
      </p>
    </div>
  </div>
</template>
