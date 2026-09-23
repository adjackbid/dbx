import { safeLocalStorageGet, safeLocalStorageRemove, safeLocalStorageSet } from "@/lib/backend/safeStorage";

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

function transactionToPromise(transaction: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () => reject(transaction.error ?? new Error("IndexedDB transaction aborted"));
    transaction.onerror = () => reject(transaction.error ?? new Error("IndexedDB transaction failed"));
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
    const transaction = db.transaction(STORE_NAME, mode);
    const request = requestToPromise(run(transaction.objectStore(STORE_NAME)));
    if (mode === "readwrite") {
      const [result] = await Promise.all([request, transactionToPromise(transaction)]);
      return result;
    }
    return await request;
  } catch (error) {
    // Never fully silent: a persistent failure here degrades every save to the
    // localStorage fallback while loads keep preferring the stale IndexedDB
    // snapshot — exactly the workspace-freeze regression this module guards
    // against elsewhere.
    console.warn("[DBX][browserAppStateStorage] IndexedDB operation failed; falling back to localStorage", error);
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

/**
 * Values handed to this module frequently come straight out of reactive stores
 * (open tabs carry reactive editor selection/viewport objects, editor settings
 * are store objects). IndexedDB's structured clone rejects Vue reactive
 * proxies with a DataCloneError, which used to silently degrade every save to
 * the localStorage fallback while loadBrowserAppState kept preferring the
 * stale IndexedDB snapshot — freezing workspace persistence in web mode after
 * the first cursor move. JSON round-tripping yields the exact plain
 * representation the fallback already stores (proxies unwrapped, undefined
 * fields dropped), keeping both copies equivalent.
 */
function toStorableValue(value: unknown): unknown {
  return JSON.parse(JSON.stringify(value));
}

export async function saveBrowserAppState(key: string, value: unknown): Promise<void> {
  // Every account keeps its own browser state; `currentUserScope` prefixes the
  // key with the signed-in user so accounts never overwrite each other.
  const scopedKey = currentUserScope(key);
  const stored = toStorableValue(value);
  const result = await withStore("readwrite", (store) => store.put(stored, scopedKey));
  if (result !== null) {
    // Drop any fallback copy left behind by an earlier failed save: it is now
    // older than the IndexedDB value and must never be resurrected by a future
    // load that cannot reach IndexedDB.
    safeLocalStorageRemove(fallbackKey(scopedKey));
    return;
  }
  if (safeLocalStorageSet(fallbackKey(scopedKey), JSON.stringify(stored))) return;
  throw new Error(`Failed to persist browser app state: ${key}`);
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
