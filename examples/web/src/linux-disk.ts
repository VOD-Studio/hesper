const SECTOR_BYTES = 512;
const MAX_DISK_BYTES = 16 * 1024 * 1024;

export function validDiskSize(bytes: number): boolean {
  return bytes > 0 && bytes <= MAX_DISK_BYTES && bytes % SECTOR_BYTES === 0;
}
