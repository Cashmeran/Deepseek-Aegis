export type ProviderKind = "deepseek";
export type PermissionMode = "ask" | "auto";
export type PermissionResult = "allow_once" | "allow_always" | "deny";

export type ProviderConfig = {
  apiKey: string;
  model: string;
  baseUrl?: string;
};

export type ProviderConfigs = {
  deepseek: ProviderConfig;
};

export type StreamMessage = {
  type: string;
  text?: string;
  sessionId?: string;
  name?: string;
  input?: unknown;
  output?: string;
  isError?: boolean;
  elapsedMs?: number;
  tokens?: number;
};

export type SessionStatus = "idle" | "running" | "completed" | "error";

export type SessionInfo = {
  id: string;
  title: string;
  status: SessionStatus;
  cwd?: string;
  createdAt: number;
  updatedAt: number;
};

// Server -> Client events (matches backend events.rs)
export type ServerEvent =
  | { type: "stream.delta"; payload: { sessionId: string; text: string } }
  | { type: "stream.thinking"; payload: { sessionId: string; text: string } }
  | { type: "stream.tool_start"; payload: { sessionId: string; id: string; name: string; input: unknown } }
  | { type: "stream.tool_result"; payload: { sessionId: string; id: string; name: string; is_error: boolean; output: string; elapsed_ms: number } }
  | { type: "stream.tool_progress"; payload: { sessionId: string; line: string } }
  | { type: "stream.user_prompt"; payload: { sessionId: string; prompt: string } }
  | { type: "stream.done"; payload: { sessionId: string; input_tokens: number; output_tokens: number; cache_read_tokens: number; cost: number } }
  | { type: "stream.message"; payload: { sessionId: string; message: StreamMessage } }
  | { type: "session.status"; payload: { sessionId: string; status: SessionStatus; title?: string; cwd?: string; error?: string } }
  | { type: "session.list"; payload: { sessions: SessionInfo[] } }
  | { type: "session.history"; payload: { sessionId: string; status: SessionStatus; messages: StreamMessage[] } }
  | { type: "session.deleted"; payload: { sessionId: string } }
  | { type: "ask_user"; payload: { sessionId: string; question: string; header: string; options: unknown[] } }
  | { type: "runner.error"; payload: { sessionId?: string; message: string } };

// Client -> Server events
export type ClientEvent =
  | {
      type: "session.start";
      payload: {
        title: string;
        prompt: string;
        cwd?: string;
        provider: ProviderKind;
        apiKey: string;
        model: string;
        baseUrl?: string;
        executionMode?: string;
      };
    }
  | { type: "session.continue"; payload: { sessionId: string; prompt: string; messages?: StreamMessage[] } }
  | { type: "session.stop"; payload: { sessionId: string } }
  | { type: "session.delete"; payload: { sessionId: string } }
  | { type: "session.list" }
  | { type: "session.history"; payload: { sessionId: string } }
  | { type: "ask_user.response"; payload: { sessionId: string; answer: string } };
