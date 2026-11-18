export const DEFAULT_INITRAMFS_NAME = "initramfs.cpio";
const MAX_INITRAMFS_BYTES = 16 * 1024 * 1024;

export function validInitramfsSize(bytes: number): boolean {
  return bytes > 0 && bytes <= MAX_INITRAMFS_BYTES;
}
