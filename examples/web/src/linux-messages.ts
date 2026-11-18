export type LinuxState =
  | "loading"
  | "running"
  | "paused"
  | "waiting"
  | "failed"
  | "ended";

export type LinuxCommand = { sessionId: number } & (
  | { type: "boot"; files: File[]; disk?: File }
  | { type: "stdin"; bytes: Uint8Array }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "export" }
  | { type: "exported" }
  | { type: "stop"; discard?: boolean }
);

export type LinuxReply = { sessionId: number } & (
  | { type: "ready" }
  | { type: "stdout"; bytes: Uint8Array }
  | { type: "disk"; bytes: Uint8Array<ArrayBuffer> }
  | { type: "discard-required" }
  | { type: "state"; state: LinuxState; time: bigint; retired: bigint }
  | { type: "error"; message: string }
);
