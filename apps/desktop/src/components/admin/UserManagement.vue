<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useUserStore, type UserInfo } from "@/stores/userStore";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import PasswordInput from "@/components/ui/PasswordInput.vue";
import { Shield, UserPlus, Key, Trash2, Loader2, Ban, CheckCircle } from "@lucide/vue";

const { t } = useI18n();
const userStore = useUserStore();

const users = ref<UserInfo[]>([]);
const loading = ref(false);
const error = ref("");

// Create user dialog
const showCreateDialog = ref(false);
const newUser = ref({ username: "", displayName: "", password: "", isAdmin: false, isLdap: false });
const createError = ref("");

// Reset password dialog
const showResetDialog = ref(false);
const resetTarget = ref<UserInfo | null>(null);
const resetPassword = ref("");
const resetError = ref("");

async function loadUsers() {
  loading.value = true;
  error.value = "";
  try {
    users.value = await userStore.listUsers();
  } catch (e: any) {
    error.value = e?.message || "Failed to load users";
  } finally {
    loading.value = false;
  }
}

async function handleCreateUser() {
  createError.value = "";
  try {
    await userStore.createUser({
      username: newUser.value.username,
      password: newUser.value.isLdap ? "" : newUser.value.password,
      displayName: newUser.value.displayName || undefined,
      isAdmin: newUser.value.isAdmin,
      isLdap: newUser.value.isLdap,
    } as any);
    showCreateDialog.value = false;
    newUser.value = { username: "", displayName: "", password: "", isAdmin: false };
    await loadUsers();
  } catch (e: any) {
    createError.value = e?.message || "Failed to create user";
  }
}

async function handleToggleAdmin(user: UserInfo) {
  try {
    await userStore.updateUser(user.id, { isAdmin: !user.isAdmin });
    await loadUsers();
  } catch (e: any) {
    error.value = e?.message || "Failed to update user";
  }
}

async function handleToggleActive(user: UserInfo) {
  try {
    await userStore.updateUser(user.id, { isActive: !user.isActive });
    await loadUsers();
  } catch (e: any) {
    error.value = e?.message || "Failed to update user";
  }
}

async function handleDeleteUser(user: UserInfo) {
  if (!confirm(`Delete user "${user.username}"? This will remove all their data.`)) return;
  try {
    await userStore.deleteUser(user.id);
    await loadUsers();
  } catch (e: any) {
    error.value = e?.message || "Failed to delete user";
  }
}

function openResetDialog(user: UserInfo) {
  resetTarget.value = user;
  resetPassword.value = "";
  resetError.value = "";
  showResetDialog.value = true;
}

async function handleResetPassword() {
  if (!resetTarget.value) return;
  resetError.value = "";
  try {
    await userStore.resetUserPassword(resetTarget.value.id, resetPassword.value);
    showResetDialog.value = false;
  } catch (e: any) {
    resetError.value = e?.message || "Failed to reset password";
  }
}

onMounted(() => {
  loadUsers();
});
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Shield class="w-5 h-5 text-primary" />
        <h2 class="text-lg font-semibold">User Management</h2>
      </div>
      <Button size="sm" @click="showCreateDialog = true">
        <UserPlus class="w-4 h-4 mr-1" />
        {{ t("common.add") }}
      </Button>
    </div>

    <p v-if="error" class="text-sm text-destructive">{{ error }}</p>

    <div v-if="loading" class="flex items-center justify-center py-8">
      <Loader2 class="w-6 h-6 animate-spin text-muted-foreground" />
    </div>

    <div v-else-if="users.length > 0" class="rounded-md border">
      <table class="w-full text-sm">
        <thead class="bg-muted/50">
          <tr>
            <th class="text-left p-3 font-medium">Username</th>
            <th class="text-left p-3 font-medium">Display Name</th>
            <th class="text-left p-3 font-medium">Source</th>
            <th class="text-left p-3 font-medium">Role</th>
            <th class="text-left p-3 font-medium">Status</th>
            <th class="text-right p-3 font-medium">Actions</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="user in users" :key="user.id" class="border-t">
            <td class="p-3 font-medium">{{ user.username }}</td>
            <td class="p-3">{{ user.displayName }}</td>
            <td class="p-3">
              <span class="inline-flex items-center px-2 py-0.5 rounded text-xs" :class="user.authSource === 'ldap' ? 'bg-blue-100 text-blue-700' : 'bg-gray-100 text-gray-700'">
                {{ user.authSource }}
              </span>
            </td>
            <td class="p-3">
              <span class="inline-flex items-center px-2 py-0.5 rounded text-xs" :class="user.isAdmin ? 'bg-amber-100 text-amber-700' : 'bg-gray-100 text-gray-600'">
                {{ user.isAdmin ? "Admin" : "User" }}
              </span>
            </td>
            <td class="p-3">
              <span v-if="user.isActive" class="inline-flex items-center gap-1 text-green-600"> <CheckCircle class="w-3.5 h-3.5" /> Active </span>
              <span v-else class="inline-flex items-center gap-1 text-red-500"> <Ban class="w-3.5 h-3.5" /> Disabled </span>
            </td>
            <td class="p-3">
              <div class="flex items-center justify-end gap-1">
                <Button v-if="user.authSource === 'local'" size="sm" variant="ghost" @click="openResetDialog(user)" title="Reset password">
                  <Key class="w-4 h-4" />
                </Button>
                <Button size="sm" variant="ghost" @click="handleToggleAdmin(user)" :title="user.isAdmin ? 'Demote to user' : 'Promote to admin'">
                  <Shield class="w-4 h-4" />
                </Button>
                <Button size="sm" variant="ghost" @click="handleToggleActive(user)" :title="user.isActive ? 'Disable' : 'Enable'">
                  <component :is="user.isActive ? Ban : CheckCircle" class="w-4 h-4" />
                </Button>
                <Button size="sm" variant="ghost" class="text-destructive" @click="handleDeleteUser(user)" title="Delete">
                  <Trash2 class="w-4 h-4" />
                </Button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <p v-else class="text-sm text-muted-foreground text-center py-8">No users found.</p>

    <!-- Create User Dialog -->
    <Dialog :open="showCreateDialog" @update:open="showCreateDialog = $event">
      <DialogContent class="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>Add User</DialogTitle>
        </DialogHeader>
        <div class="space-y-3">
          <div>
            <label class="text-sm font-medium">Username</label>
            <input v-model="newUser.username" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="username" />
          </div>
          <div>
            <label class="text-sm font-medium">Display Name</label>
            <input v-model="newUser.displayName" type="text" class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm mt-1" placeholder="Display name (optional)" />
          </div>
          <div>
            <label class="text-sm font-medium">Password</label>
            <PasswordInput v-if="!newUser.isLdap" v-model="newUser.password" placeholder="At least 8 characters" inputClass="h-10 mt-1" />
            <p v-else class="text-sm text-muted-foreground mt-1">LDAP users authenticate via LDAP server — no local password needed.</p>
          </div>
          <div class="flex items-center gap-4 text-sm">
            <label class="flex items-center gap-2">
              <input type="checkbox" v-model="newUser.isAdmin" />
              <span>Admin user</span>
            </label>
            <label class="flex items-center gap-2">
              <input type="checkbox" v-model="newUser.isLdap" />
              <span>LDAP authentication</span>
            </label>
          </div>
          <p v-if="createError" class="text-sm text-destructive">{{ createError }}</p>
        </div>
        <DialogFooter>
          <Button variant="outline" @click="showCreateDialog = false">Cancel</Button>
          <Button @click="handleCreateUser" :disabled="!newUser.username || (!newUser.isLdap && (!newUser.password || newUser.password.length < 8))"> Create </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <!-- Reset Password Dialog -->
    <Dialog :open="showResetDialog" @update:open="showResetDialog = $event">
      <DialogContent class="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>Reset Password — {{ resetTarget?.username }}</DialogTitle>
        </DialogHeader>
        <div class="space-y-3">
          <div>
            <label class="text-sm font-medium">New Password</label>
            <PasswordInput v-model="resetPassword" placeholder="At least 8 characters" inputClass="h-10 mt-1" />
          </div>
          <p v-if="resetError" class="text-sm text-destructive">{{ resetError }}</p>
        </div>
        <DialogFooter>
          <Button variant="outline" @click="showResetDialog = false">Cancel</Button>
          <Button @click="handleResetPassword" :disabled="resetPassword.length < 8">Reset</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
