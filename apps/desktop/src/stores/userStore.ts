import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { apiUrl } from "@/lib/common/webPath";

export interface AuthUserInfo {
  id: string;
  username: string;
  displayName: string;
  isAdmin: boolean;
  authSource: string;
}

export interface UserInfo {
  id: string;
  username: string;
  displayName: string;
  authSource: string;
  isAdmin: boolean;
  isActive: boolean;
}

export interface CreateUserPayload {
  username: string;
  password: string;
  displayName?: string;
  isAdmin: boolean;
  isLdap?: boolean;
}

export interface UpdateUserPayload {
  displayName?: string;
  isAdmin?: boolean;
  isActive?: boolean;
}

async function readError(res: Response): Promise<string> {
  const text = (await res.text()).trim();
  if (!text) return "Request failed";
  try {
    const parsed = JSON.parse(text);
    if (parsed && typeof parsed.error === "string") return parsed.error;
  } catch {
    // not JSON
  }
  return text;
}

export const useUserStore = defineStore("user", () => {
  const currentUser = ref<AuthUserInfo | null>(null);
  const isAuthenticated = computed(() => currentUser.value !== null);
  const isAdmin = computed(() => currentUser.value?.isAdmin ?? false);

  async function checkAuth(): Promise<{ authenticated: boolean; required: boolean; setupRequired: boolean; user?: AuthUserInfo }> {
    try {
      const res = await fetch(apiUrl("/api/auth/check"));
      if (!res.ok) return { authenticated: false, required: true, setupRequired: false };
      const data = await res.json();
      currentUser.value = data.user ?? null;
      return {
        authenticated: data.authenticated,
        required: data.required,
        setupRequired: data.setup_required,
        user: data.user,
      };
    } catch {
      return { authenticated: false, required: true, setupRequired: false };
    }
  }

  async function login(username: string, password: string): Promise<AuthUserInfo> {
    const res = await fetch(apiUrl("/api/auth/login"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!res.ok) {
      throw new Error(await readError(res));
    }
    const data = await res.json();
    currentUser.value = data.user ?? null;
    return data.user;
  }

  async function setup(username: string, password: string): Promise<AuthUserInfo> {
    const res = await fetch(apiUrl("/api/auth/setup"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!res.ok) {
      throw new Error(await readError(res));
    }
    const data = await res.json();
    currentUser.value = data.user ?? null;
    return data.user;
  }

  async function logout(): Promise<void> {
    await fetch(apiUrl("/api/auth/logout"), { method: "POST" });
    currentUser.value = null;
  }

  async function changePassword(oldPassword: string, newPassword: string): Promise<void> {
    const res = await fetch(apiUrl("/api/auth/change-password"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ old_password: oldPassword, new_password: newPassword }),
    });
    if (!res.ok) {
      throw new Error(await readError(res));
    }
  }

  // ===== User Management (Admin only) =====

  async function listUsers(): Promise<UserInfo[]> {
    const res = await fetch(apiUrl("/api/users"));
    if (!res.ok) throw new Error(await readError(res));
    return res.json();
  }

  async function createUser(payload: CreateUserPayload): Promise<UserInfo> {
    const res = await fetch(apiUrl("/api/users"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) throw new Error(await readError(res));
    return res.json();
  }

  async function updateUser(id: string, payload: UpdateUserPayload): Promise<void> {
    const res = await fetch(apiUrl(`/api/users/${id}`), {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) throw new Error(await readError(res));
  }

  async function deleteUser(id: string): Promise<void> {
    const res = await fetch(apiUrl(`/api/users/${id}`), { method: "DELETE" });
    if (!res.ok) throw new Error(await readError(res));
  }

  async function resetUserPassword(id: string, newPassword: string): Promise<void> {
    const res = await fetch(apiUrl(`/api/users/${id}/reset-password`), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ new_password: newPassword }),
    });
    if (!res.ok) throw new Error(await readError(res));
  }

  return {
    currentUser,
    isAuthenticated,
    isAdmin,
    checkAuth,
    login,
    setup,
    logout,
    changePassword,
    listUsers,
    createUser,
    updateUser,
    deleteUser,
    resetUserPassword,
  };
});
