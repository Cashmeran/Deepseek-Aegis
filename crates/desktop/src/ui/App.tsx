import { useEffect, useState, useCallback, useRef } from "react";
import { useIPC } from "./hooks/useIPC";
import { useAppStore } from "./store/useAppStore";
import type { ClientEvent } from "./types";
import { Sidebar } from "./components/Sidebar";
import MDContent from "./render/markdown";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";

function App() {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const initConfig = useAppStore((s) => s.initConfig);
  useEffect(() => { initConfig(); }, []);

  const handleServerEvent = useAppStore((s) => s.handleServerEvent);
  const { connected, sendEvent } = useIPC(handleServerEvent);

  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const providerConfigs = useAppStore((s) => s.providerConfigs);
  const globalError = useAppStore((s) => s.globalError);
  const setGlobalError = useAppStore((s) => s.setGlobalError);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showNewModal, setShowNewModal] = useState(false);
  const [cwd, setCwd] = useState("");
  const [projectName, setProjectName] = useState("");
  const [prompt, setPrompt] = useState("");

  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const isRunning = activeSession?.status === "running";

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [activeSession?.messages]);

  const handleNewSession = useCallback(() => {
    const cfg = providerConfigs.deepseek;
    if (!cfg.apiKey) { alert("请先在设置中配置 API Key"); return; }
    const name = projectName.trim() || (cwd ? cwd.split(/[\\/]/).pop() || cwd : "新项目");
    sendEvent({ type: "session.start", payload: { title: name, prompt: prompt || "Hello", cwd: cwd || undefined, provider: "deepseek", apiKey: cfg.apiKey, model: cfg.model } });
    setShowNewModal(false);
    setProjectName(""); setPrompt("");
  }, [sendEvent, projectName, prompt, cwd, providerConfigs]);

  const handleContinue = useCallback(() => {
    if (!prompt.trim() || !activeSessionId || isRunning) return;
    sendEvent({ type: "session.continue", payload: { sessionId: activeSessionId, prompt: prompt.trim() } });
    setPrompt("");
  }, [sendEvent, prompt, activeSessionId, isRunning]);

  const handleDeleteSession = useCallback((id: string) => {
    sendEvent({ type: "session.delete", payload: { sessionId: id } });
  }, [sendEvent]);

  return (
    <div className="flex h-screen bg-background text-foreground">
      <Sidebar connected={connected} collapsed={sidebarCollapsed} onNewSession={() => setShowNewModal(true)}
        onDeleteSession={handleDeleteSession} onToggleCollapse={() => setSidebarCollapsed(!sidebarCollapsed)} />

      <div className="flex flex-1 flex-col min-w-0">
        <ScrollArea className="flex-1 px-8 py-6">
          {globalError && (
            <Card className="mb-4 border-destructive/50 bg-destructive/10">
              <CardContent className="py-3 flex items-center justify-between">
                <p className="text-sm text-destructive">{globalError}</p>
                <Button variant="ghost" size="sm" onClick={() => setGlobalError(null)}>Dismiss</Button>
              </CardContent>
            </Card>
          )}

          {activeSession ? (
            <div className="max-w-4xl mx-auto">
              <div className="mb-8">
                <h1 className="text-xl font-bold tracking-tight">{activeSession.title || activeSession.id}</h1>
                <div className="flex items-center gap-2 mt-1">
                  <Badge variant="secondary" className="text-xs">{activeSession.cwd || '默认目录'}</Badge>
                  <Badge variant={isRunning ? "default" : "outline"} className="text-xs">{activeSession.status}</Badge>
                </div>
              </div>
              <Separator className="mb-8" />

              <div className="space-y-6">
                {activeSession.messages.map((msg: Record<string, unknown>, i: number) => {
                  const msgType = msg.type as string;
                  if (msgType === "user_prompt") return (
                    <div key={i} className="flex justify-end">
                      <div className="max-w-[80%] rounded-2xl rounded-br-md bg-primary px-5 py-3 text-primary-foreground text-sm leading-relaxed">
                        <MDContent text={String(msg.prompt ?? "")} />
                      </div>
                    </div>
                  );
                  if (msgType === "assistant") return (
                    <div key={i} className="flex justify-start">
                      <div className="max-w-[85%] rounded-2xl rounded-bl-md bg-muted px-5 py-3 text-sm leading-relaxed">
                        <MDContent text={String(msg.text ?? "")} />
                      </div>
                    </div>
                  );
                  if (msgType === "thinking") return (
                    <details key={i} className="text-xs text-muted-foreground mx-4">
                      <summary className="cursor-pointer italic py-1 select-none">Thinking...</summary>
                      <div className="mt-2 pl-3 border-l-2 border-border whitespace-pre-wrap opacity-70">{msg.text as string}</div>
                    </details>
                  );
                  if (msgType === "tool_use") return (
                    <div key={i} className="mx-4">
                      <Badge variant={msg.status === "error" ? "destructive" : msg.status === "success" ? "default" : "secondary"} className="gap-1.5">
                        <span className="font-semibold">{msg.name as string}</span>
                        {msg.elapsed_ms ? <span className="opacity-60">{(msg.elapsed_ms as number)}ms</span> : null}
                      </Badge>
                      {msg.output ? (
                        <details className="mt-1.5">
                          <summary className="text-xs text-muted-foreground cursor-pointer">Output</summary>
                          <pre className="mt-1 p-3 rounded-lg bg-muted/50 text-xs whitespace-pre-wrap max-h-48 overflow-auto">{msg.output as string}</pre>
                        </details>
                      ) : null}
                    </div>
                  );
                  if (msgType === "usage") return (
                    <div key={i} className="text-center py-4">
                      <Badge variant="outline" className="text-xs font-normal gap-2">
                        <span>{(msg as Record<string,unknown>).input_tokens as number} in</span>
                        <span>{(msg as Record<string,unknown>).output_tokens as number} out</span>
                        <span>${Number((msg as Record<string,unknown>).cost).toFixed(4)}</span>
                      </Badge>
                    </div>
                  );
                  return null;
                })}
              </div>

              {isRunning && (
                <div className="flex items-center gap-3 ml-4 my-4 text-sm text-muted-foreground">
                  <span className="relative flex h-2.5 w-2.5"><span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-primary opacity-75"/><span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-primary"/></span>
                  Agent is working...
                </div>
              )}
              <div ref={messagesEndRef} />
            </div>
          ) : (
            <div className="flex flex-col items-center justify-center h-full -mt-12">
              <div className="flex h-20 w-20 items-center justify-center rounded-2xl bg-primary/10 text-3xl font-bold text-primary mb-6">ag</div>
              <h1 className="text-3xl font-bold tracking-tight mb-2">Aegis Desktop</h1>
              <p className="text-muted-foreground mb-1">{connected ? 'Backend connected' : 'Backend offline'}</p>
              <p className="text-muted-foreground text-sm mb-8">
                {providerConfigs.deepseek.apiKey ? `Model: ${providerConfigs.deepseek.model}` : 'No API key configured'}
              </p>
              <Button size="lg" onClick={() => setShowNewModal(true)}>+ New Project</Button>
            </div>
          )}
        </ScrollArea>

        {activeSession && (
          <div className="border-t px-8 py-4">
            <div className="flex gap-3 items-end max-w-4xl mx-auto">
              <Textarea
                value={prompt}
                onChange={e => setPrompt(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleContinue(); } }}
                placeholder={isRunning ? "Agent is working..." : "Type a message... (Enter to send, Shift+Enter for new line)"}
                rows={1}
                disabled={isRunning}
                className="min-h-[44px] max-h-[200px] resize-none"
              />
              <Button onClick={handleContinue} disabled={!prompt.trim() || isRunning} size="lg">Send</Button>
            </div>
          </div>
        )}
      </div>

      <Dialog open={showNewModal} onOpenChange={setShowNewModal}>
        <DialogContent>
          <DialogHeader><DialogTitle>New Project</DialogTitle></DialogHeader>
          <div className="space-y-4 pt-2">
            <Input value={projectName} onChange={e => setProjectName(e.target.value)} placeholder="Project name (uses directory name if empty)" />
            <Input value={cwd} onChange={e => setCwd(e.target.value)} placeholder="Working directory (uses current if empty)" />
            <Textarea rows={3} value={prompt} onChange={e => setPrompt(e.target.value)} placeholder="Initial prompt (optional)" />
            <div className="flex justify-end gap-3 pt-2">
              <Button variant="outline" onClick={() => setShowNewModal(false)}>Cancel</Button>
              <Button onClick={handleNewSession}>Create Project</Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}

export default App;
