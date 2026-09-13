import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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
  AlertTriangle
} from 'lucide-react';
import { BackendType, SystemStatus, PermissionRequest, MessageItem } from './types';

export default function App() {
  const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
  const [loadingStatus, setLoadingStatus] = useState(true);
  const [selectedBackend, setSelectedBackend] = useState<BackendType>('grok');
  const [workspace, setWorkspace] = useState('/Users/a0000/Developer/px-agent-gui');
  const [prompt, setPrompt] = useState('');
  const [isRunning, setIsRunning] = useState(false);
  const [messages, setMessages] = useState<MessageItem[]>([]);
  const [currentThoughts, setCurrentThoughts] = useState('');
  const [currentResponse, setCurrentResponse] = useState('');
  const [showThoughts, setShowThoughts] = useState(true);
  const [copiedCmd, setCopiedCmd] = useState<string | null>(null);
  const [pendingPermission, setPendingPermission] = useState<PermissionRequest | null>(null);

  const messagesEndRef = useRef<HTMLDivElement>(null);

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  };

  useEffect(() => {
    scrollToBottom();
  }, [messages, currentResponse, currentThoughts, pendingPermission]);

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
          setIsRunning(false);
          const finalResp = event.payload.fullResponse;
          setMessages((prev) => [
            ...prev,
            {
              id: String(Date.now()),
              role: 'assistant',
              content: finalResp || currentResponse,
              thoughts: currentThoughts || undefined,
              createdAt: Date.now(),
            },
          ]);
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
  }, [currentResponse, currentThoughts]);

  // Copy helper
  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedCmd(text);
    setTimeout(() => setCopiedCmd(null), 2000);
  };

  // Switch backend (resets active session)
  const handleBackendChange = async (newBackend: BackendType) => {
    if (newBackend === selectedBackend) return;
    if (isRunning) {
      await invoke('stop_session');
      setIsRunning(false);
    }
    setSelectedBackend(newBackend);
    setCurrentResponse('');
    setCurrentThoughts('');
    setPendingPermission(null);
  };

  // Send a task
  const handleSend = async () => {
    if (!prompt.trim() || isRunning) return;

    const currentCli = systemStatus?.[selectedBackend];
    if (!currentCli?.installed || !currentCli?.loggedIn) {
      alert(`当前后端 ${selectedBackend === 'grok' ? 'Grok' : 'Antigravity'} 未安装或未登录，请先在终端完成配置。`);
      return;
    }

    const userPrompt = prompt.trim();
    setPrompt('');
    setIsRunning(true);
    setCurrentResponse('');
    setCurrentThoughts('');
    setPendingPermission(null);

    setMessages((prev) => [
      ...prev,
      {
        id: String(Date.now()),
        role: 'user',
        content: userPrompt,
        createdAt: Date.now(),
      },
    ]);

    try {
      const sessId = await invoke<string>('start_task', {
        backend: selectedBackend,
        workspace,
        prompt: userPrompt,
      });
      console.log("Session started:", sessId);
    } catch (err: any) {
      setIsRunning(false);
      setMessages((prev) => [
        ...prev,
        {
          id: String(Date.now()),
          role: 'system',
          content: `[启动失败] ${err?.message || String(err)}`,
          createdAt: Date.now(),
        },
      ]);
    }
  };

  // Abort running task
  const handleStop = async () => {
    try {
      await invoke('stop_session');
    } catch (err) {
      console.error('Stop error:', err);
    }
    setIsRunning(false);
    setPendingPermission(null);
  };

  // Respond to permission request
  const handlePermissionDecision = async (allow: boolean) => {
    if (!pendingPermission) return;

    // Find the matching option id
    // In Grok: allow-once / reject-once
    let optionId = allow ? 'allow-once' : 'reject-once';
    if (pendingPermission.options?.length > 0) {
      const match = pendingPermission.options.find((o) =>
        allow
          ? o.kind === 'allow_once' || o.optionId.includes('allow')
          : o.kind === 'reject_once' || o.optionId.includes('reject')
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

  const activeCli = systemStatus?.[selectedBackend];
  const isBackendReady = activeCli?.installed && activeCli?.loggedIn;

  return (
    <div className="flex flex-col h-screen bg-neutral-950 text-neutral-100 select-none overflow-hidden">
      {/* Top Header */}
      <header className="h-14 border-b border-neutral-800/80 bg-neutral-900/60 backdrop-blur px-4 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 font-semibold text-sm tracking-tight text-neutral-200">
            <span className="w-2.5 h-2.5 rounded-full bg-blue-500 shadow-[0_0_8px_rgba(59,130,246,0.6)]"></span>
            <span>PX Agent GUI</span>
            <span className="text-xs px-2 py-0.5 rounded bg-neutral-800 text-neutral-400 font-mono">v0.1</span>
          </div>

          {/* Backend Selector Tabs */}
          <div className="flex bg-neutral-800/80 p-0.5 rounded-lg border border-neutral-700/50 ml-4">
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

        {/* Backend Status Indicators */}
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2 text-xs">
            <span className="text-neutral-500">当前后端:</span>
            {loadingStatus ? (
              <span className="text-neutral-400 flex items-center gap-1">
                <RefreshCw className="w-3 h-3 animate-spin" /> 探测中...
              </span>
            ) : isBackendReady ? (
              <span className="text-emerald-400 flex items-center gap-1 font-medium">
                <CheckCircle2 className="w-3.5 h-3.5" /> 已就绪
                {activeCli?.models?.[0] && (
                  <span className="text-neutral-500 font-normal">({activeCli.models[0]})</span>
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

      {/* Workspace Bar */}
      <div className="px-4 py-2 border-b border-neutral-800/60 bg-neutral-900/30 flex items-center gap-2 text-xs shrink-0">
        <Folder className="w-4 h-4 text-neutral-400 shrink-0" />
        <span className="text-neutral-400 shrink-0">工作区 (cwd):</span>
        <input
          type="text"
          value={workspace}
          onChange={(e) => setWorkspace(e.target.value)}
          disabled={isRunning}
          className="flex-1 bg-neutral-950/80 border border-neutral-800 rounded px-2.5 py-1 text-neutral-200 font-mono text-xs focus:outline-none focus:border-blue-500 transition disabled:opacity-50"
          placeholder="/path/to/project/workspace"
        />
        {isRunning && (
          <span className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-blue-500/10 text-blue-400 border border-blue-500/20 shrink-0">
            <span className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-pulse"></span>
            执行中
          </span>
        )}
      </div>

      {/* Main Content Area */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
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
                <h4 className="font-bold text-amber-200 text-sm">CLI 权限执行请求</h4>
                <p className="text-neutral-300 mt-1 font-medium">
                  {pendingPermission.title || `操作工具: ${pendingPermission.toolName}`}
                </p>
                {pendingPermission.command && (
                  <div className="mt-2 p-2 bg-neutral-950 rounded border border-neutral-800 font-mono text-neutral-200 break-all">
                    {pendingPermission.command}
                  </div>
                )}
                <p className="text-neutral-500 mt-1 text-[11px]">
                  安全约束：权限默认不放行。点击拒绝将终止此操作，点击允许仅单次放行。
                </p>
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-neutral-800">
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
              disabled={!prompt.trim() || !isBackendReady}
              className="h-11 px-4 rounded-xl bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 disabled:text-neutral-500 text-white text-xs font-semibold flex items-center gap-1.5 transition shadow-sm disabled:cursor-not-allowed shrink-0"
            >
              <Send className="w-3.5 h-3.5" />
              <span>发送任务</span>
            </button>
          )}
        </div>
      </footer>
    </div>
  );
}
