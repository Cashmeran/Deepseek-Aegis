import { create } from 'zustand';
import type { PermissionMode, ProviderConfig, ProviderConfigs, ProviderKind, ServerEvent, SessionStatus, StreamMessage } from "../types";

const PROVIDER_STORAGE_KEY = "open-cowork.provider-configs";
const PERMISSION_STORAGE_KEY = "open-cowork.permission-mode";

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
  if (typeof window === "undefined") return "ask";
  const stored = window.localStorage.getItem(PERMISSION_STORAGE_KEY);
  if (stored === "auto" || stored === "ask") return stored;
  return "ask";
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
}

function createSession(id: string): SessionView {
  return { id, title: "", status: "idle", messages: [], permissionRequests: [], hydrated: false };
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
        const { sessionId, text } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          const msgs = [...existing.messages];
          const last = msgs[msgs.length - 1];
          // Append to last assistant message if it exists
          if (last && last.type === "assistant") {
            msgs[msgs.length - 1] = { ...last, text: (last.text || "") + text };
          } else {
            msgs.push({ type: "assistant", text });
          }
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: msgs, status: "running" } } };
        });
        break;
      }

      case "stream.thinking": {
        const { sessionId, text } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          const msgs = [...existing.messages];
          const last = msgs[msgs.length - 1];
          if (last && last.type === "thinking") {
            msgs[msgs.length - 1] = { ...last, text: (last.text || "") + text };
          } else {
            msgs.push({ type: "thinking", text });
          }
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, messages: msgs } } };
        });
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
        const { sessionId, input_tokens, output_tokens, cost } = event.payload;
        set((state) => {
          const existing = state.sessions[sessionId] ?? createSession(sessionId);
          return { sessions: { ...state.sessions, [sessionId]: { ...existing, status: "completed", messages: [...existing.messages, { type: "usage", input_tokens, output_tokens, cost }] } } };
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

      case "runner.error": {
        set({ globalError: event.payload.message });
        break;
      }
    }
  },

  initConfig: async () => {
    try {
      if (!window.__TAURI__?.core?.invoke) return;
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
}));
