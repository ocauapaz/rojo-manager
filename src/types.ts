export type Status = "running" | "stopped" | "error";

export interface Project {
  id: string;
  name: string;
  folder: string;
  projectFile: string;
  port: number;
  args: string[];
}

export interface DiscoveredProject {
  name: string;
  folder: string;
  projectFile: string;
  port: number;
  reason: string;
}

export interface LogLine {
  ts: number;
  stream: "stdout" | "stderr" | "system";
  line: string;
}

export interface StatusPayload {
  id: string;
  status: Status;
  code?: number | null;
  message?: string | null;
}
