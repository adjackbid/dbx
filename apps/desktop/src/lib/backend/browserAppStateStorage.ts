import { safeLocalStorageGet, safeLocalStorageSet } from "@/lib/backend/safeStorage";

const DB_NAME = "dbx-app-state";
const DB_VERSION = 1;
const STORE_NAME = "state";
const LOCAL_STORAGE_PREFIX = "dbx-app-state:";

// Current user ID for per-user browser state scoping.
// When set, all keys are prefixed with "u_{userId}:" so different users
// on the same browser get isolated settings (AI config, MCP, shortcuts, etc.).
let currentUserId: string | null = null;

export function setCurrentUserId(id: string | null) {
  currentUserId = id;
}

export function getCurrentUserId(): string | null {
  return currentUserId;
}

function currentUserScope(key: string): string {
  return currentUserId ? `u_${currentUserId}:${key}` : key;
}

function indexedDb(): IDBFactory | undefined {
  return typeof globalThis.indexedDB === "undefined" ? undefined : globalThis.indexedDB;
}

function requestToPromise<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("IndexedDB request failed"));
  });
}

let dbPromise: Promise<IDBDatabase | null> | undefined;

function openDb(): Promise<IDBDatabase | null> {
  if (dbPromise) return dbPromise;
  const idb = indexedDb();
  if (!idb) return Promise.resolve(null);

  dbPromise = new Promise((resolve) => {
    const request = idb.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE_NAME)) db.createObjectStore(STORE_NAME);
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => resolve(null);
    request.onblocked = () => resolve(null);
  });
  return dbPromise;
}

function fallbackKey(key: string) {
  return `${LOCAL_STORAGE_PREFIX}${key}`;
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T | null> {
  const db = await openDb();
  if (!db) return null;
  try {
    return await requestToPromise(run(db.transaction(STORE_NAME, mode).objectStore(STORE_NAME)));
  } catch {
    return null;
  }
}

export async function loadBrowserAppState(key: string): Promise<unknown | null> {
  const scopedKey = currentUserScope(key);
  const value = await withStore("readonly", (store) => store.get(scopedKey));
  if (value !== null && value !== undefined) return value;

  const fallback = safeLocalStorageGet(fallbackKey(scopedKey));
  if (!fallback) return null;
  try {
    return JSON.parse(fallback);
  } catch {
    return null;
  }
}

export async function saveBrowserAppState(key: string, value: unknown): Promise<void> {
  const scopedKey = currentUserScope(key);
  const result = await withStore("readwrite", (store) => store.put(value, scopedKey));
  if (result !== null) return;
  safeLocalStorageSet(fallbackKey(scopedKey), JSON.stringify(value));
}

export async function clearAllBrowserAppState(): Promise<void> {
  // Clear IndexedDB store (all users' data)
  const db = await openDb();
  if (db) {
    try {
      const tx = db.transaction(STORE_NAME, "readwrite");
      tx.objectStore(STORE_NAME).clear();
      await new Promise<void>((resolve) => {
        tx.oncomplete = () => resolve();
        tx.onerror = () => resolve();
      });
    } catch {
      // ignore
    }
  }
  // Clear ALL localStorage keys with our prefix (all users)
  for (let i = localStorage.length - 1; i >= 0; i--) {
    const key = localStorage.key(i);
    if (key && key.startsWith(LOCAL_STORAGE_PREFIX)) {
      localStorage.removeItem(key);
    }
  }
  // Also clear known legacy keys
  for (const key of ["dbx-editor-settings", "dbx-desktop-settings"]) {
    localStorage.removeItem(key);
  }
  // Per-user desktop settings keys (`dbx-desktop-settings:u_{id}`)
  for (let i = localStorage.length - 1; i >= 0; i--) {
    const key = localStorage.key(i);
    if (key && key.startsWith("dbx-desktop-settings:")) {
      localStorage.removeItem(key);
    }
  }
}
