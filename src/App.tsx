import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { 
  Folder, 
  Send, 
  Square, 
  ShieldAlert, 
  CheckCircle2, 
  XCircle, 
  RefreshCw, 
  ChevronDown, 
  ChevronRight, 
  Copy, 
  Check, 
  Cpu,
  Sparkles,
  AlertTriangle,
  Sliders
} from 'lucide-react';
import { BackendType, SystemStatus, PermissionRequest, MessageItem, LocalStore, LocalSession } from './types';
import { Sidebar } from './Sidebar';
import logoApp from '../design/logo-app-1024.png';

function emptyStore(): LocalStore {
  return {
    version: 1,
    projects: [],
    sessions: [],
    activeProjectId: null,
    activeSessionId: null,
  };
}

function permissionCanAllowOnce(req: PermissionRequest): boolean {
  if (req.alreadyDenied) return false;
  return (req.options ?? []).some(
    (o) => o.kind === 'allow_once' || o.optionId === 'allow-once'
  );
}

export default function App() {
  const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
  const [loadingStatus, setLoadingStatus] = useState(true);
  const [selectedBackend, setSelectedBackend] = useState<BackendType>('grok');
  const [workspace, setWorkspace] = useState('');
  const [selectedModel, setSelectedModel] = useState('');
  const [selectedReasoningEffort, setSelectedReasoningEffort] = useState('');
  const [prompt, setPrompt] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [messages, setMessages] = useState<MessageItem[]>([]);
  const [currentThoughts, setCurrentThoughts] = useState('');
  const [currentResponse, setCurrentResponse] = useState('');
  const [showThoughts, setShowThoughts] = useState(true);
  const [copiedCmd, setCopiedCmd] = useState<string | null>(null);
  const [pendingPermission, setPendingPermission] = useState<PermissionRequest | null>(null);
  const [launchHint, setLaunchHint] = useState<string | null>(null);
  const [store, setStore] = useState<LocalStore>(emptyStore());

  const messagesEndRef = useRef<HTMLDivElement>(null);
  const storeRef = useRef(store);
  const messagesRef = useRef(messages);
  const currentResponseRef = useRef(currentResponse);
  const currentThoughtsRef = useRef(currentThoughts);
  const isRunningRef = useRef(isRunning);
  const runLocalSessionIdRef = useRef<string | null>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages, currentResponse, currentThoughts, pendingPermission]);

  useEffect(() => {
    storeRef.current = store;
  }, [store]);
  useEffect(() => {
    messagesRef.current = messages;
  }, [messages]);
  useEffect(() => {
    currentResponseRef.current = currentResponse;
  }, [currentResponse]);
  useEffect(() => {
    currentThoughtsRef.current = currentThoughts;
  }, [currentThoughts]);
  useEffect(() => {
    isRunningRef.current = isRunning;
  }, [isRunning]);

  const persistStore = async (next: LocalStore): Promise<LocalStore> => {
    const saved = await invoke<LocalStore>('save_local_store', { store: next });
    storeRef.current = saved;
    setStore(saved);
    return saved;
  };

  const patchSession = async (
    sessionId: string,
    patch: Partial<LocalSession>
  ): Promise<LocalStore> => {
    const current = storeRef.current;
    const now = Date.now();
    const next: LocalStore = {
      ...current,
      sessions: current.sessions.map((s) =>
        s.id === sessionId ? { ...s, ...patch, updatedAt: now } : s
      ),
    };
    return persistStore(next);
  };

  const applySessionToUi = (session: LocalSession | undefined, workspacePath: string) => {
    setWorkspace(workspacePath);
    setCurrentResponse('');
    setCurrentThoughts('');
    setPendingPermission(null);
    if (session) {
      messagesRef.current = session.messages;
      setMessages(session.messages);
      setSelectedBackend(session.backend);
      if (session.model) setSelectedModel(session.model);
      if (session.reasoningEffort) setSelectedReasoningEffort(session.reasoningEffort);
    } else {
      messagesRef.current = [];
      setMessages([]);
    }
  };

  const persistPartialRun = async () => {
    const runningId = runLocalSessionIdRef.current;
    const streamed = currentResponseRef.current;
    const thoughts = currentThoughtsRef.current;
    runLocalSessionIdRef.current = null;
    setIsRunning(false);
    setPendingPermission(null);
    if (!runningId || !streamed) {
      setCurrentResponse('');
      setCurrentThoughts('');
      return;
    }
    const assistant: MessageItem = {
      id: String(Date.now()),
      role: 'assistant',
      content: streamed,
      thoughts: thoughts || undefined,
      createdAt: Date.now(),
    };
    const base =
      storeRef.current.sessions.find((s) => s.id === runningId)?.messages ?? messagesRef.current;
    const nextMessages = [...base, assistant];
    await patchSession(runningId, { messages: nextMessages });
    if (storeRef.current.activeSessionId === runningId) {
      messagesRef.current = nextMessages;
      setMessages(nextMessages);
    }
    setCurrentResponse('');
    setCurrentThoughts('');
  };

  const stopIfRunning = async () => {
    try {
      await invoke('stop_session');
    } catch {
      // no active CLI session
    }
    if (isRunningRef.current || runLocalSessionIdRef.current) {
      await persistPartialRun();
    }
  };

  // Probe CLI status on mount
  const checkStatus = async () => {
    setLoadingStatus(true);
    try {
      const res = await invoke<SystemStatus>('probe_status');
      setSystemStatus(res);
    } catch (err) {
      console.error('Failed to probe status:', err);
    } finally {
      setLoadingStatus(false);
    }
  };

  useEffect(() => {
    checkStatus();
  }, []);

  useEffect(() => {
    const load = async () => {
      try {
        const loaded = await invoke<LocalStore>('load_local_store');
        storeRef.current = loaded;
        setStore(loaded);
        const project = loaded.projects.find((p) => p.id === loaded.activeProjectId);
        const session = loaded.sessions.find((s) => s.id === loaded.activeSessionId);
        applySessionToUi(session, project?.workspacePath ?? '');
      } catch (err) {
        console.error('Failed to load local store:', err);
      }
    };
    void load();
  }, []);

  // Sync model & reasoning effort whenever status or backend changes
  useEffect(() => {
    if (!systemStatus) return;
    const currentCli = systemStatus[selectedBackend];
    if (currentCli) {
      if (currentCli.supportsModel && currentCli.models.length > 0) {
        if (!currentCli.models.includes(selectedModel)) {
          setSelectedModel(currentCli.defaultModel || currentCli.models[0]);
        }
      } else {
        setSelectedModel('');
      }

      if (currentCli.supportsReasoningEffort && currentCli.reasoningEfforts.length > 0) {
        if (!currentCli.reasoningEfforts.includes(selectedReasoningEffort)) {
          setSelectedReasoningEffort(currentCli.defaultReasoningEffort || currentCli.reasoningEfforts[0]);
        }
      } else {
        setSelectedReasoningEffort('');
      }
    }
  }, [systemStatus, selectedBackend]);

  // Listen for Tauri events from Rust backend
  useEffect(() => {
    let unlistenUpdates: (() => void) | undefined;
    let unlistenPermissions: (() => void) | undefined;
    let unlistenSessionEnd: (() => void) | undefined;

    const setupListeners = async () => {
      // Stream updates
      unlistenUpdates = await listen<{
        sessionId: string;
        kind: 'text_delta' | 'thought_delta' | 'tool_call' | 'error';
        text?: string;
        toolName?: string;
        title?: string;
      }>('session_update', (event) => {
        const payload = event.payload;
        if (payload.kind === 'text_delta' && payload.text) {
          setCurrentResponse((prev) => prev + payload.text);
        } else if (payload.kind === 'thought_delta' && payload.text) {
          setCurrentThoughts((prev) => prev + payload.text);
        } else if (payload.kind === 'error' && payload.text) {
          setMessages((prev) => [
            ...prev,
            {
              id: String(Date.now()),
              role: 'system',
              content: `[错误] ${payload.text}`,
              createdAt: Date.now(),
            },
          ]);
        }
      });

      // Permission requests
      unlistenPermissions = await listen<PermissionRequest>('permission_request', (event) => {
        setPendingPermission(event.payload);
      });

      // Session ended
      unlistenSessionEnd = await listen<{ sessionId: string; status: string; fullResponse?: string }>(
        'session_end',
        (event) => {
          const localId = runLocalSessionIdRef.current;
          runLocalSessionIdRef.current = null;
          setIsRunning(false);
          const finalResp = event.payload.fullResponse || currentResponseRef.current;
          const thoughts = currentThoughtsRef.current || undefined;
          const assistant: MessageItem = {
            id: String(Date.now()),
            role: 'assistant',
            content: finalResp,
            thoughts,
            createdAt: Date.now(),
          };
          if (localId) {
            const base =
              storeRef.current.sessions.find((s) => s.id === localId)?.messages ??
              messagesRef.current;
            const nextMessages = [...base, assistant];
            void patchSession(localId, { messages: nextMessages });
            if (storeRef.current.activeSessionId === localId) {
              messagesRef.current = nextMessages;
              setMessages(nextMessages);
            }
          }
          setCurrentResponse('');
          setCurrentThoughts('');
          setPendingPermission(null);
        }
      );
    };

    setupListeners();

    return () => {
      unlistenUpdates?.();
      unlistenPermissions?.();
      unlistenSessionEnd?.();
    };
  }, []);

  // Copy helper
  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCmd(text);
    setTimeout(() => setCopiedCmd(null), 2000);
  };

  // Switch backend (resets active CLI session; keeps the local chat)
  const handleBackendChange = async (newBackend: BackendType) => {
    if (newBackend === selectedBackend) return;
    await stopIfRunning();
    setSelectedBackend(newBackend);
    setCurrentResponse('');
    setCurrentThoughts('');
    setPendingPermission(null);
    const sid = storeRef.current.activeSessionId;
    if (sid) {
      void patchSession(sid, { backend: newBackend });
    }
  };

  const pickDirectory = async (defaultPath?: string): Promise<string | null> => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择项目文件夹',
        defaultPath: defaultPath || (workspace.trim() ? workspace : undefined),
      });
      if (typeof selected === 'string' && selected.trim()) {
        return selected;
      }
    } catch (err: unknown) {
      setLaunchHint(err instanceof Error ? err.message : String(err));
    }
    return null;
  };

  const handlePickWorkspace = async () => {
    const selected = await pickDirectory(workspace.trim() ? workspace : undefined);
    if (!selected) return;
    setWorkspace(selected);
    const current = storeRef.current;
    if (current.activeProjectId) {
      const next: LocalStore = {
        ...current,
        projects: current.projects.map((p) =>
          p.id === current.activeProjectId ? { ...p, workspacePath: selected } : p
        ),
      };
      try {
        await persistStore(next);
      } catch (err: unknown) {
        setLaunchHint(err instanceof Error ? err.message : String(err));
      }
    }
  };

  const handleNewProject = async (name: string, workspacePath: string) => {
    await stopIfRunning();
    const next = await invoke<LocalStore>('create_local_project', {
      name,
      workspace: workspacePath,
    });
    storeRef.current = next;
    setStore(next);
    applySessionToUi(undefined, workspacePath);
  };

  const handleNewSession = async () => {
    if (!storeRef.current.activeProjectId) return;
    await stopIfRunning();
    const next = await invoke<LocalStore>('create_local_session', {
      projectId: storeRef.current.activeProjectId,
      backend: selectedBackend,
      model: selectedModel,
      reasoningEffort: selectedReasoningEffort,
    });
    storeRef.current = next;
    setStore(next);
    const session = next.sessions.find((s) => s.id === next.activeSessionId);
    const project = next.projects.find((p) => p.id === next.activeProjectId);
    applySessionToUi(session, project?.workspacePath ?? '');
  };

  const handleSelectProject = async (projectId: string) => {
    await stopIfRunning();
    const current = storeRef.current;
    const project = current.projects.find((p) => p.id === projectId);
    if (!project) return;
    const sessions = current.sessions
      .filter((s) => s.projectId === projectId)
      .sort((a, b) => b.updatedAt - a.updatedAt);
    const keep =
      current.activeSessionId && sessions.some((s) => s.id === current.activeSessionId)
        ? current.activeSessionId
        : sessions[0]?.id ?? null;
    const next: LocalStore = {
      ...current,
      activeProjectId: projectId,
      activeSessionId: keep,
    };
    await persistStore(next);
    applySessionToUi(
      sessions.find((s) => s.id === keep),
      project.workspacePath
    );
  };

  const handleSelectSession = async (sessionId: string) => {
    if (sessionId === storeRef.current.activeSessionId && !isRunningRef.current) return;
    await stopIfRunning();
    const current = storeRef.current;
    const session = current.sessions.find((s) => s.id === sessionId);
    if (!session) return;
    const project = current.projects.find((p) => p.id === session.projectId);
    if (!project) return;
    const next: LocalStore = {
      ...current,
      activeProjectId: session.projectId,
      activeSessionId: sessionId,
    };
    await persistStore(next);
    applySessionToUi(session, project.workspacePath);
  };

  const handleRenameSession = async (sessionId: string, title: string) => {
    const current = storeRef.current;
    const next: LocalStore = {
      ...current,
      sessions: current.sessions.map((s) =>
        s.id === sessionId ? { ...s, title, titleCustom: true } : s
      ),
    };
    await persistStore(next);
  };

  const persistSessionSettings = (patch: Partial<LocalSession>) => {
    const sid = storeRef.current.activeSessionId;
    if (sid) void patchSession(sid, patch);
  };

  // Send a task
  const handleSend = async () => {
    if (!prompt.trim() || isRunning) return;

    const currentCli = systemStatus?.[selectedBackend];
    if (!currentCli?.installed || !currentCli?.loggedIn) {
      alert(`当前后端 ${selectedBackend === 'grok' ? 'Grok' : 'Antigravity'} 未安装或未登录，请先在终端完成配置。`);
      return;
    }

    if (!storeRef.current.activeProjectId || !workspace.trim()) {
      setMessages((prev) => [
        ...prev,
        {
          id: String(Date.now()),
          role: 'system',
          content: '[启动失败] 请先选择项目和工作区',
          createdAt: Date.now(),
        },
      ]);
      return;
    }

    let local = storeRef.current;
    let localSessionId = local.activeSessionId;
    const activeSession = local.sessions.find((s) => s.id === localSessionId);
    if (!localSessionId || activeSession?.projectId !== local.activeProjectId) {
      try {
        local = await invoke<LocalStore>('create_local_session', {
          projectId: local.activeProjectId,
          backend: selectedBackend,
          model: selectedModel,
          reasoningEffort: selectedReasoningEffort,
        });
        storeRef.current = local;
        setStore(local);
        localSessionId = local.activeSessionId;
      } catch (err: unknown) {
        setMessages((prev) => [
          ...prev,
          {
            id: String(Date.now()),
            role: 'system',
            content: `[启动失败] ${err instanceof Error ? err.message : String(err)}`,
            createdAt: Date.now(),
          },
        ]);
        return;
      }
    }
    if (!localSessionId) return;

    const userPrompt = prompt.trim();
    setPrompt('');
    setIsRunning(true);
    setCurrentResponse('');
    setCurrentThoughts('');
    setPendingPermission(null);

    const userMsg: MessageItem = {
      id: String(Date.now()),
      role: 'user',
      content: userPrompt,
      createdAt: Date.now(),
    };
    const nextMessages = [...messagesRef.current, userMsg];
    messagesRef.current = nextMessages;
    setMessages(nextMessages);
    runLocalSessionIdRef.current = localSessionId;
    await patchSession(localSessionId, {
      messages: nextMessages,
      backend: selectedBackend,
      model: selectedModel,
      reasoningEffort: selectedReasoningEffort,
    });

    try {
      const sessId = await invoke<string>('start_task', {
        backend: selectedBackend,
        workspace,
        prompt: userPrompt,
        model: selectedModel.trim() ? selectedModel.trim() : null,
        reasoningEffort: (currentCli?.supportsReasoningEffort && selectedReasoningEffort.trim()) ? selectedReasoningEffort.trim() : null,
      });
      console.log("Session started:", sessId);
    } catch (err: any) {
      runLocalSessionIdRef.current = null;
      setIsRunning(false);
      const failMsg: MessageItem = {
        id: String(Date.now()),
        role: 'system',
        content: `[启动失败] ${err?.message || String(err)}`,
        createdAt: Date.now(),
      };
      const failedMessages = [...messagesRef.current, failMsg];
      messagesRef.current = failedMessages;
      setMessages(failedMessages);
      void patchSession(localSessionId, { messages: failedMessages });
    }
  };

  // Abort running task
  const handleStop = async () => {
    try {
      await invoke('stop_session');
    } catch (err) {
      console.error('Stop error:', err);
    }
    await persistPartialRun();
  };

  // Respond to permission request
  const handlePermissionDecision = async (allow: boolean) => {
    if (!pendingPermission) return;

    // agy cards are already-denied: close only, never invoke allow.
    if (allow && !permissionCanAllowOnce(pendingPermission)) {
      setPendingPermission(null);
      return;
    }

    // Only single-shot decisions. Never match allow_always / *-session
    // (Grok lists allow-edits-session first; includes('allow') would auto-pick it).
    let optionId = allow ? 'allow-once' : 'reject-once';
    if (pendingPermission.options?.length > 0) {
      const match = pendingPermission.options.find((o) =>
        allow
          ? o.kind === 'allow_once' || o.optionId === 'allow-once'
          : o.kind === 'reject_once' ||
            o.optionId === 'reject-once' ||
            o.kind === 'dismiss' ||
            o.optionId === 'dismiss'
      );
      if (match) optionId = match.optionId;
    }

    try {
      await invoke('respond_permission', {
        requestId: pendingPermission.id,
        optionId,
      });
    } catch (err) {
      console.error('Permission respond error:', err);
    }
    setPendingPermission(null);
  };

  const handleOpenDesktop = async (appId: 'kayg' | 'antigravity') => {
    setLaunchHint(null);
    try {
      const msg = await invoke<string>('open_desktop_app', {
        appId,
        workspace,
      });
      setLaunchHint(msg);
    } catch (err: unknown) {
      setLaunchHint(err instanceof Error ? err.message : String(err));
    }
  };

  const activeCli = systemStatus?.[selectedBackend];
  const isBackendReady = activeCli?.installed && activeCli?.loggedIn;
  const activeProject = store.projects.find((p) => p.id === store.activeProjectId);
  const canSend = Boolean(store.activeProjectId && workspace.trim());

  return (
    <div className="flex h-screen bg-neutral-950 text-neutral-100 select-none overflow-hidden font-sans">
      <Sidebar
        store={store}
        disabled={false}
        onNewProject={handleNewProject}
        onNewSession={() => void handleNewSession()}
        onSelectProject={(id) => void handleSelectProject(id)}
        onSelectSession={(id) => void handleSelectSession(id)}
        onRenameSession={(id, title) => void handleRenameSession(id, title)}
        onPickDirectory={pickDirectory}
      />

      <div className="flex flex-col flex-1 min-w-0">
      {/* 1. Top Header 标题栏 */}
      <header className="h-14 border-b border-neutral-800/80 bg-neutral-900/70 backdrop-blur px-4 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 font-semibold text-sm tracking-tight text-neutral-200">
            <img
              src={logoApp}
              alt="PX Agent"
              className="w-8 h-8 object-contain drop-shadow-sm"
            />
            <span>PX Agent GUI</span>
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-neutral-800 text-neutral-400 font-mono">v0.1</span>
          </div>

          {/* 后端切换 Tabs */}
          <div className="flex bg-neutral-800/80 p-0.5 rounded-lg border border-neutral-700/50 ml-3">
            <button
              onClick={() => handleBackendChange('grok')}
              className={`flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium transition ${
                selectedBackend === 'grok'
                  ? 'bg-blue-600 text-white shadow-sm'
                  : 'text-neutral-400 hover:text-neutral-200'
              }`}
            >
              <Sparkles className="w-3.5 h-3.5" />
              <span>Grok</span>
            </button>
            <button
              onClick={() => handleBackendChange('agy')}
              className={`flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-medium transition ${
                selectedBackend === 'agy'
                  ? 'bg-blue-600 text-white shadow-sm'
                  : 'text-neutral-400 hover:text-neutral-200'
              }`}
            >
              <Cpu className="w-3.5 h-3.5" />
              <span>Antigravity</span>
            </button>
          </div>
        </div>

        {/* 后端状态指示与刷新 */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 text-xs">
            <span className="text-neutral-500">后端状态:</span>
            {loadingStatus ? (
              <span className="text-neutral-400 flex items-center gap-1">
                <RefreshCw className="w-3 h-3 animate-spin" /> 探测中...
              </span>
            ) : isBackendReady ? (
              <span className="text-emerald-400 flex items-center gap-1 font-medium">
                <CheckCircle2 className="w-3.5 h-3.5" /> 已就绪
                {selectedModel && (
                  <span className="text-neutral-500 font-normal">({selectedModel})</span>
                )}
              </span>
            ) : (
              <span className="text-amber-400 flex items-center gap-1 font-medium">
                <XCircle className="w-3.5 h-3.5" /> 未就绪
              </span>
            )}
          </div>

          <button
            onClick={checkStatus}
            title="刷新 CLI 探测状态"
            className="p-1.5 hover:bg-neutral-800 rounded-md text-neutral-400 hover:text-neutral-200 transition"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loadingStatus ? 'animate-spin' : ''}`} />
          </button>
        </div>
      </header>

      {/* 2. Control Deck 控制区：Logo主标、启动器、工作区、模型与推理强度 */}
      <section className="border-b border-neutral-800/80 bg-neutral-900/30 shrink-0">
        {/* Logo 窗口主标与外部应用启动器 */}
        <div className="px-4 py-3 border-b border-neutral-800/50 flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            {/* 窗口主标 */}
            <img 
              src={logoApp} 
              alt="PX Agent Logo" 
              className="w-10 h-10 object-contain rounded-xl shadow-md border border-neutral-800/80 shrink-0" 
            />
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h1 className="text-xs font-semibold text-neutral-200">薄客户端控制台</h1>
                <span className="text-[11px] text-neutral-400 truncate">
                  日常 Grok 用 KayG（审计），日常 Gemini 用官方 Antigravity（实现）
                </span>
              </div>
              <p className="text-[11px] text-neutral-400 leading-tight mt-0.5">
                本窗仅配置工作区驱动官方 CLI，不替代官方桌面工具。
              </p>
            </div>
          </div>

          {/* 启动器按钮 */}
          <div className="flex items-center gap-2 shrink-0">
            {systemStatus?.kayg?.installed ? (
              <button
                onClick={() => handleOpenDesktop('kayg')}
                className="px-2.5 py-1.5 rounded-lg bg-blue-600/90 hover:bg-blue-500 text-white text-xs font-medium transition flex items-center gap-1.5 shadow-sm"
              >
                <Sparkles className="w-3.5 h-3.5" />
                <span>打开 KayG</span>
              </button>
            ) : !loadingStatus ? (
              <span className="text-[11px] text-neutral-400">未装 KayG</span>
            ) : null}

            {systemStatus?.antigravity?.installed ? (
              <button
                onClick={() => handleOpenDesktop('antigravity')}
                className="px-2.5 py-1.5 rounded-lg bg-emerald-700/90 hover:bg-emerald-600 text-white text-xs font-medium transition flex items-center gap-1.5 shadow-sm"
              >
                <Cpu className="w-3.5 h-3.5" />
                <span>打开 Antigravity</span>
              </button>
            ) : !loadingStatus ? (
              <span className="text-[11px] text-neutral-400">未装 Antigravity</span>
            ) : null}
          </div>
        </div>

        {launchHint && (
          <div className="px-4 py-1.5 bg-neutral-900/60 border-b border-neutral-800/40 text-[11px] text-neutral-300">
            {launchHint}
          </div>
        )}

        {/* 工作区、模型选择器、推理强度控制行 */}
        <div className="px-4 py-2.5 flex flex-wrap items-center gap-4 text-xs">
          {/* 工作区 (cwd) */}
          <div className="flex-1 min-w-[240px] flex items-center gap-2 min-h-[28px]">
            <Folder className="w-4 h-4 text-neutral-400 shrink-0" />
            <span className="text-neutral-400 shrink-0 font-medium">工作区:</span>
            <button
              type="button"
              onClick={handlePickWorkspace}
              disabled={isRunning}
              className="shrink-0 px-2.5 py-1 rounded bg-neutral-800 hover:bg-neutral-700 border border-neutral-700 text-neutral-200 font-medium transition disabled:opacity-50"
            >
              选择文件夹
            </button>
            <span
              className={`flex-1 min-w-0 truncate font-mono text-xs ${
                workspace ? 'text-neutral-200' : 'text-neutral-500'
              }`}
              title={workspace || undefined}
            >
              {workspace || '未选择'}
            </span>
          </div>

          {/* 模型选择器（探测失败禁用并标明「未探测」） */}
          <div className="flex items-center gap-2 shrink-0">
            <Cpu className="w-3.5 h-3.5 text-neutral-400" />
            <span className="text-neutral-400 font-medium">模型:</span>
            {activeCli?.supportsModel && activeCli.models.length > 0 ? (
              <select
                value={selectedModel}
                onChange={(e) => {
                  const value = e.target.value;
                  setSelectedModel(value);
                  persistSessionSettings({ model: value });
                }}
                disabled={isRunning}
                className="bg-neutral-950 border border-neutral-800 rounded px-2.5 py-1 text-neutral-200 text-xs font-mono focus:outline-none focus:border-blue-500 transition disabled:opacity-50 cursor-pointer"
              >
                {activeCli.models.map((m) => (
                  <option key={m} value={m}>
                    {m} {m === activeCli.defaultModel ? '(默认)' : ''}
                  </option>
                ))}
              </select>
            ) : (
              <select
                disabled
                className="bg-neutral-950/50 border border-neutral-800/60 rounded px-2.5 py-1 text-neutral-400 text-xs font-mono opacity-60 cursor-not-allowed"
              >
                <option value="">未探测</option>
              </select>
            )}
          </div>

          {/* 推理强度选择器：有能力才显示，无能力则隐藏，不许做假滑条 */}
          {activeCli?.supportsReasoningEffort && activeCli.reasoningEfforts.length > 0 ? (
            <div className="flex items-center gap-2 shrink-0">
              <Sliders className="w-3.5 h-3.5 text-neutral-400" />
              <span className="text-neutral-400 font-medium">推理强度:</span>
              <select
                value={selectedReasoningEffort}
                onChange={(e) => {
                  const value = e.target.value;
                  setSelectedReasoningEffort(value);
                  persistSessionSettings({ reasoningEffort: value });
                }}
                disabled={isRunning}
                className="bg-neutral-950 border border-neutral-800 rounded px-2.5 py-1 text-neutral-200 text-xs font-mono focus:outline-none focus:border-blue-500 transition disabled:opacity-50 cursor-pointer"
              >
                {activeCli.reasoningEfforts.map((effort) => (
                  <option key={effort} value={effort}>
                    {effort === 'high' ? 'high (高 / 默认)' :
                     effort === 'medium' ? 'medium (中)' :
                     effort === 'low' ? 'low (低)' :
                     effort === 'xhigh' ? 'xhigh (超高)' : effort}
                  </option>
                ))}
              </select>
            </div>
          ) : null}

          {isRunning && (
            <span className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-blue-500/10 text-blue-400 border border-blue-500/20 shrink-0 ml-auto">
              <span className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-pulse"></span>
              执行中
            </span>
          )}
        </div>
      </section>

      {/* Main Content Area */}
      <div className="flex-1 overflow-y-auto p-4 flex flex-col gap-4">
        {/* If CLI is missing or not logged in, display official onboarding card */}
        {!isBackendReady && !loadingStatus && (
          <div className="border border-amber-500/30 bg-amber-950/20 rounded-xl p-4 text-xs space-y-3">
            <div className="flex items-start gap-2.5">
              <AlertTriangle className="w-4 h-4 text-amber-400 shrink-0 mt-0.5" />
              <div>
                <h3 className="font-semibold text-amber-200 text-sm">
                  {selectedBackend === 'grok' ? 'Grok CLI' : 'Antigravity CLI'} 尚未就绪
                </h3>
                <p className="text-neutral-400 mt-1 leading-relaxed">
                  本客户端直接驱动本机官方 CLI。登录认证与额度均保留在官方终端中，请在终端执行以下官方指令后点击右上角刷新：
                </p>
              </div>
            </div>

            {/* Install command */}
            {!activeCli?.installed && (
              <div className="space-y-1">
                <div className="text-neutral-400 font-medium">1. 安装官方 CLI:</div>
                <div className="flex items-center justify-between bg-neutral-900 border border-neutral-800 rounded-lg p-2 font-mono text-neutral-300">
                  <code>{activeCli?.installCommand}</code>
                  <button
                    onClick={() => copyToClipboard(activeCli?.installCommand || '')}
                    className="p-1 hover:bg-neutral-800 rounded text-neutral-400 hover:text-neutral-200 transition"
                  >
                    {copiedCmd === activeCli?.installCommand ? (
                      <Check className="w-3.5 h-3.5 text-emerald-400" />
                    ) : (
                      <Copy className="w-3.5 h-3.5" />
                    )}
                  </button>
                </div>
              </div>
            )}

            {/* Login command */}
            {(!activeCli?.loggedIn || !activeCli?.installed) && (
              <div className="space-y-1">
                <div className="text-neutral-400 font-medium">2. 官方登录认证:</div>
                <div className="flex items-center justify-between bg-neutral-900 border border-neutral-800 rounded-lg p-2 font-mono text-neutral-300">
                  <code>{activeCli?.loginCommand}</code>
                  <button
                    onClick={() => copyToClipboard(activeCli?.loginCommand || '')}
                    className="p-1 hover:bg-neutral-800 rounded text-neutral-400 hover:text-neutral-200 transition"
                  >
                    {copiedCmd === activeCli?.loginCommand ? (
                      <Check className="w-3.5 h-3.5 text-emerald-400" />
                    ) : (
                      <Copy className="w-3.5 h-3.5" />
                    )}
                  </button>
                </div>
              </div>
            )}
          </div>
        )}

        {messages.length === 0 && !isRunning && !pendingPermission && (
          <div className="flex-1 flex flex-col items-center justify-center min-h-[240px] text-center px-6">
            <img
              src={logoApp}
              alt="PX Agent"
              className="w-28 h-28 object-contain drop-shadow-lg mb-4"
            />
            <p className="text-sm text-neutral-300 font-medium">PX Agent GUI</p>
            <p className="text-xs text-neutral-500 mt-1.5 max-w-sm leading-relaxed">
              {!store.activeProjectId
                ? '请先在左侧新建项目（名称 + 本机文件夹），再发送任务'
                : workspace
                ? '工作区已就绪，输入任务开始会话'
                : '请先选择工作区文件夹，再发送任务或打开 KayG / Antigravity'}
            </p>
          </div>
        )}

        {/* Message history */}
        {messages.map((msg) => (
          <div
            key={msg.id}
            className={`flex flex-col ${msg.role === 'user' ? 'items-end' : 'items-start'}`}
          >
            <div className="flex items-center gap-1.5 mb-1 px-1 text-[11px] text-neutral-500 font-medium">
              {msg.role === 'user' ? '你' : selectedBackend === 'grok' ? 'Grok 4.6' : 'Antigravity'}
            </div>

            <div
              className={`max-w-[85%] rounded-xl px-4 py-3 text-sm leading-relaxed ${
                msg.role === 'user'
                  ? 'bg-blue-600 text-white'
                  : msg.role === 'system'
                  ? 'bg-red-950/40 border border-red-800/40 text-red-200'
                  : 'bg-neutral-900 border border-neutral-800/80 text-neutral-200'
              }`}
            >
              {/* Optional thoughts accordion */}
              {msg.thoughts && (
                <div className="mb-3 pb-2 border-b border-neutral-800 text-xs text-neutral-400 font-mono">
                  <div className="text-neutral-500 font-semibold mb-1">思考过程:</div>
                  <div className="whitespace-pre-wrap">{msg.thoughts}</div>
                </div>
              )}
              <div className="whitespace-pre-wrap font-sans selection:bg-blue-700">{msg.content}</div>
            </div>
          </div>
        ))}

        {/* Live streaming bubble while running */}
        {isRunning && (
          <div className="flex flex-col items-start">
            <div className="flex items-center gap-1.5 mb-1 px-1 text-[11px] text-neutral-500 font-medium">
              {selectedBackend === 'grok' ? 'Grok (流式输出中...)' : 'Antigravity (流式输出中...)'}
            </div>

            <div className="max-w-[85%] rounded-xl px-4 py-3 text-sm leading-relaxed bg-neutral-900 border border-neutral-800/80 text-neutral-200 space-y-3">
              {/* Live thoughts stream */}
              {currentThoughts && (
                <div className="rounded-lg bg-neutral-950/60 border border-neutral-800 p-2.5 text-xs text-neutral-400 font-mono">
                  <button
                    onClick={() => setShowThoughts(!showThoughts)}
                    className="flex items-center gap-1 text-neutral-500 hover:text-neutral-300 font-semibold mb-1 w-full text-left"
                  >
                    {showThoughts ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />}
                    <span>模型思考过程 ({currentThoughts.length} 字符)</span>
                  </button>
                  {showThoughts && (
                    <div className="whitespace-pre-wrap max-h-40 overflow-y-auto text-neutral-400">
                      {currentThoughts}
                    </div>
                  )}
                </div>
              )}

              {/* Live text stream */}
              <div className="whitespace-pre-wrap font-sans">
                {currentResponse || (
                  <span className="text-neutral-500 italic flex items-center gap-2">
                    <span className="w-2 h-2 rounded-full bg-blue-500 animate-ping"></span>
                    正在等待模型响应...
                  </span>
                )}
              </div>
            </div>
          </div>
        )}

        {/* Permission Request Card (High-visibility modal/card) */}
        {pendingPermission && (
          <div className="my-3 border-2 border-amber-500/80 bg-neutral-900 shadow-2xl rounded-xl p-4 text-xs space-y-3 max-w-xl mx-auto animate-in fade-in zoom-in-95">
            <div className="flex items-start gap-2.5">
              <ShieldAlert className="w-5 h-5 text-amber-400 shrink-0 mt-0.5" />
              <div>
                <h4 className="font-bold text-amber-200 text-sm">
                  {permissionCanAllowOnce(pendingPermission)
                    ? 'CLI 权限执行请求'
                    : 'CLI 已拒绝该操作'}
                </h4>
                <p className="text-neutral-300 mt-1 font-medium">
                  {pendingPermission.title || `操作工具: ${pendingPermission.toolName}`}
                </p>
                {pendingPermission.command && (
                  <div className="mt-2 p-2 bg-neutral-950 rounded border border-neutral-800 font-mono text-neutral-200 break-all">
                    {pendingPermission.command}
                  </div>
                )}
                <p className="text-neutral-500 mt-1 text-[11px]">
                  {permissionCanAllowOnce(pendingPermission)
                    ? '安全约束：权限默认不放行。点击拒绝将终止此操作，点击允许仅单次放行。'
                    : 'agy headless 已拒绝，本窗口无法放行。关闭即可。日常实现请打开官方 Antigravity 桌面。'}
                </p>
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-neutral-800">
              {permissionCanAllowOnce(pendingPermission) ? (
                <>
                  <button
                    onClick={() => handlePermissionDecision(false)}
                    className="px-4 py-1.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 font-semibold transition flex items-center gap-1.5"
                  >
                    <XCircle className="w-3.5 h-3.5 text-red-400" />
                    <span>拒绝 (默认)</span>
                  </button>
                  <button
                    onClick={() => handlePermissionDecision(true)}
                    className="px-4 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-semibold transition flex items-center gap-1.5 shadow-md shadow-emerald-950"
                  >
                    <Check className="w-3.5 h-3.5" />
                    <span>允许执行</span>
                  </button>
                </>
              ) : (
                <button
                  onClick={() => handlePermissionDecision(false)}
                  className="px-4 py-1.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-200 font-semibold transition flex items-center gap-1.5"
                >
                  <XCircle className="w-3.5 h-3.5 text-neutral-400" />
                  <span>关闭</span>
                </button>
              )}
            </div>
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Bottom Prompt Bar */}
      <footer className="p-4 border-t border-neutral-800/80 bg-neutral-900/40 backdrop-blur shrink-0">
        <div className="flex items-end gap-2 max-w-5xl mx-auto">
          <div className="flex-1 bg-neutral-900 border border-neutral-700/70 focus-within:border-blue-500 rounded-xl px-3 py-2 transition shadow-inner">
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  handleSend();
                }
              }}
              disabled={isRunning || !isBackendReady}
              rows={2}
              placeholder={
                !isBackendReady
                  ? '请先根据上方指引登录并就绪官方 CLI...'
                  : !store.activeProjectId
                  ? '请先新建或选择项目...'
                  : !workspace.trim()
                  ? '请先选择工作区文件夹...'
                  : `输入任务发给 ${selectedBackend === 'grok' ? 'Grok' : 'Antigravity'}... (Enter 发送, Shift+Enter 换行)`
              }
              className="w-full bg-transparent text-neutral-100 text-sm focus:outline-none resize-none placeholder:text-neutral-500 disabled:opacity-50"
            />
          </div>

          {isRunning ? (
            <button
              onClick={handleStop}
              className="h-11 px-4 rounded-xl bg-red-600 hover:bg-red-500 text-white text-xs font-semibold flex items-center gap-1.5 transition shadow-sm shrink-0"
            >
              <Square className="w-3.5 h-3.5 fill-current" />
              <span>停止</span>
            </button>
          ) : (
            <button
              onClick={handleSend}
              disabled={!prompt.trim() || !isBackendReady || !canSend}
              className="h-11 px-4 rounded-xl bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 disabled:text-neutral-500 text-white text-xs font-semibold flex items-center gap-1.5 transition shadow-sm disabled:cursor-not-allowed shrink-0"
            >
              <Send className="w-3.5 h-3.5" />
              <span>发送任务</span>
            </button>
          )}
        </div>
        <div className="max-w-5xl mx-auto mt-2 flex items-center gap-4 text-[11px] text-neutral-500 min-w-0">
          <span className="shrink-0">项目: {activeProject?.name || '未选择'}</span>
          <span className="truncate font-mono" title={workspace || undefined}>
            本地: {workspace || '未选择'}
          </span>
        </div>
      </footer>
      </div>
    </div>
  );
}
