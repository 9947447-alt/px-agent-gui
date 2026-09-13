export type BackendType = 'grok' | 'agy';

export interface CliStatus {
  backend: BackendType;
  installed: boolean;
  loggedIn: boolean;
  path: string | null;
  models: string[];
  installCommand: string;
  loginCommand: string;
  error?: string;
}

export interface DesktopAppStatus {
  id: string;
  name: string;
  purpose: string;
  installed: boolean;
  path: string | null;
  installHint: string;
}

export interface SystemStatus {
  grok: CliStatus;
  agy: CliStatus;
  kayg: DesktopAppStatus;
  antigravity: DesktopAppStatus;
}

export interface PermissionOption {
  optionId: string;
  name: string;
  kind?: string;
}

export interface PermissionRequest {
  id: number | string;
  sessionId: string;
  toolName: string;
  title: string;
  command?: string;
  options: PermissionOption[];
  alreadyDenied?: boolean;
}

export interface MessageItem {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  thoughts?: string;
  toolCall?: {
    name: string;
    title?: string;
    status?: string;
  };
  createdAt: number;
}
