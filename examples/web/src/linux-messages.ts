export type LinuxState =
  | "loading"
  | "running"
  | "paused"
  | "waiting"
  | "powered-off"
  | "failed"
  | "ended";

export type LinuxProfile = "standard" | "alpine" | "alpine-root";

export function canExportDisk(profile: LinuxProfile, state: LinuxState): boolean {
  return state === (profile === "alpine-root" ? "powered-off" : "paused");
}

export type LinuxCommand = { sessionId: number } & (
  | { type: "boot"; profile: LinuxProfile; files: File[]; initramfs?: File; disk?: File }
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
  | { type: "state"; state: LinuxState; time: bigint; retired: bigint; fuel: bigint }
  | { type: "error"; message: string; invalidInitramfs?: boolean }
);
