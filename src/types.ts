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

export interface SystemStatus {
  grok: CliStatus;
  agy: CliStatus;
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
