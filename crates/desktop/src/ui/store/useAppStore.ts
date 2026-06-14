import { create } from 'zustand';
import type { PermissionMode, ProviderConfig, ProviderConfigs, ProviderKind, ServerEvent, SessionStatus, StreamMessage } from "../types";

const PROVIDER_STORAGE_KEY = "aegis.provider-configs";
const PERMISSION_STORAGE_KEY = "aegis.permission-mode";

const DEFAULT_PROVIDER_CONFIGS: ProviderConfigs = {
  deepseek: {
    apiKey: "",
    model: "deepseek-v4-pro",
    baseUrl: ""
  },

};

const loadProviderConfigs = (): ProviderConfigs => {
  if (typeof window === "undefined") return DEFAULT_PROVIDER_CONFIGS;
  try {
    const raw = window.localStorage.getItem(PROVIDER_STORAGE_KEY);
    if (!raw) return DEFAULT_PROVIDER_CONFIGS;
    const parsed = JSON.parse(raw) as Partial<ProviderConfigs>;
    return {
      deepseek: { ...DEFAULT_PROVIDER_CONFIGS.deepseek, ...parsed.deepseek },

    };
  } catch {
    return DEFAULT_PROVIDER_CONFIGS;
  }
};

const persistProviderConfigs = (configs: ProviderConfigs) => {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(PROVIDER_STORAGE_KEY, JSON.stringify(configs));
};

const loadPermissionMode = (): PermissionMode => {
  if (typeof window === "undefined") return "default";
  const stored = window.localStorage.getItem(PERMISSION_STORAGE_KEY);
  if (stored === "default" || stored === "plan" || stored === "yolo" || stored === "chat") return stored;
  return "default";
};

const persistPermissionMode = (mode: PermissionMode) => {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(PERMISSION_STORAGE_KEY, mode);
};

export type PermissionRequest = {
  toolUseId: string;
  toolName: string;
  input: unknown;
};

export type SessionView = {
  id: string;
  title: string;
  status: SessionStatus;
  cwd?: string;
  messages: StreamMessage[];
  permissionRequests: PermissionRequest[];
  lastPrompt?: string;
  createdAt?: number;
  updatedAt?: number;
  hydrated: boolean;
  cachePct?: number;
};

interface AppState {
  sessions: Record<string, SessionView>;
  activeSessionId: string | null;
  prompt: string;
  cwd: string;
  pendingStart: boolean;
  globalError: string | null;
  sessionsLoaded: boolean;
  showStartModal: boolean;
  historyRequested: Set<string>;
  activeProvider: ProviderKind;
  providerConfigs: ProviderConfigs;
  permissionMode: PermissionMode;
  projectMeta: ProjectMeta | null;
  scanResult: ScanResult | null;
  projectRules: RuleFile[];

  setPrompt: (prompt: string) => void;
  setCwd: (cwd: string) => void;
  setPendingStart: (pending: boolean) => void;
  setGlobalError: (error: string | null) => void;
  setShowStartModal: (show: boolean) => void;
  setActiveSessionId: (id: string | null) => void;
  setActiveProvider: (provider: ProviderKind) => void;
  setProviderConfig: (provider: ProviderKind, config: ProviderConfig) => void;
  setPermissionMode: (mode: PermissionMode) => void;
  markHistoryRequested: (sessionId: string) => void;
  resolvePermissionRequest: (sessionId: string, toolUseId: string) => void;
  handleServerEvent: (event: ServerEvent) => void;
  initConfig: () => Promise<void>;
  initProject: (cwd: string) => Promise<ProjectMeta | null>;
  openProject: (cwd: string) => Promise<ProjectMeta | null>;
  scanProject: (cwd: string) => Promise<ScanResult | null>;
  loadProjectRules: (cwd: string) => Promise<RuleFile[]>;
  saveProjectRule: (cwd: string, name: string, content: string) => Promise<void>;
  checkProject: (cwd: string) => Promise<boolean>;
  loadProjectSessions: (cwd: string) => Promise<void>;
}

function createSession(id: string): SessionView {
  return { id, title: "", status: "idle", messages: [], permissionRequests: [], hydrated: false };
}

// ── rAF-based stream accumulation: smooth 60fps rendering, no setTimeout races ──
// Each sessionId+type has its own accumulator. On first token, schedules an rAF
// flush. All tokens arriving within the same frame are batched into one state update.
// rAF naturally syncs with browser paint — no timer drift, no drop-on-done bugs.
type StreamSlot = { text: string; type: string; rAFId: number | null };
const streamSlots: Record<string, StreamSlot> = {};

function slotKey(sessionId: string, type: string): string {
  return `${sessionId}\0${type}`; // \0 can't appear in UUIDs, safe delimiter
}

function streamFlush(key: string) {
  const slot = streamSlots[key];
  if (!slot || !slot.text) return;
  const accumulated = slot.text;
  const type = slot.type;
  const sessionId = key.slice(0, key.indexOf("\0"));
  slot.text = "";
  slot.rAFId = null;
  useAppStore.setState((state) => {
    const s = state.sessions[sessionId] ?? createSession(sessionId);
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.type === type) {
      msgs[msgs.length - 1] = { ...last, text: (last.text || "") + accumulated };
    } else {
      msgs.push({ type, text: accumulated } as StreamMessage);
    }
    return { sessions: { ...state.sessions, [sessionId]: { ...s, messages: msgs, status: "running" } } };
  });
}

function streamPush(sessionId: string, text: string, type: string) {
  const key = slotKey(sessionId, type);
  if (!streamSlots[key]) {
    streamSlots[key] = { text: "", type, rAFId: null };
  }
  const slot = streamSlots[key];
  slot.text += text;
  if (slot.rAFId === null) {
    slot.rAFId = requestAnimationFrame(() => streamFlush(key));
  }
}

function streamFlushAll() {
  for (const key of Object.keys(streamSlots)) {
    const slot = streamSlots[key];
    if (slot?.rAFId !== null) {
      cancelAnimationFrame(slot.rAFId!);
      slot.rAFId = null;
    }
    streamFlush(key);
  }
}

export const useAppStore = create<AppState>((set, get) => ({
  sessions: {},
  activeSessionId: null,
  prompt: "",
  cwd: "",
  pendingStart: false,
  globalError: null,
  sessionsLoaded: false,
  showStartModal: false,
  historyRequested: new Set(),
  activeProvider: "deepseek",
  providerConfigs: loadProviderConfigs(),
  permissionMode: loadPermissionMode(),
  projectMeta: null,
  scanResult: null,
  projectRules: [],

  setPrompt: (prompt) => set({ prompt }),
  setCwd: (cwd) => set({ cwd }),
  setPendingStart: (pendingStart) => set({ pendingStart }),
  setGlobalError: (globalError) => set({ globalError }),
  setShowStartModal: (showStartModal) => set({ showStartModal }),
  setActiveSessionId: (id) => set({ activeSessionId: id }),
  setActiveProvider: (provider) => set({ activeProvider: provider }),
  setProviderConfig: (provider, config) =>
    set((state) => {
      const nextConfigs = { ...state.providerConfigs, [provider]: config };
      persistProviderConfigs(nextConfigs);
      return { providerConfigs: nextConfigs };
    }),
  setPermissionMode: (mode) =>
    set(() => {
      persistPermissionMode(mode);
      return { permissionMode: mode };
    }),

  markHistoryRequested: (sessionId) => {
    set((state) => {
      const next = new Set(state.historyRequested);
      next.add(sessionId);
      return { historyRequested: next };
    });
  },

  resolvePermissionRequest: (sessionId, toolUseId) => {
    set((state) => {
      const existing = state.sessions[sessionId];
      if (!existing) return {};
      return {
        sessions: {
          ...state.sessions,
          [sessionId]: {
            ...existing,
            permissionRequests: existing.permissionRequests.filter(req => req.toolUseId !== toolUseId)
          }
        }
      };
    });
  },

  handleServerEvent: (event) => {
    const state = get();

    switch (event.type) {
      case "session.list": {
        const nextSessions: Record<string, SessionView> = {};
        for (const session of event.payload.sessions) {
          const existing = state.sessions[session.id] ?? createSession(session.id);
          nextSessions[session.id] = {
            ...existing,
            status: session.status,
            title: session.title,
            cwd: session.cwd,
            createdAt: session.createdAt,
            updatedAt: session.updatedAt
          };
        }

        set({ sessions: nextSessions, sessionsLoaded: true });

        const hasSessions = event.payload.sessions.length > 0;
        set({ showStartModal: !hasSessions });

        if (!hasSessions) {
          get().setActiveSessionId(null);
        }

        if (!state.activeSessionId && event.payload.sessions.length > 0) {
          const sorted = [...event.payload.sessions].sort((a, b) => {
            const aTime = a.updatedAt ?? a.createdAt ?? 0;
            const bTime = b.updatedAt ?? b.createdAt ?? 0;
            return aTime - bTime;
          });
          const latestSession = sorted[sorted.length - 1];
          if (latestSession) {
            get().setActiveSessionId(latestSession.id);
          }
        } else if (state.activeSessionId) {
          const stillExists = event.payload.sessions.some(
            (session) => session.id === state.activeSessionId
          );
          if (!stillExists) {
            get().setActiveSessionId(null);
          }
        }
        break;
      }

      case "session.history": {
        const { sessionId, messages, status } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return {
            sessions: {
              ...state.sessions,
              [sessionId]: { ...existing, status, messages, hydrated: true }
            }
          };
        });
        break;
      }

      case "session.status": {
        if (event.payload.status === "completed" || event.payload.status === "idle") {
          streamFlushAll();
        }
        const { sessionId, status, title, cwd } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return {
            sessions: {
              ...state.sessions,
              [sessionId]: {
                ...existing,
                status,
                title: title ?? existing.title,
                cwd: cwd ?? existing.cwd,
                updatedAt: Date.now()
              }
            }
          };
        });

        if (state.pendingStart) {
          get().setActiveSessionId(sessionId);
          set({ pendingStart: false, showStartModal: false });
        }
        break;
      }

      case "session.deleted": {
        const { sessionId } = event.payload;
        const state = get();
        if (!state.sessions[sessionId]) break;
        const nextSessions = { ...state.sessions };
        delete nextSessions[sessionId];
        set({
          sessions: nextSessions,
          showStartModal: Object.keys(nextSessions).length === 0
        });
        if (state.activeSessionId === sessionId) {
          const remaining = Object.values(nextSessions).sort(
            (a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0)
          );
          get().setActiveSessionId(remaining[0]?.id ?? null);
        }
        break;
      }

      case "stream.delta": {
        streamPush(event.payload.sessionId, event.payload.text, "assistant");
        break;
      }

      case "stream.thinking": {
        streamPush(event.payload.sessionId, event.payload.text, "thinking");
        break;
      }

      case "stream.tool_start": {
        const { sessionId, id, name, input } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: [...existing.messages, { type: "tool_use", id, name, input, status: "pending" }] } } };
        });
        break;
      }

      case "stream.tool_progress": {
        const { sessionId, line } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          const msgs = [...existing.messages];
          const last = msgs[msgs.length - 1];
          // Append progress lines to last tool_use output
          if (last && last.type === "tool_use" && last.status === "pending") {
            msgs[msgs.length - 1] = { ...last, output: (last.output || "") + line + "\n" };
          }
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: msgs } } };
        });
        break;
      }

      case "stream.tool_result": {
        const { sessionId, id, name, is_error, output, elapsed_ms } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          const msgs = existing.messages.map(m => {
            if (m.type === "tool_use" && (m as Record<string,unknown>).id === id) {
              return { ...m, status: is_error ? "error" : "success", output, elapsed_ms };
            }
            return m;
          });
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: msgs } } };
        });
        break;
      }

      case "stream.done": {
        streamFlushAll();
        const { sessionId, input_tokens, output_tokens, cache_read_tokens, cost } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          const updatedMessages = [...existing.messages, { type: "usage", input_tokens, output_tokens, cache_read_tokens, cost } as StreamMessage];
          // Cumulative cache rate across all turns
          let totalInput = 0, totalCache = 0;
          for (const m of updatedMessages) {
            if (m.type === "usage") {
              const u = m as Record<string,number>;
              totalInput += u.input_tokens || 0;
              totalCache += u.cache_read_tokens || 0;
            }
          }
          const cachePct = totalInput + totalCache > 0
            ? Math.min(100, Math.round((totalCache / (totalInput + totalCache)) * 100))
            : 0;
          const cwd = existing.cwd;
          if (cwd) {
            window.__TAURI__?.core?.invoke("save_session_messages", {
              cwd, sessionId, messages: updatedMessages,
            }).catch(() => {});
          }
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, status: "completed", messages: updatedMessages, cachePct } } };
        });
        break;
      }

      case "stream.user_prompt": {
        const { sessionId, prompt } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: [...existing.messages, { type: "user_prompt", prompt }] } } };
        });
        break;
      }

      case "permission.request": {
        const { sessionId, toolUseId, toolName, input } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return {
            sessions: {
              ...state.sessions,
              [sessionId]: {
                ...existing,
                permissionRequests: [...existing.permissionRequests, { toolUseId, toolName, input }]
              }
            }
          };
        });
        break;
      }

      case "stream.message": {
        const { sessionId, message } = event.payload;
        if (message.type === "stream_event") break; // ignore raw stream events in history
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: [...existing.messages, message] } } };
        });
        break;
      }

      case "ask_user": {
        const { sessionId, question, header, options } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: [...existing.messages, { type: "ask_user", question, header, options }] } } };
        });
        break;
      }

      case "runner.error": {
        set({ globalError: event.payload.message });
        break;
      }
    }
  },

  initConfig: async () => {
    try {
      if (!window.__TAURI__?.core?.invoke) return;

      // Load project index from disk (~/.aegis/projects.json)
      try {
        const projects = await window.__TAURI__.core.invoke<ProjectEntry[]>("load_projects");
        if (projects?.length) {
          // Restore each project as a session in the sidebar
          for (const p of projects) {
            const sessions = useAppStore.getState().sessions;
            if (!sessions[p.path]) {
              useAppStore.setState({
                sessions: {
                  ...sessions,
                  [p.path]: {
                    id: p.path,
                    title: p.name || p.path.split(/[\\/]/).pop() || p.path,
                    status: "completed" as SessionStatus,
                    cwd: p.path,
                    messages: [],
                    permissionRequests: [],
                    hydrated: false,
                    lastPrompt: undefined,
                    createdAt: p.lastOpened,
                    updatedAt: p.lastOpened,
                  },
                },
              });
            }
          }
        }
      } catch { /* no projects yet */ }

      const config = await window.__TAURI__.core.invoke<{ apiKey: string; model: string }>("get_config");
      if (config?.apiKey) {
        const state = get();
        const current = state.providerConfigs.deepseek;
        set({
          providerConfigs: {
            ...state.providerConfigs,
            deepseek: {
              apiKey: current.apiKey || config.apiKey,
              model: current.model || config.model,
              baseUrl: current.baseUrl,
            },
          },
        });
      }
    } catch {
      // Config not available — user can enter key manually
    }
  },

  // ── Project API (calls Rust backend project.rs) ──

  initProject: async (cwd: string) => {
    try {
      const meta = await window.__TAURI__?.core?.invoke<ProjectMeta>("project_init", { cwd });
      if (meta) {
        set({ projectMeta: meta });
        return meta;
      }
    } catch (e) {
      set({ globalError: `项目初始化失败: ${e}` });
    }
    return null;
  },

  openProject: async (cwd: string) => {
    try {
      const meta = await window.__TAURI__?.core?.invoke<ProjectMeta>("project_open", { cwd });
      if (meta) {
        set({ projectMeta: meta });
        return meta;
      }
    } catch {
      // No .aegis/ directory — use global mode
      set({ projectMeta: null });
    }
    return null;
  },

  scanProject: async (cwd: string) => {
    try {
      const result = await window.__TAURI__?.core?.invoke<ScanResult>("project_scan", { cwd });
      if (result) {
        set({ scanResult: result });
        return result;
      }
    } catch (e) {
      console.error("scanProject failed:", e);
    }
    return null;
  },

  loadProjectRules: async (cwd: string) => {
    try {
      const rules = await window.__TAURI__?.core?.invoke<RuleFile[]>("project_list_rules", { cwd });
      if (rules) {
        set({ projectRules: rules });
        return rules;
      }
    } catch {
      set({ projectRules: [] });
    }
    return [];
  },

  saveProjectRule: async (cwd: string, name: string, content: string) => {
    try {
      await window.__TAURI__?.core?.invoke("project_save_rule", { cwd, name, content });
      const state = get();
      await state.loadProjectRules(cwd);
    } catch (e) {
      console.error("saveProjectRule failed:", e);
    }
  },

  checkProject: async (cwd: string) => {
    try {
      return await window.__TAURI__?.core?.invoke<boolean>("project_check", { cwd });
    } catch {
      return false;
    }
  },

  loadProjectSessions: async (cwd: string) => {
    try {
      const entries = await window.__TAURI__?.core?.invoke<{ session_id: string; completed_at: string; turn_count: number }[]>("load_project_sessions", { cwd });
      if (!entries?.length) return;
      const state = get();
      const existing = state.sessions[cwd] ?? { id: cwd, title: cwd.split(/[\\/]/).pop() || cwd, status: "completed" as SessionStatus, cwd, messages: [], permissionRequests: [], hydrated: false };

      // Load the most recent session's full message history
      const latest = entries[entries.length - 1];
      let messages: StreamMessage[] = [];
      try {
        const sessionData = await window.__TAURI__?.core?.invoke<{ messages: StreamMessage[] }>("read_session_file", { cwd, sessionId: latest.session_id });
        if (sessionData?.messages) {
          messages = sessionData.messages;
        }
      } catch { /* session file may not exist */ }

      // Recalculate cumulative cache hit rate from loaded messages
      let totalInput = 0, totalCache = 0;
      for (const m of messages) {
        if (m.type === "usage") {
          const u = m as Record<string,number>;
          totalInput += u.input_tokens || 0;
          totalCache += u.cache_read_tokens || 0;
        }
      }
      const cachePct = totalInput + totalCache > 0
        ? Math.min(100, Math.round((totalCache / (totalInput + totalCache)) * 100))
        : undefined;

      set({
        sessions: {
          ...state.sessions,
          [cwd]: {
            ...existing,
            hydrated: true,
            updatedAt: Date.now(),
            messages: messages.length > 0 ? messages : existing.messages,
            cachePct,
          },
        },
      });
    } catch { /* no sessions */ }
  },
}));

// Project types (mirrors Rust project.rs)
export type ProjectMeta = {
  name: string;
  root: string;
  language?: string;
  file_count: number;
  has_aegis_dir: boolean;
  created_at: number;
};

export type ScanResult = {
  total_files: number;
  total_functions: number;
  total_modules: number;
  languages: LanguageCount[];
  duration_ms: number;
};

export type LanguageCount = {
  name: string;
  files: number;
  functions: number;
};

export type RuleFile = {
  name: string;
  content: string;
};

type ProjectEntry = {
  path: string;
  name: string;
  lastOpened: number;
  sessionCount: number;
};
