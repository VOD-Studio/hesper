export type LinuxState =
  | "loading"
  | "running"
  | "paused"
  | "waiting"
  | "failed"
  | "ended";

export type LinuxCommand = { sessionId: number } & (
  | { type: "boot"; files: File[] }
  | { type: "stdin"; bytes: Uint8Array }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "stop" }
);

export type LinuxReply = { sessionId: number } & (
  | { type: "ready" }
  | { type: "stdout"; bytes: Uint8Array }
  | { type: "state"; state: LinuxState; time: bigint; retired: bigint }
  | { type: "error"; message: string }
);
