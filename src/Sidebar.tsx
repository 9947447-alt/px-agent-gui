import { useState } from 'react';
import {
  Folder,
  FolderPlus,
  MessageSquare,
  Pencil,
  Plus,
} from 'lucide-react';
import { LocalStore } from './types';

function formatSessionTime(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function Sidebar({
  store,
  disabled,
  onNewProject,
  onNewSession,
  onSelectProject,
  onSelectSession,
  onRenameSession,
  onPickDirectory,
}: {
  store: LocalStore;
  disabled: boolean;
  onNewProject: (name: string, workspace: string) => Promise<void>;
  onNewSession: () => void;
  onSelectProject: (projectId: string) => void;
  onSelectSession: (sessionId: string) => void;
  onRenameSession: (sessionId: string, title: string) => void;
  onPickDirectory: (defaultPath?: string) => Promise<string | null>;
}) {
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const [newDir, setNewDir] = useState('');
  const [creatingBusy, setCreatingBusy] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState('');

  const activeProject = store.projects.find((p) => p.id === store.activeProjectId);
  const projectSessions = store.sessions
    .filter((s) => s.projectId === store.activeProjectId)
    .sort((a, b) => b.updatedAt - a.updatedAt);

  const submitProject = async () => {
    if (!newName.trim() || !newDir.trim() || creatingBusy) return;
    setCreatingBusy(true);
    setCreateError(null);
    try {
      await onNewProject(newName.trim(), newDir.trim());
      setNewName('');
      setNewDir('');
      setCreating(false);
    } catch (err: unknown) {
      setCreateError(
        err instanceof Error ? err.message : typeof err === 'string' ? err : String(err)
      );
    } finally {
      setCreatingBusy(false);
    }
  };

  const commitRename = () => {
    if (!renamingId) return;
    const title = renameDraft.trim();
    if (title) onRenameSession(renamingId, title);
    setRenamingId(null);
  };

  return (
    <aside className="w-60 shrink-0 border-r border-neutral-800/80 bg-neutral-950 flex flex-col min-h-0">
      <div className="px-3 py-3 border-b border-neutral-800/80 space-y-2">
        <div className="text-[11px] font-semibold tracking-wide text-neutral-400 uppercase">
          项目
        </div>
        <div className="flex gap-1.5">
          <button
            type="button"
            onClick={() => {
              setCreating((v) => !v);
              setCreateError(null);
            }}
            disabled={disabled}
            className="flex-1 px-2 py-1.5 rounded-md bg-neutral-800 hover:bg-neutral-700 text-[11px] font-medium text-neutral-200 flex items-center justify-center gap-1 disabled:opacity-50"
          >
            <FolderPlus className="w-3.5 h-3.5" />
            新建项目
          </button>
          <button
            type="button"
            onClick={onNewSession}
            disabled={disabled || !store.activeProjectId}
            title={!store.activeProjectId ? '请先选择项目' : '在当前项目中新建对话'}
            className="flex-1 px-2 py-1.5 rounded-md bg-blue-600/90 hover:bg-blue-500 text-[11px] font-medium text-white flex items-center justify-center gap-1 disabled:bg-neutral-800 disabled:text-neutral-500"
          >
            <Plus className="w-3.5 h-3.5" />
            新对话
          </button>
        </div>
      </div>

      {creating && (
        <div className="px-3 py-2.5 border-b border-neutral-800/80 space-y-2">
          <input
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="项目名称"
            disabled={creatingBusy}
            className="w-full bg-neutral-900 border border-neutral-700 rounded-md px-2 py-1.5 text-xs text-neutral-100 placeholder:text-neutral-500 focus:outline-none focus:border-blue-500"
          />
          <button
            type="button"
            onClick={async () => {
              const picked = await onPickDirectory(newDir || undefined);
              if (picked) setNewDir(picked);
            }}
            disabled={creatingBusy}
            className="w-full px-2 py-1.5 rounded-md bg-neutral-800 hover:bg-neutral-700 text-[11px] text-neutral-200 flex items-center gap-1.5"
          >
            <Folder className="w-3.5 h-3.5 shrink-0" />
            <span className="truncate">{newDir || '选择本机文件夹'}</span>
          </button>
          {createError && (
            <p className="text-[11px] text-red-300 leading-snug">{createError}</p>
          )}
          <div className="flex gap-1.5">
            <button
              type="button"
              onClick={() => {
                setCreating(false);
                setCreateError(null);
              }}
              className="flex-1 px-2 py-1 rounded-md text-[11px] text-neutral-400 hover:text-neutral-200"
            >
              取消
            </button>
            <button
              type="button"
              onClick={() => void submitProject()}
              disabled={!newName.trim() || !newDir.trim() || creatingBusy}
              className="flex-1 px-2 py-1 rounded-md bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 disabled:text-neutral-500 text-[11px] text-white font-medium"
            >
              {creatingBusy ? '创建中...' : '创建'}
            </button>
          </div>
        </div>
      )}

      <div className="flex-1 overflow-y-auto py-2">
        {store.projects.length === 0 ? (
          <p className="px-3 py-6 text-[11px] text-neutral-500 leading-relaxed">
            还没有项目。新建项目时填写名称并选择已有本机文件夹，不会创建云端项目。
          </p>
        ) : (
          store.projects.map((project) => {
            const selected = project.id === store.activeProjectId;
            return (
              <div key={project.id} className="px-2 mb-1">
                <button
                  type="button"
                  onClick={() => onSelectProject(project.id)}
                  className={`w-full text-left px-2 py-1.5 rounded-md flex items-start gap-1.5 ${
                    selected ? 'bg-neutral-800 text-neutral-100' : 'text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200'
                  }`}
                >
                  <Folder className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                  <span className="min-w-0">
                    <span className="block text-xs font-medium truncate">{project.name}</span>
                    <span className="block text-[10px] text-neutral-500 truncate font-mono">
                      {project.workspacePath}
                    </span>
                  </span>
                </button>
              </div>
            );
          })
        )}

        {activeProject && (
          <div className="px-2 mt-2">
            <div className="px-2 pb-1 text-[10px] uppercase tracking-wide text-neutral-500">
              对话
            </div>
            {projectSessions.length === 0 ? (
              <p className="px-2 py-2 text-[11px] text-neutral-500">此项目还没有对话</p>
            ) : (
              projectSessions.map((session) => {
                const selected = session.id === store.activeSessionId;
                const renaming = renamingId === session.id;
                return (
                  <div
                    key={session.id}
                    className={`group rounded-md mb-0.5 ${
                      selected ? 'bg-blue-600/20 text-neutral-100' : 'text-neutral-400 hover:bg-neutral-900'
                    }`}
                  >
                    {renaming ? (
                      <input
                        autoFocus
                        value={renameDraft}
                        onChange={(e) => setRenameDraft(e.target.value)}
                        onBlur={commitRename}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') {
                            e.preventDefault();
                            commitRename();
                          } else if (e.key === 'Escape') {
                            setRenamingId(null);
                          }
                        }}
                        className="w-full bg-neutral-900 border border-blue-500 rounded-md px-2 py-1.5 text-xs text-neutral-100 focus:outline-none"
                      />
                    ) : (
                      <div className="flex items-start gap-1 px-1 py-1">
                        <button
                          type="button"
                          onClick={() => onSelectSession(session.id)}
                          onDoubleClick={() => {
                            setRenamingId(session.id);
                            setRenameDraft(session.title);
                          }}
                          className="flex-1 min-w-0 text-left px-1 py-0.5 flex items-start gap-1.5"
                        >
                          <MessageSquare className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                          <span className="min-w-0 flex-1">
                            <span className="block text-xs truncate">{session.title}</span>
                            <span className="block text-[10px] text-neutral-500">
                              {formatSessionTime(session.updatedAt)}
                            </span>
                          </span>
                        </button>
                        <button
                          type="button"
                          title="重命名"
                          onClick={() => {
                            setRenamingId(session.id);
                            setRenameDraft(session.title);
                          }}
                          className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-neutral-800 text-neutral-400 shrink-0"
                        >
                          <Pencil className="w-3 h-3" />
                        </button>
                      </div>
                    )}
                  </div>
                );
              })
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
